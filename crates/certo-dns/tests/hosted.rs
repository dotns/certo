//! Hosted zones end to end: database -> `Zones` -> catalog -> DNS answers.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

mod common;

use std::{net::SocketAddr, str::FromStr};

use certo_db::Db;
use certo_dns::{
    ChallengeConfig, ChallengeResponder, DnsHandler, DynamicCatalog, RecordError, RecordInput,
    ZoneOpError, ZonePolicy, Zones,
};
use common::{query, start, tcp, types, udp};
use hickory_server::proto::{
    dnssec::rdata::DNSSECRData,
    op::{Message, ResponseCode},
    rr::{Name, RData, RecordType, TSigner, rdata::tsig::TsigAlgorithm},
};

struct Fixture {
    addr: SocketAddr,
    db: Db,
    zones: Zones,
    alice: i64,
    bob: i64,
}

async fn setup() -> Fixture {
    let db = Db::open(":memory:").unwrap();
    let challenge = ChallengeResponder::new(
        &ChallengeConfig {
            domain: "auth.example.org".into(),
            nsname: "ns1.auth.example.org".into(),
            nsadmin: "admin.example.org".into(),
            records: vec![
                "auth.example.org. A 192.0.2.53".into(),
                "4.3.2.1.in-addr.arpa. PTR static.example.org.".into(),
            ],
        },
        db.clone(),
    )
    .unwrap();
    let catalog = DynamicCatalog::new();
    let policy = ZonePolicy::new(
        challenge.domain(),
        "ns1.auth.example.org",
        "admin.example.org",
        challenge.static_owners(),
    )
    .unwrap();
    let zones = Zones::new(db.clone(), catalog.clone(), policy);
    let addr = start(DnsHandler::new(challenge, catalog)).await;
    let alice = db.create_user("alice", "h").await.unwrap().id;
    let bob = db.create_user("bob", "h").await.unwrap().id;
    Fixture {
        addr,
        db,
        zones,
        alice,
        bob,
    }
}

const fn rec<'a>(name: &'a str, rtype: &'a str, data: &'a str) -> RecordInput<'a> {
    RecordInput {
        name,
        rtype,
        ttl: Some(300),
        data,
    }
}

async fn ask(f: &Fixture, qname: &str, rtype: RecordType) -> Message {
    udp(f.addr, &query(qname, rtype, false)).await
}

#[tokio::test]
async fn reverse_zone_ptr_round_trip() {
    let f = setup().await;
    let zone = f
        .zones
        .create(f.alice, "23.251.162.in-addr.arpa.")
        .await
        .unwrap();
    assert_eq!(zone.name, "23.251.162.in-addr.arpa");
    let ptr = f
        .zones
        .add_record(&zone, rec("5", "PTR", "Host.Example.com"))
        .await
        .unwrap();
    assert_eq!(
        (ptr.name.as_str(), ptr.data.as_str()),
        ("5.23.251.162.in-addr.arpa", "host.example.com.")
    );

    let r = ask(&f, "5.23.251.162.in-addr.arpa.", RecordType::PTR).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NoError);
    assert!(r.metadata.authoritative);
    assert_eq!(
        r.answers[0].data,
        RData::try_from_str(RecordType::PTR, "host.example.com.").unwrap()
    );

    // Apex SOA and the default NS pointing at this server.
    let r = ask(&f, "23.251.162.in-addr.arpa.", RecordType::NS).await;
    assert_eq!(
        r.answers[0].data,
        RData::try_from_str(RecordType::NS, "ns1.auth.example.org.").unwrap()
    );
    let r = ask(&f, "23.251.162.in-addr.arpa.", RecordType::SOA).await;
    let RData::SOA(soa) = &r.answers[0].data else {
        panic!("no SOA: {r:?}");
    };
    assert_eq!(soa.serial, f.db.zone(&zone.name).await.unwrap().serial);

    let r = ask(&f, "6.23.251.162.in-addr.arpa.", RecordType::PTR).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NXDomain);
    assert_eq!(types(&r.authorities), vec![RecordType::SOA]);

    // Updates and deletes are served immediately.
    f.zones
        .update_record(&zone, ptr.id, rec("5", "PTR", "other.example.com."))
        .await
        .unwrap();
    let r = ask(&f, "5.23.251.162.in-addr.arpa.", RecordType::PTR).await;
    assert_eq!(
        r.answers[0].data,
        RData::try_from_str(RecordType::PTR, "other.example.com.").unwrap()
    );
    f.zones.delete_record(&zone, ptr.id).await.unwrap();
    let r = ask(&f, "5.23.251.162.in-addr.arpa.", RecordType::PTR).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NXDomain);

    // Gone zones fall back to the base-domain responder, which knows nothing about them.
    f.zones.delete(&zone).await.unwrap();
    let r = ask(&f, "23.251.162.in-addr.arpa.", RecordType::SOA).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NXDomain);
    assert!(!r.metadata.authoritative);
}

#[tokio::test]
async fn forward_zone_cname_and_nodata() {
    let f = setup().await;
    let zone = f.zones.create(f.alice, "Example.COM").await.unwrap();
    f.zones
        .add_record(&zone, rec("www", "A", "192.0.2.10"))
        .await
        .unwrap();
    f.zones
        .add_record(&zone, rec("alias", "CNAME", "www.example.com"))
        .await
        .unwrap();

    let r = ask(&f, "alias.example.com.", RecordType::A).await;
    assert_eq!(types(&r.answers), vec![RecordType::CNAME, RecordType::A]);

    let r = ask(&f, "www.example.com.", RecordType::AAAA).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NoError);
    assert_eq!(r.answers.len(), 0);
    assert_eq!(types(&r.authorities), vec![RecordType::SOA]);

    let err = f
        .zones
        .add_record(&zone, rec("www", "CNAME", "x.example.net"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ZoneOpError::Record(RecordError::CnameConflict)
    ));
    let err = f
        .zones
        .update_record(&zone, 999, rec("www", "A", "192.0.2.1"))
        .await
        .unwrap_err();
    assert!(matches!(err, ZoneOpError::NotFound("record")));
}

#[tokio::test]
async fn zone_names_that_collide_are_rejected() {
    let f = setup().await;
    let not_allowed = |r: Result<_, ZoneOpError>| matches!(r, Err(ZoneOpError::NotAllowed(_)));
    // The server's own domain and its subdomains.
    assert!(not_allowed(
        f.zones.create(f.alice, "auth.example.org").await
    ));
    assert!(not_allowed(
        f.zones.create(f.alice, "x.auth.example.org").await
    ));
    // Zones that would shadow static records.
    assert!(not_allowed(
        f.zones.create(f.alice, "2.1.in-addr.arpa").await
    ));
    // Single labels are not zones users can host.
    assert!(matches!(
        f.zones.create(f.alice, "com").await,
        Err(ZoneOpError::Record(RecordError::InvalidName(_)))
    ));

    f.zones.create(f.alice, "example.net").await.unwrap();
    assert!(matches!(
        f.zones.create(f.bob, "example.net.").await,
        Err(ZoneOpError::Exists)
    ));
    assert!(not_allowed(f.zones.create(f.bob, "sub.example.net").await));
    // Nor may another user take a parent of someone's zone.
    f.zones.create(f.alice, "a.deep.test").await.unwrap();
    assert!(not_allowed(f.zones.create(f.bob, "deep.test").await));
    // The owner may nest zones.
    f.zones.create(f.alice, "sub.example.net").await.unwrap();
}

#[tokio::test]
async fn deleting_a_user_stops_serving_their_zones() {
    let f = setup().await;
    f.zones.create(f.bob, "bob.test").await.unwrap();
    assert_eq!(
        ask(&f, "bob.test.", RecordType::SOA)
            .await
            .metadata
            .response_code,
        ResponseCode::NoError
    );
    f.db.delete_user(f.bob, "auth.example.org").await.unwrap();
    f.zones.publish("bob.test").await.unwrap();
    let r = ask(&f, "bob.test.", RecordType::SOA).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NXDomain);
    assert!(!r.metadata.authoritative);
    assert!(f.db.user_by_id(f.bob).await.is_err());
}

#[tokio::test]
async fn stored_zones_are_served_after_restart() {
    let f = setup().await;
    let zone = f.zones.create(f.alice, "restart.test").await.unwrap();
    f.zones
        .add_record(&zone, rec("@", "TXT", "\"hello world\""))
        .await
        .unwrap();

    // A fresh catalog and service over the same database.
    let catalog = DynamicCatalog::new();
    let zones = Zones::new(f.db.clone(), catalog.clone(), f.zones.policy().clone());
    assert_eq!(zones.load_all().await.unwrap(), 1);
    let challenge = ChallengeResponder::new(
        &ChallengeConfig {
            domain: "auth.example.org".into(),
            nsname: "ns1.auth.example.org".into(),
            nsadmin: "admin.example.org".into(),
            records: vec![],
        },
        f.db.clone(),
    )
    .unwrap();
    let addr = start(DnsHandler::new(challenge, catalog)).await;
    let r = udp(addr, &query("restart.test.", RecordType::TXT, false)).await;
    assert_eq!(
        r.answers[0].data,
        RData::try_from_str(RecordType::TXT, "\"hello world\"").unwrap()
    );
}

fn axfr(zone: &str) -> Message {
    query(zone, RecordType::AXFR, false)
}

fn transferred(resps: &[Message]) -> Vec<RecordType> {
    resps.iter().flat_map(|m| types(&m.answers)).collect()
}

#[tokio::test]
async fn dnssec_signs_publishes_ds_and_can_be_disabled() {
    let f = setup().await;
    let zone = f.zones.create(f.alice, "signed.test").await.unwrap();
    f.zones
        .add_record(&zone, rec("www", "A", "192.0.2.1"))
        .await
        .unwrap();
    let off = f.zones.dnssec(&zone).await.unwrap();
    assert_eq!((off.enabled, off.ds.as_deref()), (false, None));

    let on = f.zones.set_dnssec(&zone, true, "nsec").await.unwrap();
    assert!(on.enabled);
    let ds = on.ds.unwrap();
    let r = udp(f.addr, &query("www.signed.test.", RecordType::A, true)).await;
    assert_eq!(types(&r.answers), vec![RecordType::A, RecordType::RRSIG]);
    let r = udp(f.addr, &query("signed.test.", RecordType::DNSKEY, true)).await;
    let served = r
        .answers
        .iter()
        .find_map(|rr| match &rr.data {
            RData::DNSSEC(DNSSECRData::DNSKEY(k)) => Some(k.clone()),
            _ => None,
        })
        .unwrap();
    let tag = served.calculate_key_tag().unwrap();
    assert!(ds.starts_with(&format!("{tag} 13 2 ")), "{ds} vs tag {tag}");
    assert_eq!(on.dnskey.unwrap(), served.to_string());

    // Re-enabling keeps the key (and so the DS at the parent).
    let again = f.zones.set_dnssec(&zone, true, "nsec3").await.unwrap();
    assert_eq!(again.ds.as_deref(), Some(ds.as_str()));
    let r = udp(f.addr, &query("nope.signed.test.", RecordType::A, true)).await;
    assert_eq!(r.metadata.response_code, ResponseCode::NXDomain);
    assert!(types(&r.authorities).contains(&RecordType::NSEC3));

    // Re-signing bumps the serial and keeps the zone signed.
    let before = f.db.zone("signed.test").await.unwrap().serial;
    assert_eq!(f.zones.resign_all().await.unwrap(), 1);
    assert!(f.db.zone("signed.test").await.unwrap().serial > before);
    let r = udp(f.addr, &query("www.signed.test.", RecordType::A, true)).await;
    assert!(types(&r.answers).contains(&RecordType::RRSIG));

    let off = f.zones.set_dnssec(&zone, false, "nsec").await.unwrap();
    assert_eq!(off.dnskey, None);
    let r = udp(f.addr, &query("www.signed.test.", RecordType::A, true)).await;
    assert_eq!(types(&r.answers), vec![RecordType::A]);
    // A later enable generates a fresh key.
    let fresh = f.zones.set_dnssec(&zone, true, "nsec").await.unwrap();
    assert_ne!(fresh.ds.as_deref(), Some(ds.as_str()));

    assert!(matches!(
        f.zones.set_dnssec(&zone, true, "nsec5").await,
        Err(ZoneOpError::Invalid {
            code: "invalid_nsec_mode",
            ..
        })
    ));
}

#[tokio::test]
async fn axfr_access_follows_allow_list_and_tsig_keys() {
    let f = setup().await;
    let zone = f.zones.create(f.alice, "xfr.test").await.unwrap();
    f.zones
        .add_record(&zone, rec("www", "A", "192.0.2.1"))
        .await
        .unwrap();
    assert_eq!(
        tcp(f.addr, &axfr("xfr.test.")).await[0]
            .metadata
            .response_code,
        ResponseCode::Refused
    );

    let info = f
        .zones
        .set_transfer_allow_from(&zone, &["127.0.0.1/8".into()])
        .await
        .unwrap();
    assert_eq!(info.allow_from, ["127.0.0.0/8"]);
    let got = transferred(&tcp(f.addr, &axfr("xfr.test.")).await);
    assert_eq!(got.first(), Some(&RecordType::SOA));
    assert!(got.contains(&RecordType::A));
    assert!(matches!(
        f.zones
            .set_transfer_allow_from(&zone, &["bogus".into()])
            .await,
        Err(ZoneOpError::Invalid {
            code: "invalid_cidr",
            ..
        })
    ));
    f.zones.set_transfer_allow_from(&zone, &[]).await.unwrap();

    let (key, secret) = f
        .zones
        .add_tsig_key(&zone, "Secondary.XFR.test.", "HMAC-SHA256")
        .await
        .unwrap();
    assert_eq!(
        (key.name.as_str(), secret.len()),
        ("secondary.xfr.test", 32)
    );
    assert!(matches!(
        f.zones
            .add_tsig_key(&zone, "secondary.xfr.test", "hmac-sha256")
            .await,
        Err(ZoneOpError::Invalid {
            code: "tsig_key_exists",
            ..
        })
    ));
    assert!(matches!(
        f.zones.add_tsig_key(&zone, "k2", "hmac-md5").await,
        Err(ZoneOpError::Invalid {
            code: "invalid_algorithm",
            ..
        })
    ));
    let tsig = TSigner::new(
        secret,
        TsigAlgorithm::HmacSha256,
        Name::from_str("secondary.xfr.test.").unwrap(),
        300,
    )
    .unwrap();
    let mut signed = axfr("xfr.test.");
    signed.finalize(&tsig, now()).unwrap();
    let resps = tcp(f.addr, &signed).await;
    assert_eq!(resps[0].metadata.response_code, ResponseCode::NoError);
    assert!(transferred(&resps).contains(&RecordType::A));
    let listed = f.zones.transfer(&zone).await.unwrap();
    assert_eq!(listed.tsig_keys[0].algorithm, "hmac-sha256");

    f.zones
        .delete_tsig_key(&zone, "secondary.xfr.test")
        .await
        .unwrap();
    let mut signed = axfr("xfr.test.");
    signed.finalize(&tsig, now()).unwrap();
    assert_ne!(
        tcp(f.addr, &signed).await[0].metadata.response_code,
        ResponseCode::NoError
    );
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
