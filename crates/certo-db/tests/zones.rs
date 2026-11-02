//! Zone ownership and activation state.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

use certo_db::{Db, DbError};

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
