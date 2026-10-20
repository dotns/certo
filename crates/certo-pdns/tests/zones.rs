//! Zone service behaviour against the mock PowerDNS API and a scripted delegation checker.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::doc_markdown,
    reason = "test code"
)]

use std::sync::Arc;

use certo_db::Db;
use certo_pdns::{
    ChangeType, Delegation, DelegationChecker, RrSetChange, ZoneOpError, ZonePolicy, Zones,
    mock::{MockPdns, ScriptedChecker},
};

struct Harness {
    db: Db,
    pdns: MockPdns,
    checker: Arc<ScriptedChecker>,
    zones: Zones,
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("certo.db");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let pdns = MockPdns::start().await.unwrap();
    let checker = Arc::new(ScriptedChecker::default());
    let zones = Zones::new(
        db.clone(),
        pdns.client(),
        ZonePolicy {
            base_domain: "dns.example.net".into(),
            nsname: "ns1.dns.example.net".into(),
        },
        Arc::clone(&checker) as Arc<dyn DelegationChecker>,
    );
    Harness {
        db,
        pdns,
        checker,
        zones,
        _dir: dir,
        path,
    }
}

impl Harness {
    /// Runs raw SQL on the test database (to move timestamps around).
    fn sql(&self, sql: &str) {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }
}

fn change(name: &str, rtype: &str, records: &[&str]) -> RrSetChange {
    RrSetChange {
        name: name.into(),
        rtype: rtype.into(),
        records: records.iter().map(|r| (*r).to_owned()).collect(),
        ..RrSetChange::default()
    }
}

#[tokio::test]
async fn base_zone_is_seeded_once_and_serves_challenge_txt() {
    let h = harness().await;
    let seed = vec![
        "dns.example.net. A 198.51.100.1".to_owned(),
        "dns.example.net. 300 IN NS ns1.dns.example.net.".to_owned(),
        "other.example.org. A 192.0.2.1".to_owned(),
        "garbage".to_owned(),
    ];
    h.zones.ensure_base_zone(&seed).await.unwrap();
    let base = h.db.zone("dns.example.net").await.unwrap();
    assert!(base.is_system() && base.active && base.exempt);
    let served = h.pdns.zone("dns.example.net").unwrap();
    assert_eq!(served.contents("dns.example.net", "A"), ["198.51.100.1"]);
    assert_eq!(
        served.contents("dns.example.net", "NS"),
        ["ns1.dns.example.net."]
    );
    assert!(h.pdns.zone("other.example.org").is_none());

    // Second start: nothing is re-seeded.
    h.zones
        .ensure_base_zone(&["dns.example.net. A 203.0.113.9".to_owned()])
        .await
        .unwrap();
    assert_eq!(
        h.pdns
            .zone("dns.example.net")
            .unwrap()
            .contents("dns.example.net", "A"),
        ["198.51.100.1"]
    );

    // Challenge TXT values follow the database.
    h.db.present_txt("abc.dns.example.net", "t1").await.unwrap();
    h.db.present_txt("abc.dns.example.net", "t2").await.unwrap();
    h.zones.sync_txt("abc.dns.example.net").await.unwrap();
    let z = h.pdns.zone("dns.example.net").unwrap();
    assert_eq!(
        z.contents("abc.dns.example.net", "TXT"),
        ["\"t1\"", "\"t2\""]
    );
    let rr = z
        .rrsets
        .iter()
        .find(|r| r.name == "abc.dns.example.net.")
        .unwrap();
    assert_eq!(rr.ttl, 1);
    h.db.cleanup_txt("abc.dns.example.net", "t1").await.unwrap();
    h.db.cleanup_txt("abc.dns.example.net", "t2").await.unwrap();
    h.zones.sync_txt("abc.dns.example.net").await.unwrap();
    assert!(
        h.pdns
            .zone("dns.example.net")
            .unwrap()
            .contents("abc.dns.example.net", "TXT")
            .is_empty()
    );
    assert!(matches!(
        h.zones.sync_txt("abc.example.org").await,
        Err(ZoneOpError::Invalid {
            code: "name_outside_zone",
            ..
        })
    ));

    // Own certificate challenge.
    h.zones.set_own_challenge(Some("own-token")).await.unwrap();
    assert_eq!(
        h.pdns
            .zone("dns.example.net")
            .unwrap()
            .contents("_acme-challenge.dns.example.net", "TXT"),
        ["\"own-token\""]
    );
    h.zones.set_own_challenge(None).await.unwrap();
    assert!(
        h.pdns
            .zone("dns.example.net")
            .unwrap()
            .contents("_acme-challenge.dns.example.net", "TXT")
            .is_empty()
    );

    // Reconcile: stale TTL-1 TXT rrsets go, database values come back, own token is kept.
    h.zones.set_own_challenge(Some("own-token")).await.unwrap();
    h.pdns
        .client()
        .patch_rrsets(
            "dns.example.net",
            &[certo_pdns::RrSet::replace(
                "stale.dns.example.net.",
                "TXT",
                1,
                vec![certo_pdns::RecordData::new("\"x\"")],
            )],
        )
        .await
        .unwrap();
    h.db.present_txt("fresh.dns.example.net", "f")
        .await
        .unwrap();
    let n = h.zones.reconcile_txt().await.unwrap();
    assert_eq!(n, 2);
    let z = h.pdns.zone("dns.example.net").unwrap();
    assert!(z.contents("stale.dns.example.net", "TXT").is_empty());
    assert_eq!(z.contents("fresh.dns.example.net", "TXT"), ["\"f\""]);
    assert_eq!(
        z.contents("_acme-challenge.dns.example.net", "TXT"),
        ["\"own-token\""]
    );
}

#[tokio::test]
async fn zones_activate_only_when_delegated() {
    let h = harness().await;
    h.zones.ensure_base_zone(&[]).await.unwrap();
    let alice = h.db.create_user("alice", "h").await.unwrap();

    assert!(matches!(
        h.zones.create(Some(alice.id), "x.dns.example.net").await,
        Err(ZoneOpError::NotAllowed(_))
    ));
    assert!(matches!(
        h.zones.create(Some(alice.id), "com").await,
        Err(ZoneOpError::Invalid {
            code: "invalid_name",
            ..
        })
    ));

    let zone = h
        .zones
        .create(Some(alice.id), "Example.COM.")
        .await
        .unwrap();
    assert_eq!(zone.name, "example.com");
    assert!(!zone.active);
    assert_eq!(zone.last_error.as_deref(), Some("not delegated"));
    assert_eq!(zone.failed_checks, 1);
    assert!(h.pdns.zone("example.com").is_none());
    assert!(matches!(
        h.zones.rrsets(&zone).await,
        Err(ZoneOpError::NotActive)
    ));
    assert!(matches!(
        h.zones
            .patch_rrsets(&zone, &[change("www", "A", &["192.0.2.1"])])
            .await,
        Err(ZoneOpError::NotActive)
    ));
    assert!(matches!(
        h.zones.verify(&zone).await,
        Err(ZoneOpError::RateLimited)
    ));
    assert!(matches!(
        h.zones.create(Some(alice.id), "example.com").await,
        Err(ZoneOpError::Exists)
    ));

    // Inconclusive checks are reported but not recorded.
    h.checker.set(Err("resolver down".into()));
    let other = h.zones.create(Some(alice.id), "other.test").await.unwrap();
    assert!(other.last_check.is_none() && other.last_error.is_none());

    // Delegated: the zone appears in PowerDNS with the apex NS.
    h.checker.set(Ok(Delegation::Delegated));
    let active = h.zones.verify(&other).await.unwrap();
    assert!(active.active && !active.exempt && active.verified_at.is_some());
    let served = h.pdns.zone("other.test").unwrap();
    assert_eq!(
        served.contents("other.test", "NS"),
        ["ns1.dns.example.net."]
    );
    assert_eq!(h.zones.get("Other.Test").await.unwrap().id, other.id);

    // Records.
    h.zones
        .patch_rrsets(
            &active,
            &[
                change("www", "a", &["192.0.2.1"]),
                change("@", "MX", &["10 mail.other.test."]),
            ],
        )
        .await
        .unwrap();
    let rrsets = h.zones.rrsets(&active).await.unwrap();
    let www = rrsets.iter().find(|r| r.name == "www.other.test.").unwrap();
    assert_eq!(
        (www.rtype.as_str(), www.ttl, www.changetype),
        ("A", 3600, None)
    );
    assert!(matches!(
        h.zones
            .patch_rrsets(&active, &[change("www.example.org.", "A", &["192.0.2.1"])])
            .await,
        Err(ZoneOpError::Invalid {
            code: "name_outside_zone",
            ..
        })
    ));
    // Content validation is PowerDNS': a broken TXT quoting comes back as pdns_rejected.
    assert!(matches!(
        h.zones
            .patch_rrsets(&active, &[change("t", "TXT", &["\"a\" b"])])
            .await,
        Err(ZoneOpError::Invalid {
            code: "pdns_rejected",
            ..
        })
    ));
    h.zones
        .patch_rrsets(
            &active,
            &[RrSetChange {
                changetype: Some(ChangeType::Delete),
                ..change("www", "A", &[])
            }],
        )
        .await
        .unwrap();
    assert!(
        h.pdns
            .zone("other.test")
            .unwrap()
            .contents("www.other.test", "A")
            .is_empty()
    );

    // Delete removes both sides.
    h.zones.delete(&active).await.unwrap();
    assert!(h.pdns.zone("other.test").is_none());
    assert!(matches!(
        h.zones.get("other.test").await,
        Err(ZoneOpError::NotFound("zone"))
    ));
    let base = h.zones.get("dns.example.net").await.unwrap();
    assert!(matches!(
        h.zones.delete(&base).await,
        Err(ZoneOpError::NotAllowed(_))
    ));
    assert!(matches!(
        h.zones.verify(&base).await,
        Err(ZoneOpError::NotAllowed(_))
    ));
}

#[tokio::test]
async fn exemption_dnssec_and_transfers() {
    let h = harness().await;
    h.zones.ensure_base_zone(&[]).await.unwrap();
    let alice = h.db.create_user("alice", "h").await.unwrap();
    let pending = h.zones.create(Some(alice.id), "exempt.test").await.unwrap();
    assert!(!pending.active);

    let zone = h.zones.set_exempt(&pending, true).await.unwrap();
    assert!(zone.active && zone.exempt);
    assert!(h.pdns.zone("exempt.test").is_some());

    let info = h.zones.dnssec(&zone).await.unwrap();
    assert!(!info.enabled && info.ds.is_empty());
    let on = h.zones.set_dnssec(&zone, true, "nsec3").await.unwrap();
    assert!(on.enabled);
    assert_eq!(on.nsec_mode, "nsec3");
    assert_eq!(on.ds.len(), 1);
    assert!(on.dnskey.is_some());
    assert!(matches!(
        h.zones.set_dnssec(&zone, true, "nsec5").await,
        Err(ZoneOpError::Invalid {
            code: "invalid_nsec_mode",
            ..
        })
    ));
    let off = h.zones.set_dnssec(&zone, false, "nsec").await.unwrap();
    assert!(!off.enabled && off.ds.is_empty());
    assert_eq!(off.nsec_mode, "nsec");

    let t = h.zones.transfer(&zone).await.unwrap();
    assert!(t.allow_from.is_empty() && t.tsig_keys.is_empty());
    let t = h
        .zones
        .set_transfer_allow_from(&zone, &["192.0.2.0/24".into(), "2001:db8::1".into()])
        .await
        .unwrap();
    assert_eq!(t.allow_from, ["192.0.2.0/24", "2001:db8::1/128"]);
    assert!(matches!(
        h.zones
            .set_transfer_allow_from(&zone, &["nope".into()])
            .await,
        Err(ZoneOpError::Invalid {
            code: "invalid_cidr",
            ..
        })
    ));
    let (key, secret) = h
        .zones
        .add_tsig_key(&zone, "Xfer", "hmac-sha256")
        .await
        .unwrap();
    assert_eq!(
        (key.name.as_str(), key.algorithm.as_str()),
        ("xfer", "hmac-sha256")
    );
    assert!(!secret.is_empty());
    assert!(matches!(
        h.zones.add_tsig_key(&zone, "xfer", "hmac-sha256").await,
        Err(ZoneOpError::Invalid {
            code: "tsig_key_exists",
            ..
        })
    ));
    assert!(matches!(
        h.zones.add_tsig_key(&zone, "k2", "hmac-md5").await,
        Err(ZoneOpError::Invalid {
            code: "invalid_algorithm",
            ..
        })
    ));
    let t = h.zones.transfer(&zone).await.unwrap();
    assert_eq!(t.tsig_keys.len(), 1);
    assert_eq!(t.tsig_keys[0].algorithm, "hmac-sha256");
    assert!(matches!(
        h.zones.delete_tsig_key(&zone, "ghost").await,
        Err(ZoneOpError::NotFound("tsig key"))
    ));
    h.zones.delete_tsig_key(&zone, "xfer").await.unwrap();
    assert!(h.zones.transfer(&zone).await.unwrap().tsig_keys.is_empty());
    assert!(h.pdns.tsig_keys().is_empty());

    // Back to the normal cycle: still active, no longer exempt.
    let normal = h.zones.set_exempt(&zone, false).await.unwrap();
    assert!(normal.active && !normal.exempt);
}

#[tokio::test]
async fn repeated_failures_deactivate_and_snapshots_restore() {
    let h = harness().await;
    h.zones.ensure_base_zone(&[]).await.unwrap();
    let alice = h.db.create_user("alice", "h").await.unwrap();
    h.checker.set(Ok(Delegation::Delegated));
    let zone = h.zones.create(Some(alice.id), "flaky.test").await.unwrap();
    assert!(zone.active);
    h.zones
        .patch_rrsets(&zone, &[change("www", "A", &["192.0.2.7"])])
        .await
        .unwrap();

    // Daily checks fail; the zone survives six failures and goes after the seventh.
    h.checker
        .set(Ok(Delegation::NotDelegated("moved away".into())));
    let mut current = zone.clone();
    for i in 1..certo_pdns::MAX_FAILED_CHECKS {
        h.db.record_zone_check_failure(current.id, "moved away")
            .await
            .unwrap();
        current = h.db.zone("flaky.test").await.unwrap();
        assert!(current.active, "still active after {i} failures");
    }
    // run_checks skips zones checked recently, so force the check directly.
    let report = h.zones.run_checks().await.unwrap();
    assert_eq!(report.checked, 0);
    let recent = h.db.zone("flaky.test").await.unwrap();
    assert!(matches!(
        h.zones.verify(&recent).await,
        Err(ZoneOpError::RateLimited)
    ));
    // Two days later the daily re-check runs and the seventh failure deactivates the zone.
    h.sql("UPDATE zones SET last_check = last_check - 2*24*3600 WHERE name='flaky.test'");
    let report = h.zones.run_checks().await.unwrap();
    assert_eq!((report.checked, report.deactivated), (1, 1));
    let off = h.db.zone("flaky.test").await.unwrap();
    assert!(!off.active);
    assert!(h.pdns.zone("flaky.test").is_none());
    assert!(
        h.db.zone_snapshot(off.id)
            .await
            .unwrap()
            .unwrap()
            .contains("192.0.2.7")
    );

    // Delegation comes back: records are restored from the snapshot.
    h.checker.set(Ok(Delegation::Delegated));
    h.sql("UPDATE zones SET last_check = last_check - 2*3600 WHERE name='flaky.test'");
    let report = h.zones.run_checks().await.unwrap();
    assert_eq!((report.checked, report.activated), (1, 1));
    let served = h.pdns.zone("flaky.test").unwrap();
    assert_eq!(served.contents("www.flaky.test", "A"), ["192.0.2.7"]);
    assert_eq!(
        served.contents("flaky.test", "NS"),
        ["ns1.dns.example.net."]
    );

    // Pending zones nobody activates expire.
    h.checker.set(Ok(Delegation::NotDelegated("no".into())));
    let old = h
        .zones
        .create(Some(alice.id), "abandoned.test")
        .await
        .unwrap();
    h.sql("UPDATE zones SET created_at = created_at - 31*24*3600 WHERE name='abandoned.test'");
    let report = h.zones.run_checks().await.unwrap();
    assert_eq!(report.expired, 1);
    assert!(matches!(
        h.db.zone(&old.name).await,
        Err(certo_db::DbError::NotFound(_))
    ));
}
