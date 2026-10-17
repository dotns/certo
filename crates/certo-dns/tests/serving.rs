//! End-to-end checks of zone serving over real UDP/TCP sockets.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

mod common;

use std::{net::SocketAddr, time::Duration};

use certo_dns::{AxfrAccess, DnssecConfig, DynamicCatalog, ZoneConfig, build_zone};
use common::{name, query, rr, start, tcp, types, udp};
use hickory_server::{
    dnssec::NxProofKind,
    proto::{
        dnssec::{
            Algorithm, DnssecSigner, Nsec3HashAlgorithm, SigningKey, crypto::EcdsaSigningKey,
            rdata::DNSKEY,
        },
        op::{Message, ResponseCode},
        rr::{LowerName, RData, Record, RecordType, TSigner, rdata::tsig::TsigAlgorithm},
    },
};

const ZONE: &str = "23.251.162.in-addr.arpa.";

fn reverse_records(ptr_target: &str) -> Vec<Record> {
    vec![
        rr(
            ZONE,
            RecordType::SOA,
            "ns1.example.com. admin.example.com. 1 7200 3600 1209600 300",
        ),
        rr(ZONE, RecordType::NS, "ns1.example.com."),
        rr(&format!("5.{ZONE}"), RecordType::PTR, ptr_target),
    ]
}

fn zone(records: Vec<Record>, dnssec: Option<DnssecConfig>, axfr: AxfrAccess) -> ZoneConfig {
    ZoneConfig {
        origin: name(ZONE),
        records,
        dnssec,
        axfr,
    }
}

fn signer(origin: &str) -> DnssecSigner {
    let pkcs8 = EcdsaSigningKey::generate_pkcs8(Algorithm::ECDSAP256SHA256).unwrap();
    let key = EcdsaSigningKey::from_pkcs8(&pkcs8, Algorithm::ECDSAP256SHA256).unwrap();
    let dnskey = DNSKEY::from_key(&key.to_public_key().unwrap());
    DnssecSigner::new(
        dnskey,
        Box::new(key),
        name(origin),
        Duration::from_secs(7 * 86400),
    )
}

fn tsig(key_name: &str) -> TSigner {
    TSigner::new(
        b"0123456789abcdef0123456789abcdef".to_vec(),
        TsigAlgorithm::HmacSha256,
        name(key_name),
        300,
    )
    .unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn reverse_zone_answers_ptr_and_negative_responses() {
    let catalog = DynamicCatalog::new();
    catalog.upsert(
        build_zone(zone(
            reverse_records("host.example.com."),
            None,
            AxfrAccess::default(),
        ))
        .unwrap(),
    );
    let addr = start(catalog).await;

    let resp = udp(addr, &query(&format!("5.{ZONE}"), RecordType::PTR, false)).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert!(resp.metadata.authoritative);
    assert_eq!(
        resp.answers[0].data,
        RData::try_from_str(RecordType::PTR, "host.example.com.").unwrap()
    );

    let resp = udp(addr, &query(&format!("6.{ZONE}"), RecordType::PTR, false)).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);
    assert_eq!(types(&resp.authorities), vec![RecordType::SOA]);

    let resp = udp(addr, &query(&format!("5.{ZONE}"), RecordType::A, false)).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::NoError);
    assert_eq!(resp.answers, Vec::<Record>::new());
    assert_eq!(types(&resp.authorities), vec![RecordType::SOA]);

    let resp = udp(addr, &query(ZONE, RecordType::NS, false)).await;
    assert_eq!(types(&resp.answers), vec![RecordType::NS]);

    let resp = udp(addr, &query("1.1.1.in-addr.arpa.", RecordType::PTR, false)).await;
    assert_eq!(resp.metadata.response_code, ResponseCode::Refused);
}

#[tokio::test]
async fn zones_can_be_replaced_and_removed_at_runtime() {
    let catalog = DynamicCatalog::new();
    let addr = start(catalog.clone()).await;
    let q = query(&format!("5.{ZONE}"), RecordType::PTR, false);

    assert_eq!(
        udp(addr, &q).await.metadata.response_code,
        ResponseCode::Refused
    );

    catalog.upsert(
        build_zone(zone(
            reverse_records("a.example.com."),
            None,
            AxfrAccess::default(),
        ))
        .unwrap(),
    );
    assert_eq!(
        udp(addr, &q).await.answers[0].data,
        RData::try_from_str(RecordType::PTR, "a.example.com.").unwrap()
    );

    catalog.upsert(
        build_zone(zone(
            reverse_records("b.example.com."),
            None,
            AxfrAccess::default(),
        ))
        .unwrap(),
    );
    assert_eq!(
        udp(addr, &q).await.answers[0].data,
        RData::try_from_str(RecordType::PTR, "b.example.com.").unwrap()
    );

    assert!(catalog.remove(&LowerName::new(&name(ZONE))));
    assert_eq!(
        udp(addr, &q).await.metadata.response_code,
        ResponseCode::Refused
    );
}

#[tokio::test]
async fn records_outside_the_zone_or_without_soa_are_rejected() {
    let mut records = reverse_records("host.example.com.");
    records.push(rr("evil.example.com.", RecordType::A, "192.0.2.1"));
    assert!(build_zone(zone(records, None, AxfrAccess::default())).is_err());

    let records = reverse_records("host.example.com.").split_off(1);
    assert!(build_zone(zone(records, None, AxfrAccess::default())).is_err());
}

#[tokio::test]
async fn signed_zone_returns_rrsig_dnskey_and_denial_proofs() {
    for nx_proof in [
        NxProofKind::Nsec,
        NxProofKind::Nsec3 {
            algorithm: Nsec3HashAlgorithm::default(),
            salt: std::sync::Arc::from([]),
            iterations: 0,
            opt_out: false,
        },
    ] {
        let denial = match nx_proof {
            NxProofKind::Nsec => RecordType::NSEC,
            NxProofKind::Nsec3 { .. } => RecordType::NSEC3,
        };
        let catalog = DynamicCatalog::new();
        let dnssec = DnssecConfig {
            signer: signer(ZONE),
            nx_proof,
        };
        catalog.upsert(
            build_zone(zone(
                reverse_records("host.example.com."),
                Some(dnssec),
                AxfrAccess::default(),
            ))
            .unwrap(),
        );
        let addr = start(catalog).await;

        let resp = udp(addr, &query(&format!("5.{ZONE}"), RecordType::PTR, true)).await;
        assert_eq!(
            types(&resp.answers),
            vec![RecordType::PTR, RecordType::RRSIG]
        );

        let resp = udp(addr, &query(ZONE, RecordType::DNSKEY, true)).await;
        assert!(types(&resp.answers).contains(&RecordType::DNSKEY));

        let resp = udp(addr, &query(&format!("6.{ZONE}"), RecordType::PTR, true)).await;
        assert_eq!(resp.metadata.response_code, ResponseCode::NXDomain);
        assert!(
            types(&resp.authorities).contains(&denial),
            "{denial} missing: {resp:?}"
        );

        let resp = udp(addr, &query(&format!("5.{ZONE}"), RecordType::PTR, false)).await;
        assert_eq!(types(&resp.answers), vec![RecordType::PTR]);
    }
}

fn axfr() -> Message {
    query(ZONE, RecordType::AXFR, false)
}

async fn serve_with(axfr: AxfrAccess) -> SocketAddr {
    let catalog = DynamicCatalog::new();
    catalog.upsert(build_zone(zone(reverse_records("host.example.com."), None, axfr)).unwrap());
    start(catalog).await
}

fn transferred(resps: &[Message]) -> Vec<RecordType> {
    resps.iter().flat_map(|m| types(&m.answers)).collect()
}

#[tokio::test]
async fn axfr_is_refused_by_default() {
    let addr = serve_with(AxfrAccess::default()).await;
    let resps = tcp(addr, &axfr()).await;
    assert_eq!(resps[0].metadata.response_code, ResponseCode::Refused);
}

#[tokio::test]
async fn axfr_is_allowed_from_listed_networks() {
    let addr = serve_with(AxfrAccess {
        tsig_keys: vec![],
        allow_from: vec!["127.0.0.0/8".parse().unwrap()],
    })
    .await;
    let resps = tcp(addr, &axfr()).await;
    let got = transferred(&resps);
    assert_eq!(got.first(), Some(&RecordType::SOA));
    assert_eq!(got.last(), Some(&RecordType::SOA));
    assert!(got.contains(&RecordType::PTR));

    let other = serve_with(AxfrAccess {
        tsig_keys: vec![],
        allow_from: vec!["10.0.0.0/8".parse().unwrap()],
    })
    .await;
    assert_eq!(
        tcp(other, &axfr()).await[0].metadata.response_code,
        ResponseCode::Refused
    );
}

#[tokio::test]
async fn axfr_requires_a_valid_tsig_key() {
    let addr = serve_with(AxfrAccess {
        tsig_keys: vec![tsig("xfr.example.com.")],
        allow_from: vec![],
    })
    .await;

    assert_eq!(
        tcp(addr, &axfr()).await[0].metadata.response_code,
        ResponseCode::Refused
    );

    let mut signed = axfr();
    signed.finalize(&tsig("xfr.example.com."), now()).unwrap();
    let resps = tcp(addr, &signed).await;
    assert_eq!(resps[0].metadata.response_code, ResponseCode::NoError);
    assert!(resps[0].signature.is_some(), "response must be TSIG-signed");
    assert!(transferred(&resps).contains(&RecordType::PTR));

    let mut wrong = axfr();
    wrong.finalize(&tsig("other.example.com."), now()).unwrap();
    assert_eq!(
        tcp(addr, &wrong).await[0].metadata.response_code,
        ResponseCode::NotAuth
    );

    let mut stale = axfr();
    stale
        .finalize(&tsig("xfr.example.com."), now() - 3600)
        .unwrap();
    assert_eq!(
        tcp(addr, &stale).await[0].metadata.response_code,
        ResponseCode::NotAuth
    );
}
