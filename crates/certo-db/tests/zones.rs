//! Zone ownership, activation state and the zone data `PowerDNS` is rebuilt from.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    reason = "test code"
)]

use certo_core::{ZoneDnssec, ZoneRrSet, ZoneTsigKey};
use certo_db::{Db, DbError};

fn rrset(name: &str, rtype: &str, records: &[&str]) -> ZoneRrSet {
    ZoneRrSet {
        name: name.into(),
        rtype: rtype.into(),
        ttl: 300,
        records: records.iter().map(|r| (*r).to_owned()).collect(),
    }
}

#[tokio::test]
async fn zones_start_pending_and_track_activation() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let zone = db
        .create_zone(Some(alice.id), " Example.COM ")
        .await
        .unwrap();
    assert_eq!(zone.name, "example.com");
    assert_eq!(zone.owner.as_deref(), Some("alice"));
    assert!(!zone.active && !zone.exempt && !zone.is_system());
    assert_eq!(
        (zone.verified_at, zone.last_check, zone.failed_checks),
        (None, None, 0)
    );
    assert_eq!(zone.version, 1);
    assert!(zone.axfr_allow_from.is_empty());
    assert!(matches!(
        db.create_zone(Some(alice.id), "example.com").await,
        Err(DbError::Conflict("zone"))
    ));

    let failed = db
        .record_zone_check_failure(zone.id, "not delegated")
        .await
        .unwrap();
    assert_eq!(failed.last_error.as_deref(), Some("not delegated"));
    assert_eq!(failed.failed_checks, 1);
    assert!(failed.last_check.is_some() && !failed.active);
    assert_eq!(failed.version, 1);

    let active = db.activate_zone(zone.id, false).await.unwrap();
    assert!(active.active && !active.exempt);
    assert_eq!(active.failed_checks, 0);
    assert!(active.last_error.is_none() && active.verified_at.is_some());
    assert_eq!(active.version, 2);

    let off = db.deactivate_zone(zone.id).await.unwrap();
    assert!(!off.active);
    assert_eq!(off.version, 3);
    let exempt = db.activate_zone(zone.id, true).await.unwrap();
    assert!(exempt.active && exempt.exempt);
    let normal = db.clear_zone_exemption(zone.id).await.unwrap();
    assert!(normal.active && !normal.exempt);
    assert_eq!(normal.version, 4);

    db.delete_zone(zone.id).await.unwrap();
    assert!(matches!(
        db.zone("example.com").await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(matches!(
        db.delete_zone(zone.id).await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(matches!(
        db.activate_zone(zone.id, false).await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(matches!(
        db.deactivate_zone(zone.id).await,
        Err(DbError::NotFound("zone"))
    ));
}

#[tokio::test]
async fn zone_data_is_versioned() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let zone = db.create_zone(Some(alice.id), "example.com").await.unwrap();
    assert!(db.zone_rrsets(zone.id).await.unwrap().is_empty());

    // Rrsets: an empty record list removes the rrset; the rest are replaced whole.
    let v = db
        .apply_zone_rrsets(
            zone.id,
            &[
                rrset("www.example.com.", "A", &["192.0.2.1", "192.0.2.2"]),
                rrset("example.com.", "NS", &["ns1.example.net."]),
            ],
        )
        .await
        .unwrap();
    assert_eq!(v, 2);
    let v = db
        .apply_zone_rrsets(
            zone.id,
            &[
                rrset("www.example.com.", "A", &["192.0.2.3"]),
                rrset("example.com.", "NS", &[]),
                rrset("mail.example.com.", "MX", &["10 mx.example.com."]),
            ],
        )
        .await
        .unwrap();
    assert_eq!(v, 3);
    let stored = db.zone_rrsets(zone.id).await.unwrap();
    assert_eq!(
        stored,
        [
            rrset("mail.example.com.", "MX", &["10 mx.example.com."]),
            rrset("www.example.com.", "A", &["192.0.2.3"]),
        ]
    );
    assert_eq!(db.zone("example.com").await.unwrap().version, 3);
    let v = db
        .replace_zone_rrsets(zone.id, &[rrset("example.com.", "A", &["192.0.2.9"])])
        .await
        .unwrap();
    assert_eq!(v, 4);
    assert_eq!(
        db.zone_rrsets(zone.id).await.unwrap(),
        [rrset("example.com.", "A", &["192.0.2.9"])]
    );

    // DNSSEC key content.
    assert_eq!(db.zone_dnssec(zone.id).await.unwrap(), None);
    let key = ZoneDnssec {
        nsec3: true,
        algorithm: "ECDSAP256SHA256".into(),
        private_key: "Private-key-format: v1.2\nPrivateKey: abc\n".into(),
    };
    assert_eq!(db.set_zone_dnssec(zone.id, Some(&key)).await.unwrap(), 5);
    assert_eq!(db.zone_dnssec(zone.id).await.unwrap(), Some(key.clone()));
    let nsec = ZoneDnssec {
        nsec3: false,
        ..key
    };
    db.set_zone_dnssec(zone.id, Some(&nsec)).await.unwrap();
    assert_eq!(db.zone_dnssec(zone.id).await.unwrap(), Some(nsec));
    assert_eq!(db.set_zone_dnssec(zone.id, None).await.unwrap(), 7);
    assert_eq!(db.zone_dnssec(zone.id).await.unwrap(), None);

    // TSIG keys and the transfer allow list.
    let tsig = ZoneTsigKey {
        name: "xfr".into(),
        algorithm: "hmac-sha256".into(),
        secret: "c2VjcmV0".into(),
    };
    assert_eq!(db.add_zone_tsig_key(zone.id, &tsig).await.unwrap(), 8);
    assert!(matches!(
        db.add_zone_tsig_key(zone.id, &tsig).await,
        Err(DbError::Conflict("tsig key"))
    ));
    assert_eq!(
        db.zone_tsig_keys(zone.id).await.unwrap(),
        std::slice::from_ref(&tsig)
    );
    assert!(matches!(
        db.delete_zone_tsig_key(zone.id, "ghost").await,
        Err(DbError::NotFound("tsig key"))
    ));
    assert_eq!(db.delete_zone_tsig_key(zone.id, "xfr").await.unwrap(), 9);
    assert!(db.zone_tsig_keys(zone.id).await.unwrap().is_empty());
    assert_eq!(
        db.set_zone_axfr_allow_from(zone.id, &["192.0.2.0/24".into()])
            .await
            .unwrap(),
        10
    );
    assert_eq!(
        db.zone("example.com").await.unwrap().axfr_allow_from,
        ["192.0.2.0/24"]
    );

    // Unknown zones.
    assert!(matches!(
        db.apply_zone_rrsets(999, &[]).await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(matches!(
        db.set_zone_dnssec(999, None).await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(matches!(
        db.add_zone_tsig_key(999, &tsig).await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(matches!(
        db.set_zone_axfr_allow_from(999, &[]).await,
        Err(DbError::NotFound("zone"))
    ));

    // Only active zones count towards the overall version.
    assert_eq!(db.dns_version().await.unwrap(), 0);
    db.activate_zone(zone.id, false).await.unwrap();
    assert_eq!(db.dns_version().await.unwrap(), 11);

    // Deleting the zone takes its data along.
    db.add_zone_tsig_key(zone.id, &tsig).await.unwrap();
    db.delete_zone(zone.id).await.unwrap();
    assert!(db.zone_rrsets(zone.id).await.unwrap().is_empty());
    assert_eq!(db.zone_dnssec(zone.id).await.unwrap(), None);
    assert!(db.zone_tsig_keys(zone.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn challenge_txt_writes_bump_the_base_zone() {
    let db = Db::open(":memory:").unwrap();
    let base = db.create_zone(None, "dns.example.net").await.unwrap();
    db.activate_zone(base.id, true).await.unwrap();
    assert_eq!(db.zone("dns.example.net").await.unwrap().version, 2);
    db.present_txt("abc.dns.example.net", "t1").await.unwrap();
    assert_eq!(db.zone("dns.example.net").await.unwrap().version, 3);
    db.update_acmedns_txt("abc.dns.example.net", "t2")
        .await
        .unwrap();
    assert_eq!(db.zone("dns.example.net").await.unwrap().version, 4);
    db.cleanup_txt("abc.dns.example.net", "t1").await.unwrap();
    assert_eq!(db.zone("dns.example.net").await.unwrap().version, 5);
    // Removing a domain drops its TXT values and bumps as well.
    let alice = db.create_user("alice", "h").await.unwrap();
    let d = db
        .add_user_domain(alice.id, "alice", "example.org")
        .await
        .unwrap();
    db.present_txt(&format!("{}.dns.example.net", d.subdomain), "x")
        .await
        .unwrap();
    let before = db.zone("dns.example.net").await.unwrap().version;
    db.delete_user(alice.id, "dns.example.net").await.unwrap();
    assert_eq!(
        db.zone("dns.example.net").await.unwrap().version,
        before + 1
    );
}

#[tokio::test]
async fn listing_and_user_deletion() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let bob = db.create_user("bob", "h").await.unwrap();
    let base = db.create_zone(None, "dns.example.net").await.unwrap();
    assert!(base.is_system() && base.owner.is_none());
    let b = db.create_zone(Some(alice.id), "b.test").await.unwrap();
    db.create_zone(Some(alice.id), "a.test").await.unwrap();
    db.create_zone(Some(bob.id), "c.test").await.unwrap();
    db.apply_zone_rrsets(b.id, &[rrset("b.test.", "A", &["192.0.2.1"])])
        .await
        .unwrap();

    let names =
        |zones: Vec<certo_core::Zone>| zones.into_iter().map(|z| z.name).collect::<Vec<_>>();
    assert_eq!(
        names(db.zones(Some(alice.id)).await.unwrap()),
        ["a.test", "b.test"]
    );
    assert_eq!(
        names(db.zones(None).await.unwrap()),
        ["a.test", "b.test", "c.test", "dns.example.net"]
    );

    let removed = db.delete_user(alice.id, "dns.example.net").await.unwrap();
    assert!(removed.is_empty());
    assert_eq!(
        names(db.zones(None).await.unwrap()),
        ["c.test", "dns.example.net"]
    );
    assert!(db.zone_rrsets(b.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn zones_of_other_owners_may_not_overlap() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let bob = db.create_user("bob", "h").await.unwrap();
    db.create_zone(Some(alice.id), "example.com").await.unwrap();
    // Alice may nest her own zones; Bob may not go above or below hers.
    db.create_zone(Some(alice.id), "sub.example.com")
        .await
        .unwrap();
    assert!(matches!(
        db.create_zone(Some(bob.id), "deep.sub.example.com").await,
        Err(DbError::Conflict("zone overlap"))
    ));
    assert!(matches!(
        db.create_zone(Some(bob.id), "com").await,
        Err(DbError::Conflict("zone overlap"))
    ));
    db.create_zone(Some(bob.id), "example.org").await.unwrap();
    // The base zone (no owner) never counts as an overlap: the zone service refuses names
    // under it, and an ancestor (the owner's apex above the challenge domain) is allowed.
    db.create_zone(None, "dns.example.net").await.unwrap();
    db.create_zone(Some(bob.id), "example.net").await.unwrap();
    db.create_zone(Some(bob.id), "x.dns.example.net")
        .await
        .unwrap();
}
