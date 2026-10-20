//! Supervisor and client behaviour against a fake `pdns_server` and a mock API.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use certo_pdns::{Client, NewZone, PdnsConfig, RecordData, RrSet, Supervisor};
use tokio::sync::Mutex;

#[derive(Default)]
struct Seen {
    bodies: Vec<serde_json::Value>,
}

type Shared = Arc<Mutex<Seen>>;

async fn mock_api() -> (u16, Shared) {
    let seen: Shared = Arc::default();
    let app = Router::new()
        .route(
            "/api/v1/servers/localhost",
            get(|| async { Json(serde_json::json!({ "version": "5.0.7", "daemon_type": "authoritative" })) }),
        )
        .route(
            "/api/v1/servers/localhost/zones",
            post(|State(s): State<Shared>, Json(body): Json<serde_json::Value>| async move {
                s.lock().await.bodies.push(body.clone());
                (
                    StatusCode::CREATED,
                    Json(serde_json::json!({ "name": body["name"], "kind": "Native", "serial": 1, "rrsets": [] })),
                )
            })
            .get(|| async { Json(serde_json::json!([{ "name": "a.test.", "kind": "Native", "dnssec": true, "serial": 7 }])) }),
        )
        .route(
            "/api/v1/servers/localhost/zones/{id}",
            patch(|State(s): State<Shared>, Path(id): Path<String>, Json(body): Json<serde_json::Value>| async move {
                if id == "bad.test." {
                    return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error": "RRset bad.test. IN TXT: nope" })));
                }
                s.lock().await.bodies.push(body);
                (StatusCode::NO_CONTENT, Json(serde_json::Value::Null))
            })
            .get(|Path(id): Path<String>| async move {
                if id == "missing.test." {
                    return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Not Found" })));
                }
                (StatusCode::OK, Json(serde_json::json!({ "name": id, "kind": "Native", "rrsets": [
                    { "name": id, "type": "SOA", "ttl": 3600, "records": [{ "content": "ns. h. 1 1 1 1 1", "disabled": false }] }
                ] })))
            }),
        )
        .route(
            "/api/v1/servers/localhost/zones/{id}/metadata/{kind}",
            get(|Path((_, kind)): Path<(String, String)>| async move {
                if kind == "UNSET" {
                    return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Not Found" })));
                }
                (StatusCode::OK, Json(serde_json::json!({ "kind": kind, "metadata": ["1.2.3.4/32"] })))
            }),
        )
        .with_state(Arc::clone(&seen));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (port, seen)
}

#[tokio::test]
async fn client_maps_bodies_and_errors() {
    let (port, seen) = mock_api().await;
    let client = Client::new(&format!("http://127.0.0.1:{port}"), "k").unwrap();
    assert!(client.ping().await);
    assert_eq!(client.version().await.unwrap(), "5.0.7");

    let zones = client.zones().await.unwrap();
    assert_eq!(zones[0].name, "a.test.");
    assert!(zones[0].dnssec);
    assert_eq!(zones[0].serial, 7);

    client
        .create_zone(&NewZone::native(
            "New.Test.",
            vec!["ns.test.".into()],
            vec![],
        ))
        .await
        .unwrap();
    client
        .patch_rrsets(
            "new.test",
            &[RrSet::replace(
                "x.new.test.",
                "TXT",
                1,
                vec![RecordData::new("\"v\"")],
            )],
        )
        .await
        .unwrap();
    let bodies = seen.lock().await.bodies.clone();
    assert_eq!(bodies[0]["kind"], "Native");
    assert_eq!(bodies[0]["soa_edit_api"], "DEFAULT");
    assert_eq!(bodies[0]["nameservers"], serde_json::json!(["ns.test."]));
    assert_eq!(bodies[1]["rrsets"][0]["changetype"], "REPLACE");
    assert_eq!(bodies[1]["rrsets"][0]["ttl"], 1);

    let err = client
        .patch_rrsets("bad.test", &[RrSet::delete("bad.test.", "TXT")])
        .await
        .unwrap_err();
    assert!(
        matches!(err, certo_pdns::PdnsError::Api { status: 422, ref message } if message.contains("nope")),
        "{err}"
    );
    assert!(client.zone("missing.test").await.unwrap().is_none());
    let zone = client.zone("there.test").await.unwrap().unwrap();
    assert_eq!(zone.rrsets[0].rtype, "SOA");
    assert_eq!(
        client.metadata("z.test", "UNSET").await.unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        client.metadata("z.test", "ALLOW-AXFR-FROM").await.unwrap(),
        ["1.2.3.4/32"]
    );
}

/// Writes an executable shell script acting as `pdns_server`.
fn fake_binary(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join("pdns_server");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn config(dir: &std::path::Path, binary: std::path::PathBuf, api_port: u16) -> PdnsConfig {
    PdnsConfig {
        binary,
        dir: dir.join("pdns"),
        listen: "127.0.0.1:5399".into(),
        api_port,
        nsname: "ns.test".into(),
        nsadmin: "hostmaster.test".into(),
        loglevel: "info".into(),
        restart_backoff: Duration::from_millis(20),
    }
}

#[tokio::test]
async fn starts_prepares_the_directory_and_stops() {
    let (port, _) = mock_api().await;
    let tmp = tempfile::tempdir().unwrap();
    let bin = fake_binary(tmp.path(), "echo started; exec sleep 60");
    let cfg = config(tmp.path(), bin, port);
    let sup = Supervisor::start(cfg.clone()).await.unwrap();

    assert!(cfg.database().is_file(), "schema initialized");
    let conf = std::fs::read_to_string(cfg.conf_path()).unwrap();
    assert!(conf.contains(&format!("webserver-port={port}")));
    assert!(conf.contains("gsqlite3-dnssec=yes"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(cfg.conf_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&cfg.dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let db = rusqlite::Connection::open(cfg.database()).unwrap();
    let n: i64 = db
        .query_row("SELECT count(*) FROM domains", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);

    assert!(sup.client().ping().await);
    tokio::time::timeout(Duration::from_secs(5), sup.shutdown())
        .await
        .unwrap();
}

#[tokio::test]
async fn restarts_after_a_crash() {
    let (port, _) = mock_api().await;
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("crashed");
    let bin = fake_binary(
        tmp.path(),
        &format!(
            "if [ ! -e {m} ]; then touch {m}; sleep 0.2; exit 3; fi; exec sleep 60",
            m = marker.display()
        ),
    );
    let sup = Supervisor::start(config(tmp.path(), bin, port))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(marker.exists());
    // Still alive after the restart: failed() must not have resolved.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), sup.failed())
            .await
            .is_err()
    );
    sup.shutdown().await;
}

#[tokio::test]
async fn gives_up_after_repeated_crashes() {
    let (port, _) = mock_api().await;
    let tmp = tempfile::tempdir().unwrap();
    let bin = fake_binary(tmp.path(), "sleep 0.1; exit 1");
    let sup = Supervisor::start(config(tmp.path(), bin, port))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), sup.failed())
        .await
        .unwrap();
    sup.shutdown().await;
}

#[tokio::test]
async fn refuses_a_missing_binary_and_a_silent_one() {
    let (port, _) = mock_api().await;
    let tmp = tempfile::tempdir().unwrap();
    let err = Supervisor::start(config(tmp.path(), tmp.path().join("nope"), port))
        .await
        .unwrap_err();
    assert!(matches!(err, certo_pdns::PdnsError::Config(_)), "{err}");
    // No API behind this port (connections are closed unanswered): the early exit must
    // be reported, not waited out.
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unused = dead.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = dead.accept().await {
            drop(socket);
        }
    });
    let bin = fake_binary(tmp.path(), "exit 2");
    let err = Supervisor::start(config(tmp.path(), bin, unused))
        .await
        .unwrap_err();
    assert!(matches!(err, certo_pdns::PdnsError::NotReady(_)), "{err}");
}
