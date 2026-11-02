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
    harness_with("dns.example.net", "dns.example.net").await
}

async fn harness_with(base_zone: &str, base_domain: &str) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("certo.db");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let pdns = MockPdns::start().await.unwrap();
    let checker = Arc::new(ScriptedChecker::default());
    let zones = Zones::new(
        db.clone(),
        pdns.client(),
        ZonePolicy {
            base_domain: base_domain.into(),
            base_zone: base_zone.into(),
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
    assert!(h.pdns.zone("dns.example.net").is_none());
    let report = h.zones.sync(false).await.unwrap();
    assert_eq!((report.created, report.rrsets_changed), (1, 2));
    assert!(report.status.zones.is_empty());
    assert_eq!(report.status.db_version, report.status.pdns_version);
    let served = h.pdns.zone("dns.example.net").unwrap();
    assert_eq!(served.contents("dns.example.net", "A"), ["198.51.100.1"]);
    assert_eq!(
        served.contents("dns.example.net", "NS"),
        ["ns1.dns.example.net."]
    );
    assert_eq!(
        served.metadata.get(certo_pdns::VERSION_METADATA).unwrap(),
        &[base.version.to_string()]
    );
    assert!(h.pdns.zone("other.example.org").is_none());

    // Second start: nothing is re-seeded, nothing to converge.
    h.zones
        .ensure_base_zone(&["dns.example.net. A 203.0.113.9".to_owned()])
        .await
        .unwrap();
    let report = h.zones.sync(false).await.unwrap();
    assert_eq!((report.created, report.rrsets_changed), (0, 0));
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

    // Sync: stale TXT rrsets go, database values come back, the own token is kept.
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
    let status = h.zones.status().await.unwrap();
    assert_eq!(status.zones.len(), 1);
    assert_eq!(status.zones[0].name, "dns.example.net");
    assert_eq!(status.db_version, status.pdns_version + 1);
    let report = h.zones.sync(false).await.unwrap();
    assert_eq!(report.rrsets_changed, 2);
    assert!(report.status.zones.is_empty());
    let z = h.pdns.zone("dns.example.net").unwrap();
    assert!(z.contents("stale.dns.example.net", "TXT").is_empty());
    assert_eq!(z.contents("fresh.dns.example.net", "TXT"), ["\"f\""]);
    assert_eq!(
        z.contents("_acme-challenge.dns.example.net", "TXT"),
        ["\"own-token\""]
    );
}

#[tokio::test]
async fn base_zone_above_the_challenge_domain() {
    let h = harness_with("corp.example.net", "dns.corp.example.net").await;
    let seed = vec![
        "corp.example.net. A 198.51.100.1".to_owned(),
        "dns.corp.example.net. A 198.51.100.1".to_owned(),
    ];
    h.zones.ensure_base_zone(&seed).await.unwrap();
    h.zones.sync(false).await.unwrap();
    assert!(h.db.zone("corp.example.net").await.unwrap().is_system());
    assert!(h.db.zone("dns.corp.example.net").await.is_err());
    assert!(h.pdns.zone("dns.corp.example.net").is_none());
    let z = h.pdns.zone("corp.example.net").unwrap();
    assert_eq!(z.contents("corp.example.net", "A"), ["198.51.100.1"]);
    assert_eq!(z.contents("dns.corp.example.net", "A"), ["198.51.100.1"]);
    assert_eq!(
        z.contents("corp.example.net", "NS"),
        ["ns1.dns.example.net."]
    );

    // Challenge names stay under the challenge domain but land in the zone.
    h.db.present_txt("abc.dns.corp.example.net", "t1")
        .await
        .unwrap();
    h.zones.sync_txt("abc.dns.corp.example.net").await.unwrap();
    h.zones.set_own_challenge(Some("own")).await.unwrap();
    let z = h.pdns.zone("corp.example.net").unwrap();
    assert_eq!(z.contents("abc.dns.corp.example.net", "TXT"), ["\"t1\""]);
    assert_eq!(
        z.contents("_acme-challenge.corp.example.net", "TXT"),
        ["\"own\""]
    );
    assert_eq!(h.zones.sync(true).await.unwrap().rrsets_changed, 0);
    assert_eq!(
        h.pdns
            .zone("corp.example.net")
            .unwrap()
            .contents("_acme-challenge.corp.example.net", "TXT"),
        ["\"own\""]
    );
    assert!(matches!(
        h.zones.sync_txt("abc.corp.example.net").await,
        Err(ZoneOpError::Invalid {
            code: "name_outside_zone",
            ..
        })
    ));

    // Users may not take the zone or anything under it, but may own an ancestor.
    let alice = h.db.create_user("alice", "h").await.unwrap();
    for name in [
        "corp.example.net",
        "dns.corp.example.net",
        "x.dns.corp.example.net",
    ] {
        assert!(
            matches!(
                h.zones.create(Some(alice.id), name).await,
                Err(ZoneOpError::NotAllowed(_))
            ),
            "{name}"
        );
    }
    let apex = h.zones.create(Some(alice.id), "example.net").await.unwrap();
    assert_eq!(apex.name, "example.net");
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
async fn repeated_failures_deactivate_and_stored_records_restore() {
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
    assert!(matches!(
        h.zones.rrsets(&off).await,
        Err(ZoneOpError::NotActive)
    ));
    assert_eq!(h.db.zone_rrsets(off.id).await.unwrap().len(), 2);

    // Delegation comes back: records are restored from the database.
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

#[tokio::test]
async fn public_suffixes_cannot_become_zones() {
    let h = harness().await;
    h.zones.ensure_base_zone(&[]).await.unwrap();
    let alice = h.db.create_user("alice", "h").await.unwrap();
    let bob = h.db.create_user("bob", "h").await.unwrap();

    for suffix in ["in-addr.arpa", "IP6.ARPA.", "co.uk", "github.io"] {
        assert!(
            matches!(
                h.zones.create(Some(alice.id), suffix).await,
                Err(ZoneOpError::NotAllowed(_))
            ),
            "{suffix}"
        );
    }
    // Nothing was stored, so zones below those suffixes stay available to others.
    for name in ["23.251.162.in-addr.arpa", "example.co.uk", "example.test"] {
        h.zones.create(Some(bob.id), name).await.unwrap();
    }
}

#[tokio::test]
async fn sync_rebuilds_pdns_from_the_database() {
    let h = harness().await;
    h.zones
        .ensure_base_zone(&["dns.example.net. A 198.51.100.1".to_owned()])
        .await
        .unwrap();
    assert_eq!(h.zones.sync(false).await.unwrap().created, 1);
    let alice = h.db.create_user("alice", "h").await.unwrap();
    let pending = h.zones.create(Some(alice.id), "keyed.test").await.unwrap();
    let zone = h.zones.set_exempt(&pending, true).await.unwrap();
    h.zones
        .patch_rrsets(&zone, &[change("www", "A", &["192.0.2.10"])])
        .await
        .unwrap();
    let on = h.zones.set_dnssec(&zone, true, "nsec3").await.unwrap();
    let (_, secret) = h
        .zones
        .add_tsig_key(&zone, "xfr", "hmac-sha256")
        .await
        .unwrap();
    h.zones
        .set_transfer_allow_from(&zone, &["192.0.2.0/24".into()])
        .await
        .unwrap();
    let status = h.zones.status().await.unwrap();
    assert!(status.zones.is_empty(), "{status:?}");
    assert_eq!(status.db_version, status.pdns_version);
    let zone = h.zones.get("keyed.test").await.unwrap();
    assert_eq!(
        h.pdns
            .zone("keyed.test")
            .unwrap()
            .metadata
            .get(certo_pdns::VERSION_METADATA)
            .unwrap(),
        &[zone.version.to_string()]
    );

    // pdns lost everything: both zones come back with records, key, TSIG and allow list.
    h.pdns.reset();
    let status = h.zones.status().await.unwrap();
    assert_eq!(status.zones.len(), 2);
    assert_eq!(status.pdns_version, 0);
    let report = h.zones.sync(false).await.unwrap();
    assert_eq!((report.created, report.keys_imported), (2, 1));
    assert!(report.status.zones.is_empty());
    let served = h.pdns.zone("keyed.test").unwrap();
    assert_eq!(served.contents("www.keyed.test", "A"), ["192.0.2.10"]);
    assert_eq!(
        served.contents("keyed.test", "NS"),
        ["ns1.dns.example.net."]
    );
    assert_eq!(served.nsec3param, "1 0 0 -");
    assert_eq!(
        served.metadata.get("ALLOW-AXFR-FROM").unwrap(),
        &["192.0.2.0/24"]
    );
    assert_eq!(served.metadata.get("TSIG-ALLOW-AXFR").unwrap(), &["xfr"]);
    let again = h.zones.dnssec(&zone).await.unwrap();
    assert_eq!((again.enabled, again.nsec_mode.as_str()), (true, "nsec3"));
    assert_eq!(again.ds, on.ds);
    let keys = h.pdns.tsig_keys();
    assert_eq!(keys.len(), 1);
    assert_eq!(
        (keys[0].name.as_str(), keys[0].key.as_str()),
        ("xfr", secret.as_str())
    );
    assert_eq!(
        h.pdns
            .zone("dns.example.net")
            .unwrap()
            .contents("dns.example.net", "A"),
        ["198.51.100.1"]
    );

    // Zones and keys the database does not know are removed.
    h.pdns
        .client()
        .create_zone(&certo_pdns::NewZone::native(
            "orphan.test.",
            vec!["ns1.dns.example.net.".into()],
            Vec::new(),
        ))
        .await
        .unwrap();
    h.pdns
        .client()
        .create_tsig_key(&certo_pdns::TsigKey {
            id: String::new(),
            name: "stray".into(),
            algorithm: "hmac-sha256".into(),
            key: String::new(),
        })
        .await
        .unwrap();
    let status = h.zones.status().await.unwrap();
    assert_eq!(status.zones.len(), 1);
    assert_eq!(
        (
            status.zones[0].name.as_str(),
            status.zones[0].db,
            status.zones[0].pdns
        ),
        ("orphan.test", None, None)
    );
    let report = h.zones.sync(false).await.unwrap();
    assert_eq!(report.removed, 1);
    assert!(h.pdns.zone("orphan.test").is_none());
    assert_eq!(h.pdns.tsig_keys().len(), 1);

    // A change made behind certo's back keeps the stamp, so only a forced sync undoes it.
    h.pdns
        .client()
        .patch_rrsets(
            "keyed.test",
            &[certo_pdns::RrSet::replace(
                "rogue.keyed.test.",
                "A",
                60,
                vec![certo_pdns::RecordData::new("203.0.113.1")],
            )],
        )
        .await
        .unwrap();
    assert!(h.zones.status().await.unwrap().zones.is_empty());
    assert_eq!(h.zones.sync(false).await.unwrap().rrsets_changed, 0);
    assert_eq!(h.zones.sync(true).await.unwrap().rrsets_changed, 1);
    assert!(
        h.pdns
            .zone("keyed.test")
            .unwrap()
            .contents("rogue.keyed.test", "A")
            .is_empty()
    );

    // A pdns zone without a stamp is adopted into the database instead of overwritten.
    let row =
        h.db.create_zone(Some(alice.id), "adopt.test")
            .await
            .unwrap();
    h.db.activate_zone(row.id, true).await.unwrap();
    h.pdns
        .client()
        .create_zone(&certo_pdns::NewZone::native(
            "adopt.test.",
            vec!["ns1.dns.example.net.".into()],
            vec![certo_pdns::RrSet::replace(
                "www.adopt.test.",
                "A",
                300,
                vec![certo_pdns::RecordData::new("192.0.2.77")],
            )],
        ))
        .await
        .unwrap();
    h.pdns
        .client()
        .add_csk("adopt.test", "ecdsa256")
        .await
        .unwrap();
    let report = h.zones.sync(false).await.unwrap();
    assert_eq!((report.records_adopted, report.keys_imported), (2, 0));
    let stored = h.db.zone_rrsets(row.id).await.unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[1].name, "www.adopt.test.");
    assert_eq!(stored[1].records, ["192.0.2.77"]);
    assert!(h.db.zone_dnssec(row.id).await.unwrap().is_some());
    let served = h.pdns.zone("adopt.test").unwrap();
    assert_eq!(served.contents("www.adopt.test", "A"), ["192.0.2.77"]);
    assert_eq!(served.keys.len(), 1);
    assert!(served.metadata.contains_key(certo_pdns::VERSION_METADATA));
    assert!(h.zones.status().await.unwrap().zones.is_empty());
}
