//! In-memory stand-in for the `PowerDNS` API, for tests of crates that drive it.
#![allow(
    clippy::significant_drop_tightening,
    reason = "handlers hold the state lock for one request"
)]

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use rustc_hash::FxHashMap;
use serde_json::{Value, json};

use crate::{ChangeType, Client, CryptoKey, Delegation, DelegationChecker, RrSet, TsigKey};

/// Delegation checker that answers every check with a configured outcome and counts calls.
#[derive(Debug, Default)]
pub struct ScriptedChecker {
    outcome: Mutex<Option<Result<Delegation, String>>>,
    calls: std::sync::atomic::AtomicUsize,
}

impl ScriptedChecker {
    /// Sets the outcome of the following checks.
    pub fn set(&self, outcome: Result<Delegation, String>) {
        *lock(&self.outcome) = Some(outcome);
    }

    /// Number of checks run so far.
    pub fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl DelegationChecker for ScriptedChecker {
    async fn check(&self, _zone: &str, _nsname: &str) -> Result<Delegation, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        lock(&self.outcome)
            .clone()
            .unwrap_or_else(|| Ok(Delegation::NotDelegated("not delegated".into())))
    }
}

/// A zone held by the mock.
#[derive(Debug, Clone)]
pub struct MockZone {
    /// Name with trailing dot.
    pub name: String,
    /// SOA serial, bumped on every change.
    pub serial: u32,
    /// All rrsets.
    pub rrsets: Vec<RrSet>,
    /// NSEC3 parameters.
    pub nsec3param: String,
    /// DNSSEC keys.
    pub keys: Vec<CryptoKey>,
    /// Metadata by kind.
    pub metadata: FxHashMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
struct Inner {
    zones: Vec<MockZone>,
    tsig: Vec<TsigKey>,
    next_key: u32,
}

/// A running mock server.
#[derive(Debug, Clone)]
pub struct MockPdns {
    port: u16,
    state: Arc<Mutex<Inner>>,
}

type Shared = Arc<Mutex<Inner>>;

fn lock<T>(state: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn error(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

fn zone_json(z: &MockZone) -> Value {
    json!({
        "name": z.name, "id": z.name, "kind": "Native", "dnssec": z.keys.iter().any(|k| k.active),
        "serial": z.serial, "nsec3param": z.nsec3param,
        "rrsets": z.rrsets.iter().map(|rr| json!({
            "name": rr.name, "type": rr.rtype, "ttl": rr.ttl,
            "records": rr.records.iter().map(|r| json!({ "content": r.content, "disabled": r.disabled })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

fn key_json(k: &CryptoKey) -> Value {
    json!({ "id": k.id, "keytype": k.keytype, "active": k.active, "published": k.published,
            "algorithm": k.algorithm, "dnskey": k.dnskey, "ds": k.ds })
}

fn tsig_json(k: &TsigKey) -> Value {
    json!({ "id": k.id, "name": k.name, "algorithm": k.algorithm, "key": k.key, "type": "TSIGKey" })
}

async fn server() -> Json<Value> {
    Json(json!({ "version": "mock", "daemon_type": "authoritative" }))
}

async fn list_zones(State(s): State<Shared>) -> Json<Value> {
    let inner = lock(&s);
    Json(Value::Array(
        inner
            .zones
            .iter()
            .map(|z| {
                json!({ "name": z.name, "id": z.name, "kind": "Native", "serial": z.serial,
                        "dnssec": z.keys.iter().any(|k| k.active) })
            })
            .collect(),
    ))
}

async fn create_zone(State(s): State<Shared>, Json(body): Json<Value>) -> Response {
    let Some(name) = body["name"].as_str().filter(|n| n.ends_with('.')) else {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Zone name must end with a dot",
        );
    };
    let mut inner = lock(&s);
    if inner.zones.iter().any(|z| z.name == name) {
        return error(StatusCode::CONFLICT, "Domain already exists");
    }
    let mut rrsets: Vec<RrSet> = serde_json::from_value(body["rrsets"].clone()).unwrap_or_default();
    for rr in &mut rrsets {
        rr.changetype = None;
    }
    if let Some(ns) = body["nameservers"].as_array() {
        let records = ns
            .iter()
            .filter_map(Value::as_str)
            .map(crate::RecordData::new)
            .collect::<Vec<_>>();
        if !records.is_empty() {
            rrsets.push(RrSet::replace(name, "NS", 3600, records));
        }
    }
    if !rrsets.iter().any(|rr| rr.rtype == "SOA") {
        rrsets.push(RrSet::replace(
            name,
            "SOA",
            3600,
            vec![crate::RecordData::new(
                "ns. hostmaster. 1 10800 3600 604800 3600",
            )],
        ));
    }
    for rr in &mut rrsets {
        rr.changetype = None;
    }
    let zone = MockZone {
        name: name.to_owned(),
        serial: 1,
        rrsets,
        nsec3param: String::new(),
        keys: Vec::new(),
        metadata: FxHashMap::default(),
    };
    let view = zone_json(&zone);
    inner.zones.push(zone);
    (StatusCode::CREATED, Json(view)).into_response()
}

fn with_zone<T>(s: &Shared, id: &str, f: impl FnOnce(&mut MockZone) -> T) -> Option<T> {
    let mut inner = lock(s);
    inner.zones.iter_mut().find(|z| z.name == id).map(f)
}

async fn get_zone(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    with_zone(&s, &id, |z| Json(zone_json(z)).into_response())
        .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn put_zone(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    with_zone(&s, &id, |z| {
        if let Some(p) = body["nsec3param"].as_str() {
            p.clone_into(&mut z.nsec3param);
        }
        StatusCode::NO_CONTENT.into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn delete_zone(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let mut inner = lock(&s);
    let before = inner.zones.len();
    inner.zones.retain(|z| z.name != id);
    if inner.zones.len() == before {
        return error(StatusCode::NOT_FOUND, "Not Found");
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn patch_zone(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let changes: Vec<RrSet> = match serde_json::from_value(body["rrsets"].clone()) {
        Ok(c) => c,
        Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()),
    };
    for rr in &changes {
        if !rr.name.ends_with('.') || !rr.name.ends_with(&id) && rr.name != id {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("RRset {} IN {}: Name is out of zone", rr.name, rr.rtype),
            );
        }
        if rr.rtype == "TXT"
            && rr.changetype == Some(ChangeType::Replace)
            && rr.records.iter().any(|r| {
                r.content.len() < 2 || !r.content.starts_with('"') || !r.content.ends_with('"')
            })
        {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("Record {} IN TXT: Parsing record content failed", rr.name),
            );
        }
    }
    with_zone(&s, &id, |z| {
        for change in changes {
            z.rrsets
                .retain(|rr| !(rr.name == change.name && rr.rtype == change.rtype));
            if change.changetype == Some(ChangeType::Replace) {
                z.rrsets.push(RrSet {
                    changetype: None,
                    ..change
                });
            }
        }
        z.serial += 1;
        StatusCode::NO_CONTENT.into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn list_keys(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    with_zone(&s, &id, |z| {
        Json(Value::Array(z.keys.iter().map(key_json).collect())).into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn add_key(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let next = {
        let mut inner = lock(&s);
        inner.next_key += 1;
        inner.next_key
    };
    with_zone(&s, &id, |z| {
        let key = CryptoKey {
            id: next,
            keytype: body["keytype"].as_str().unwrap_or("csk").to_owned(),
            active: body["active"].as_bool().unwrap_or(true),
            published: true,
            algorithm: "ECDSAP256SHA256".to_owned(),
            dnskey: format!("257 3 13 mockkey{next}"),
            ds: vec![format!("{next} 13 2 mockds{next}")],
        };
        let view = key_json(&key);
        z.keys.push(key);
        (StatusCode::CREATED, Json(view)).into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn delete_key(State(s): State<Shared>, Path((id, key)): Path<(String, u32)>) -> Response {
    with_zone(&s, &id, |z| {
        let before = z.keys.len();
        z.keys.retain(|k| k.id != key);
        if z.keys.len() == before {
            return error(StatusCode::NOT_FOUND, "Not Found");
        }
        StatusCode::NO_CONTENT.into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn get_metadata(
    State(s): State<Shared>,
    Path((id, kind)): Path<(String, String)>,
) -> Response {
    with_zone(&s, &id, |z| {
        z.metadata.get(&kind).map_or_else(
            || error(StatusCode::NOT_FOUND, "Not Found"),
            |values| {
                Json(json!({ "kind": kind, "metadata": values, "type": "Metadata" }))
                    .into_response()
            },
        )
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn put_metadata(
    State(s): State<Shared>,
    Path((id, kind)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let values: Vec<String> = serde_json::from_value(body["metadata"].clone()).unwrap_or_default();
    with_zone(&s, &id, |z| {
        z.metadata.insert(kind.clone(), values.clone());
        Json(json!({ "kind": kind, "metadata": values, "type": "Metadata" })).into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn delete_metadata(
    State(s): State<Shared>,
    Path((id, kind)): Path<(String, String)>,
) -> Response {
    with_zone(&s, &id, |z| {
        z.metadata.remove(&kind);
        StatusCode::NO_CONTENT.into_response()
    })
    .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "Not Found"))
}

async fn list_tsig(State(s): State<Shared>) -> Json<Value> {
    Json(Value::Array(lock(&s).tsig.iter().map(tsig_json).collect()))
}

async fn add_tsig(State(s): State<Shared>, Json(body): Json<Value>) -> Response {
    let Some(name) = body["name"].as_str() else {
        return error(StatusCode::UNPROCESSABLE_ENTITY, "name required");
    };
    let mut inner = lock(&s);
    if inner.tsig.iter().any(|k| k.name == name) {
        return error(
            StatusCode::CONFLICT,
            "A TSIG key with the name already exists",
        );
    }
    let key = TsigKey {
        id: format!("{name}."),
        name: name.to_owned(),
        algorithm: body["algorithm"]
            .as_str()
            .unwrap_or("hmac-sha256")
            .to_owned(),
        key: body["key"].as_str().map_or_else(
            || {
                use base64::Engine as _;
                let n = inner.tsig.len();
                base64::engine::general_purpose::STANDARD.encode(format!("mock-secret-{n:0>20}"))
            },
            str::to_owned,
        ),
    };
    let view = tsig_json(&key);
    inner.tsig.push(key);
    (StatusCode::CREATED, Json(view)).into_response()
}

async fn delete_tsig(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let mut inner = lock(&s);
    let before = inner.tsig.len();
    inner.tsig.retain(|k| k.id != id);
    if inner.tsig.len() == before {
        return error(StatusCode::NOT_FOUND, "Not Found");
    }
    StatusCode::NO_CONTENT.into_response()
}

impl MockPdns {
    /// Starts the mock on a free port.
    pub async fn start() -> std::io::Result<Self> {
        let state: Shared = Arc::default();
        let app = Router::new()
            .route("/api/v1/servers/localhost", get(server))
            .route(
                "/api/v1/servers/localhost/zones",
                get(list_zones).post(create_zone),
            )
            .route(
                "/api/v1/servers/localhost/zones/{id}",
                get(get_zone)
                    .put(put_zone)
                    .delete(delete_zone)
                    .patch(patch_zone),
            )
            .route(
                "/api/v1/servers/localhost/zones/{id}/cryptokeys",
                get(list_keys).post(add_key),
            )
            .route(
                "/api/v1/servers/localhost/zones/{id}/cryptokeys/{key}",
                axum::routing::delete(delete_key),
            )
            .route(
                "/api/v1/servers/localhost/zones/{id}/metadata/{kind}",
                get(get_metadata).put(put_metadata).delete(delete_metadata),
            )
            .route(
                "/api/v1/servers/localhost/cache/flush",
                axum::routing::put(|| async {
                    Json(json!({"count": 0, "result": "Flushed cache."}))
                }),
            )
            .route(
                "/api/v1/servers/localhost/tsigkeys",
                get(list_tsig).post(add_tsig),
            )
            .route(
                "/api/v1/servers/localhost/tsigkeys/{id}",
                axum::routing::delete(delete_tsig),
            )
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "mock pdns stopped");
            }
        });
        Ok(Self { port, state })
    }

    /// Port on 127.0.0.1.
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// A client bound to the mock.
    pub fn client(&self) -> Client {
        Client::new(&format!("http://127.0.0.1:{}", self.port), "mock")
            .unwrap_or_else(|e| unreachable!("mock client: {e}"))
    }

    /// A zone by name (with or without trailing dot).
    pub fn zone(&self, name: &str) -> Option<MockZone> {
        let id = format!("{}.", name.trim_end_matches('.'));
        lock(&self.state)
            .zones
            .iter()
            .find(|z| z.name == id)
            .cloned()
    }

    /// Names of all zones, without trailing dots.
    pub fn zone_names(&self) -> Vec<String> {
        lock(&self.state)
            .zones
            .iter()
            .map(|z| z.name.trim_end_matches('.').to_owned())
            .collect()
    }

    /// All TSIG keys.
    pub fn tsig_keys(&self) -> Vec<TsigKey> {
        lock(&self.state).tsig.clone()
    }
}

impl MockZone {
    /// Record contents of one rrset.
    pub fn contents(&self, name: &str, rtype: &str) -> Vec<String> {
        let id = format!("{}.", name.trim_end_matches('.'));
        self.rrsets
            .iter()
            .find(|rr| rr.name == id && rr.rtype == rtype)
            .map(|rr| rr.records.iter().map(|r| r.content.clone()).collect())
            .unwrap_or_default()
    }
}
