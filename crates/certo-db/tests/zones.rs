//! Zone ownership, activation state and the version 3 import path.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

use certo_db::{DB_VERSION, Db, DbError};
use rusqlite::Connection;

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

    let active = db.activate_zone(zone.id, false).await.unwrap();
    assert!(active.active && !active.exempt);
    assert_eq!(active.failed_checks, 0);
    assert!(active.last_error.is_none() && active.verified_at.is_some());

    let off = db
        .deactivate_zone(zone.id, r#"[{"name":"example.com."}]"#)
        .await
        .unwrap();
    assert!(!off.active);
    assert_eq!(
        db.zone_snapshot(zone.id).await.unwrap().as_deref(),
        Some(r#"[{"name":"example.com."}]"#)
    );
    let exempt = db.activate_zone(zone.id, true).await.unwrap();
    assert!(exempt.active && exempt.exempt);
    assert_eq!(db.zone_snapshot(zone.id).await.unwrap(), None);
    let normal = db.clear_zone_exemption(zone.id).await.unwrap();
    assert!(normal.active && !normal.exempt);

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
}

#[tokio::test]
async fn listing_and_user_deletion() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let bob = db.create_user("bob", "h").await.unwrap();
    let base = db.create_zone(None, "dns.example.net").await.unwrap();
    assert!(base.is_system() && base.owner.is_none());
    db.create_zone(Some(alice.id), "b.test").await.unwrap();
    db.create_zone(Some(alice.id), "a.test").await.unwrap();
    db.create_zone(Some(bob.id), "c.test").await.unwrap();

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

const V3: &str = "
CREATE TABLE acmedns(Name TEXT, Value TEXT);
INSERT INTO acmedns (Name, Value) VALUES ('db_version', '3');
CREATE TABLE txt(Domain TEXT NOT NULL, Value TEXT NOT NULL DEFAULT '', LastUpdate INT);
CREATE TABLE users(id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT UNIQUE NOT NULL, password_hash TEXT NOT NULL, created_at INT NOT NULL);
CREATE TABLE user_domains(user_id INTEGER NOT NULL, domain TEXT NOT NULL, subdomain TEXT UNIQUE NOT NULL, allowfrom TEXT NOT NULL DEFAULT '[]', UNIQUE(user_id, domain));
CREATE TABLE api_keys(id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER NOT NULL, name TEXT NOT NULL, key_value TEXT UNIQUE NOT NULL, scope TEXT NOT NULL DEFAULT '[\"*\"]', created_at INT NOT NULL);
CREATE TABLE zones(id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER NOT NULL, name TEXT UNIQUE NOT NULL, serial INTEGER NOT NULL, dnssec_enabled INTEGER NOT NULL DEFAULT 0, nsec_mode TEXT NOT NULL DEFAULT 'nsec', axfr_allow_from TEXT NOT NULL DEFAULT '[]', created_at INT NOT NULL);
CREATE TABLE records(id INTEGER PRIMARY KEY AUTOINCREMENT, zone_id INTEGER NOT NULL, name TEXT NOT NULL, rtype TEXT NOT NULL, ttl INTEGER NOT NULL, rdata TEXT NOT NULL, created_at INT NOT NULL);
CREATE TABLE zone_keys(id INTEGER PRIMARY KEY AUTOINCREMENT, zone_id INTEGER NOT NULL, algorithm TEXT NOT NULL, private_key BLOB NOT NULL, created_at INT NOT NULL);
CREATE TABLE tsig_keys(id INTEGER PRIMARY KEY AUTOINCREMENT, zone_id INTEGER NOT NULL, name TEXT NOT NULL, algorithm TEXT NOT NULL, secret BLOB NOT NULL, created_at INT NOT NULL, UNIQUE(zone_id, name));
INSERT INTO users (username, password_hash, created_at) VALUES ('alice', 'h', 1);
";

const V3_ZONES: &str = "
INSERT INTO zones (id, user_id, name, serial, dnssec_enabled, nsec_mode, axfr_allow_from, created_at)
    VALUES (7, 1, 'example.com', 5, 1, 'nsec3', '[\"192.0.2.0/24\"]', 1000);
INSERT INTO records (zone_id, name, rtype, ttl, rdata, created_at) VALUES
    (7, 'example.com', 'NS', 3600, 'ns1.example.net.', 1),
    (7, 'www.example.com', 'A', 300, '192.0.2.1', 2);
INSERT INTO zone_keys (zone_id, algorithm, private_key, created_at) VALUES (7, 'ECDSAP256SHA256', x'00', 1);
INSERT INTO tsig_keys (zone_id, name, algorithm, secret, created_at) VALUES (7, 'xfer', 'hmac-sha256', x'0102', 1);
";

fn version(path: &std::path::Path) -> String {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT Value FROM acmedns WHERE Name='db_version'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn v3_without_zones_upgrades_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.db");
    Connection::open(&path).unwrap().execute_batch(V3).unwrap();
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(version(&path), DB_VERSION.to_string());
    assert_eq!(db.schema_version().await.unwrap(), DB_VERSION);
    assert!(db.legacy_zones().await.unwrap().is_empty());
    let z = db.create_zone(Some(1), "fresh.test").await.unwrap();
    assert!(!z.active);
    let tables: Vec<String> = Connection::open(&path)
        .unwrap()
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !tables
            .iter()
            .any(|t| ["records", "zone_keys", "tsig_keys"].contains(&t.as_str()))
    );
}

#[tokio::test]
async fn v3_zones_are_exposed_for_import_then_upgraded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(&format!("{V3}{V3_ZONES}"))
        .unwrap();
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(version(&path), "3");
    assert_eq!(db.schema_version().await.unwrap(), 3);

    let legacy = db.legacy_zones().await.unwrap();
    assert_eq!(legacy.len(), 1);
    let z = &legacy[0];
    assert_eq!(
        (z.user_id, z.name.as_str(), z.dnssec, z.nsec3),
        (1, "example.com", true, true)
    );
    assert_eq!(z.axfr_allow_from, ["192.0.2.0/24"]);
    assert_eq!(
        z.records,
        [
            (
                "example.com".to_owned(),
                "NS".to_owned(),
                3600,
                "ns1.example.net.".to_owned()
            ),
            (
                "www.example.com".to_owned(),
                "A".to_owned(),
                300,
                "192.0.2.1".to_owned()
            ),
        ]
    );
    assert_eq!(
        z.tsig_keys,
        [("xfer".to_owned(), "hmac-sha256".to_owned(), vec![1u8, 2])]
    );

    // Reopening before the import keeps everything in place.
    drop(db);
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(db.legacy_zones().await.unwrap().len(), 1);

    db.complete_import().await.unwrap();
    assert_eq!(version(&path), DB_VERSION.to_string());
    assert!(db.legacy_zones().await.unwrap().is_empty());
    let zone = db.zone("example.com").await.unwrap();
    assert_eq!(
        (zone.id, zone.user_id, zone.owner.as_deref()),
        (7, Some(1), Some("alice"))
    );
    assert!(zone.active && zone.exempt && zone.verified_at.is_some());
    assert_eq!(zone.created_at, 1000);
    // Idempotent.
    db.complete_import().await.unwrap();
    assert_eq!(db.schema_version().await.unwrap(), DB_VERSION);
}
