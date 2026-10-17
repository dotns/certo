//! Base-domain behavior, ported from the Go nameserver tests.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

mod common;

use std::net::SocketAddr;

use certo_db::Db;
use certo_dns::{
    AxfrAccess, ChallengeConfig, ChallengeResponder, DnsHandler, DynamicCatalog, ZoneConfig,
    build_zone,
};
use common::{name, query, rr, start, types, udp};
use hickory_server::proto::{
    op::{Edns, Message, ResponseCode},
    rr::{RData, RecordType},
};

fn config() -> ChallengeConfig {
    ChallengeConfig {
        domain: "auth.example.org".into(),
        nsname: "ns1.auth.example.org".into(),
        nsadmin: "admin.example.org".into(),
        records: vec![
            "auth.example.org. A 192.168.1.100".into(),
            "ns1.auth.example.org. A 192.168.1.101".into(),
            "cn.example.org CNAME something.example.org.".into(),
            "!''b', unparseable ".into(),
            "ns2.auth.example.org. A 192.168.1.102".into(),
            "Static.Example.NET. 60 IN TXT \"MixedCase\"".into(),
        ],
    }
}

struct Fixture {
    addr: SocketAddr,
    db: Db,
    challenge: ChallengeResponder,
    zones: DynamicCatalog,
}

async fn setup() -> Fixture {
    let db = Db::open(":memory:").unwrap();
    let challenge = ChallengeResponder::new(&config(), db.clone()).unwrap();
    let zones = DynamicCatalog::new();
    let addr = start(DnsHandler::new(challenge.clone(), zones.clone())).await;
    Fixture {
        addr,
        db,
        challenge,
        zones,
    }
}

async fn ask(f: &Fixture, qname: &str, rtype: RecordType) -> Message {
    udp(f.addr, &query(qname, rtype, false)).await
}

fn txt_values(msg: &Message) -> Vec<String> {
    msg.answers
        .iter()
        .filter_map(|r| match &r.data {
            RData::TXT(txt) => Some(txt.to_string()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn resolves_static_a_records_case_insensitively() {
    let f = setup().await;
    let resp = ask(&f, "aUtH.eXAmpLe.org", RecordType::A).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert!(resp.metadata.authoritative);
    assert_eq!(resp.answers.len(), 1);
    assert_eq!(resp.answers[0].ttl, 3600);

    // Records after an unparsable line are still loaded.
    let resp = ask(&f, "ns2.auth.example.org", RecordType::A).await;
    assert_eq!(types(&resp.answers), [RecordType::A]);
}

#[tokio::test]
async fn unknown_names_outside_any_zone_are_non_authoritative_nxdomain() {
    let f = setup().await;
    let resp = ask(&f, "nonexistent.domain.tld", RecordType::A).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);
    assert!(!resp.metadata.authoritative);
    assert!(resp.answers.is_empty());
    assert!(resp.authorities.is_empty());
}

#[tokio::test]
async fn unknown_names_under_base_domain_get_nxdomain_with_soa() {
    let f = setup().await;
    for qname in [
        "nonexistent.auth.example.org",
        "doesnotexist.aUtH.eXAmpLe.org",
    ] {
        let resp = ask(&f, qname, RecordType::A).await;
        assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);
        assert!(resp.metadata.authoritative);
        assert_eq!(types(&resp.authorities), [RecordType::SOA]);
    }
    let resp = ask(&f, "doesnotexist.auth.example.org", RecordType::SOA).await;
    assert_eq!(types(&resp.authorities), [RecordType::SOA]);
    let RData::SOA(soa) = &resp.authorities[0].data else {
        panic!("not a SOA");
    };
    assert_eq!(soa.mname, name("ns1.auth.example.org."));
    assert_eq!(soa.rname, name("admin.example.org."));
    assert_eq!(soa.refresh, 28800);
    assert_eq!(soa.minimum, 86400);
    assert_eq!(soa.serial.to_string().len(), 10);
}

#[tokio::test]
async fn static_cname_is_returned_for_any_type() {
    let f = setup().await;
    for rtype in [RecordType::CNAME, RecordType::A] {
        let resp = ask(&f, "cn.example.org", rtype).await;
        assert_eq!(resp.answers.len(), 1);
        assert_eq!(resp.answers[0].name, name("cn.example.org."));
        assert_eq!(resp.answers[0].ttl, 3600);
        assert_eq!(
            resp.answers[0].data,
            RData::try_from_str(RecordType::CNAME, "something.example.org.").unwrap()
        );
    }
}

#[tokio::test]
async fn static_records_are_lowercased() {
    let f = setup().await;
    let resp = ask(&f, "static.example.net", RecordType::TXT).await;
    assert_eq!(txt_values(&resp), ["mixedcase"]);
    assert_eq!(resp.answers[0].ttl, 60);
}

#[tokio::test]
async fn challenge_txt_comes_from_the_database() {
    let f = setup().await;
    let domain = "_acme-challenge.test.auth.example.org";
    f.db.present_txt(
        &format!("{domain}."),
        "LHDhK3oGRvkiefQnx7OOczTY5Tic_xZ6HcMOc_gmtoM",
    )
    .await
    .unwrap();
    let resp = ask(&f, domain, RecordType::TXT).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert!(resp.metadata.authoritative);
    assert_eq!(
        txt_values(&resp),
        ["LHDhK3oGRvkiefQnx7OOczTY5Tic_xZ6HcMOc_gmtoM"]
    );
    assert_eq!(resp.answers[0].ttl, 1);

    let resp = ask(&f, "nonexistent.auth.example.org", RecordType::TXT).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);

    // Same as the Go server: only TXT queries consult the database.
    let resp = ask(&f, domain, RecordType::A).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);

    // TXT values for names outside the base domain are served, non-authoritatively.
    f.db.present_txt("elsewhere.example.com", "v")
        .await
        .unwrap();
    let resp = ask(&f, "elsewhere.example.com", RecordType::TXT).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert!(!resp.metadata.authoritative);
    assert_eq!(txt_values(&resp), ["v"]);
}

#[tokio::test]
async fn own_challenge_serves_the_in_memory_token() {
    let f = setup().await;
    f.challenge.set_own_auth_key("own-token");
    let resp = ask(&f, "_acme-challenge.auth.example.org", RecordType::TXT).await;
    assert_eq!(txt_values(&resp), ["own-token"]);
    assert_eq!(resp.answers[0].ttl, 1);

    f.challenge.set_own_auth_key("");
    let resp = ask(&f, "_acme-challenge.auth.example.org", RecordType::TXT).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert_eq!(txt_values(&resp), [""]);
}

#[tokio::test]
async fn edns_is_echoed_and_unknown_versions_get_badvers() {
    let f = setup().await;
    let resp = ask(&f, "auth.example.org", RecordType::OPT).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);

    let resp = udp(f.addr, &query("auth.example.org", RecordType::A, true)).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert!(resp.edns.is_some(), "OPT expected in response");

    let mut msg = query("auth.example.org", RecordType::A, false);
    let mut edns = Edns::new();
    edns.set_version(1);
    msg.set_edns(edns);
    let resp = udp(f.addr, &msg).await;
    // BADVERS and BADSIG share extended rcode 16; compare the wire value.
    assert_eq!(
        u16::from(resp.metadata.response_code),
        u16::from(ResponseCode::BADVERS)
    );
    assert!(resp.answers.is_empty());
}

#[tokio::test]
async fn user_zones_are_routed_to_the_catalog() {
    let f = setup().await;
    let zone = "23.251.162.in-addr.arpa.";
    f.zones.upsert(
        build_zone(ZoneConfig {
            origin: name(zone),
            records: vec![
                rr(
                    zone,
                    RecordType::SOA,
                    "ns1.auth.example.org. admin.example.org. 1 7200 3600 1209600 300",
                ),
                rr(&format!("5.{zone}"), RecordType::PTR, "host.example.com."),
            ],
            dnssec: None,
            axfr: AxfrAccess::default(),
        })
        .unwrap(),
    );

    let resp = ask(&f, "5.23.251.162.in-addr.arpa", RecordType::PTR).await;
    assert_eq!(types(&resp.answers), [RecordType::PTR]);
    let resp = ask(&f, "9.23.251.162.in-addr.arpa", RecordType::PTR).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);
    let RData::SOA(soa) = &resp.authorities[0].data else {
        panic!("zone SOA expected");
    };
    assert_eq!(soa.serial, 1, "zone SOA, not the base-domain SOA");

    // Names outside every zone still go to the challenge responder.
    let resp = ask(&f, "auth.example.org", RecordType::A).await;
    assert_eq!(types(&resp.answers), [RecordType::A]);
}
