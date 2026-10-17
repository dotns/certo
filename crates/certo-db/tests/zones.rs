//! Zone and record storage.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

use certo_db::{Db, DbError, NewRecord};

fn rec(name: &str, rtype: &str, data: &str) -> NewRecord {
    NewRecord {
        name: name.into(),
        rtype: rtype.into(),
        ttl: 300,
        data: data.into(),
    }
}

#[tokio::test]
async fn zone_lifecycle_bumps_serial_on_every_change() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let ns = rec("example.com", "NS", "ns1.example.net.");
    let zone = db
        .create_zone(alice.id, " Example.COM ", vec![ns.clone()])
        .await
        .unwrap();
    assert_eq!(zone.name, "example.com");
    assert_eq!(zone.owner.as_deref(), Some("alice"));
    assert!(!zone.dnssec);
    assert_eq!(zone.nsec_mode, "nsec");
    assert!(zone.axfr_allow_from.is_empty());
    assert!(matches!(
        db.create_zone(alice.id, "example.com", vec![]).await,
        Err(DbError::Conflict("zone"))
    ));

    let serial = |db: Db| async move { db.zone("example.com").await.unwrap().serial };
    let s0 = serial(db.clone()).await;
    let a = db
        .add_record(zone.id, rec("www.example.com", "A", "192.0.2.1"))
        .await
        .unwrap();
    assert_eq!(a.data, "192.0.2.1");
    let s1 = serial(db.clone()).await;
    assert!(s1 > s0);

    let updated = db
        .update_record(zone.id, a.id, rec("www.example.com", "A", "192.0.2.2"))
        .await
        .unwrap();
    assert_eq!((updated.id, updated.data.as_str()), (a.id, "192.0.2.2"));
    let s2 = serial(db.clone()).await;
    assert!(s2 > s1);

    let records = db.zone_records(zone.id).await.unwrap();
    assert_eq!(
        records
            .iter()
            .map(|r| (r.name.as_str(), r.rtype.as_str()))
            .collect::<Vec<_>>(),
        [("example.com", "NS"), ("www.example.com", "A")]
    );

    db.delete_record(zone.id, a.id).await.unwrap();
    assert!(serial(db.clone()).await > s2);
    assert!(matches!(
        db.delete_record(zone.id, a.id).await,
        Err(DbError::NotFound("record"))
    ));
    assert!(matches!(
        db.update_record(zone.id, a.id, ns).await,
        Err(DbError::NotFound("record"))
    ));

    db.delete_zone(zone.id).await.unwrap();
    assert!(matches!(
        db.zone("example.com").await,
        Err(DbError::NotFound("zone"))
    ));
    assert!(db.zone_records(zone.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn records_are_scoped_to_their_zone() {
    let db = Db::open(":memory:").unwrap();
    let u = db.create_user("u", "h").await.unwrap();
    let a = db.create_zone(u.id, "a.test", vec![]).await.unwrap();
    let b = db.create_zone(u.id, "b.test", vec![]).await.unwrap();
    let r = db
        .add_record(a.id, rec("a.test", "TXT", "\"x\""))
        .await
        .unwrap();
    assert!(matches!(
        db.delete_record(b.id, r.id).await,
        Err(DbError::NotFound("record"))
    ));
    assert_eq!(db.zone_records(a.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn listing_and_user_deletion() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap();
    let bob = db.create_user("bob", "h").await.unwrap();
    db.create_zone(bob.id, "z.test", vec![rec("z.test", "A", "192.0.2.1")])
        .await
        .unwrap();
    db.create_zone(alice.id, "b.test", vec![]).await.unwrap();
    db.create_zone(alice.id, "a.test", vec![]).await.unwrap();

    let names =
        |zones: Vec<certo_core::Zone>| zones.into_iter().map(|z| z.name).collect::<Vec<_>>();
    assert_eq!(
        names(db.zones(Some(alice.id)).await.unwrap()),
        ["a.test", "b.test"]
    );
    assert_eq!(
        names(db.zones(None).await.unwrap()),
        ["a.test", "b.test", "z.test"]
    );

    let z = db.zone("z.test").await.unwrap();
    db.delete_user(bob.id, "auth.example.org").await.unwrap();
    assert_eq!(names(db.zones(None).await.unwrap()), ["a.test", "b.test"]);
    assert!(db.zone_records(z.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn zones_of_other_users_may_not_overlap() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap().id;
    let bob = db.create_user("bob", "h").await.unwrap().id;
    db.create_zone(alice, "example.net", vec![]).await.unwrap();
    assert!(matches!(
        db.create_zone(bob, "example.net", vec![]).await,
        Err(DbError::Conflict("zone"))
    ));
    assert!(matches!(
        db.create_zone(bob, "sub.example.net", vec![]).await,
        Err(DbError::Conflict("zone overlap"))
    ));
    db.create_zone(alice, "a.deep.test", vec![]).await.unwrap();
    assert!(matches!(
        db.create_zone(bob, "deep.test", vec![]).await,
        Err(DbError::Conflict("zone overlap"))
    ));
    // Owners may nest their own zones; unrelated names are fine.
    db.create_zone(alice, "sub.example.net", vec![])
        .await
        .unwrap();
    db.create_zone(bob, "notexample.net", vec![]).await.unwrap();
}

#[tokio::test]
async fn cname_is_exclusive_at_its_name() {
    let db = Db::open(":memory:").unwrap();
    let alice = db.create_user("alice", "h").await.unwrap().id;
    let zone = db
        .create_zone(
            alice,
            "example.com",
            vec![rec("www.example.com", "A", "192.0.2.1")],
        )
        .await
        .unwrap();
    let conflict = |r: Result<_, DbError>| matches!(r, Err(DbError::Conflict("cname")));
    assert!(conflict(
        db.add_record(zone.id, rec("www.example.com", "CNAME", "x.example.net."))
            .await
    ));
    let aaaa = db
        .add_record(zone.id, rec("www.example.com", "AAAA", "2001:db8::1"))
        .await
        .unwrap();
    let cname = db
        .add_record(zone.id, rec("alias.example.com", "CNAME", "x.example.net."))
        .await
        .unwrap();
    assert!(conflict(
        db.add_record(zone.id, rec("alias.example.com", "TXT", "\"x\""))
            .await
    ));
    assert!(conflict(
        db.add_record(zone.id, rec("alias.example.com", "CNAME", "y.example.net."))
            .await
    ));
    // Replacing a record keeps it out of the check against itself...
    db.update_record(
        zone.id,
        cname.id,
        rec("alias.example.com", "CNAME", "z.example.net."),
    )
    .await
    .unwrap();
    // ...but a CNAME may not join the A record at www.
    assert!(conflict(
        db.update_record(
            zone.id,
            aaaa.id,
            rec("www.example.com", "CNAME", "x.example.net.")
        )
        .await
    ));
}
