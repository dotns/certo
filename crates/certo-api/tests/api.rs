//! Router-level checks of auth, CORS, limits and acme-dns rules.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::net::SocketAddr;

use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode, header},
};
use certo_api::{ApiOptions, AppState};
use certo_db::Db;
use certo_dns::{DynamicCatalog, ZonePolicy, Zones};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn options() -> ApiOptions {
    ApiOptions {
        base_domain: "auth.example.org".into(),
        api_domain: "auth.example.org".into(),
        jwt_secret: "test-secret".to_owned().into(),
        admin_key: Some("admin".to_owned().into()),
        allow_registration: true,
        allow_zone_creation: true,
        cors_origins: vec!["https://dash.example.org".into()],
        client_ip_header: None,
        tls: false,
    }
}

struct Api {
    app: Router,
    catalog: DynamicCatalog,
}

struct Resp {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl Api {
    fn new(opts: ApiOptions, peer: &str) -> Self {
        let db = Db::open(":memory:").unwrap();
        let catalog = DynamicCatalog::new();
        let policy = ZonePolicy::new(
            "auth.example.org.".parse().unwrap(),
            "ns1.auth.example.org",
            "admin.example.org",
            vec![],
        )
        .unwrap();
        let zones = Zones::new(db.clone(), catalog.clone(), policy);
        let state = AppState::new(db, zones, opts).unwrap();
        let peer: SocketAddr = peer.parse().unwrap();
        Self {
            app: certo_api::app(state).layer(MockConnectInfo(peer)),
            catalog,
        }
    }

    async fn send(&self, req: Request<Body>) -> Resp {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Resp {
            status,
            headers,
            body,
        }
    }

    async fn json(&self, method: &str, uri: &str, headers: &[(&str, &str)], body: Value) -> Resp {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        self.send(req.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn get(&self, uri: &str, headers: &[(&str, &str)]) -> Resp {
        let mut req = Request::builder().uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        self.send(req.body(Body::empty()).unwrap()).await
    }
}

#[tokio::test]
async fn probes() {
    let api = Api::new(options(), "127.0.0.1:1");
    for path in ["/health", "/livez", "/readyz"] {
        assert_eq!(api.get(path, &[]).await.status, StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn admin_key_is_required_and_checked() {
    let api = Api::new(options(), "127.0.0.1:1");
    let r = api.get("/admin/users", &[("x-admin-key", "wrong")]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.body, json!({"error": "invalid_admin_key"}));
    assert_eq!(
        api.get("/admin/users", &[("x-admin-key", "admin")])
            .await
            .status,
        StatusCode::OK
    );

    let api = Api::new(
        ApiOptions {
            admin_key: None,
            ..options()
        },
        "127.0.0.1:1",
    );
    let r = api.get("/admin/users", &[("x-admin-key", "")]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body, json!({"error": "admin_key_not_configured"}));
}

#[tokio::test]
async fn oversized_and_malformed_bodies_are_invalid_json() {
    let api = Api::new(options(), "127.0.0.1:1");
    let huge = json!({ "username": "a".repeat(70 * 1024), "password": "secret1" });
    for body in [huge, json!("not an object")] {
        let r = api.json("POST", "/api/login", &[], body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.body, json!({"error": "invalid_json"}));
    }
}

#[tokio::test]
async fn sessions_and_tokens() {
    let api = Api::new(options(), "127.0.0.1:1");
    let creds = json!({"username": "alice", "password": "secret1"});
    let r = api.json("POST", "/api/register", &[], creds.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let token = r.body["token"].as_str().unwrap().to_owned();
    let bearer = format!("Bearer {token}");
    let r = api.get("/api/profile", &[("authorization", &bearer)]).await;
    assert_eq!(r.body, json!({"username": "alice"}));

    // A token signed with another secret is rejected.
    let other = Api::new(
        ApiOptions {
            jwt_secret: "other".to_owned().into(),
            ..options()
        },
        "127.0.0.1:1",
    );
    other.json("POST", "/api/register", &[], creds).await;
    let r = other
        .get("/api/profile", &[("authorization", &bearer)])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let r = api
        .json(
            "POST",
            "/api/login",
            &[],
            json!({"username": "alice", "password": "wrong!"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.body, json!({"error": "invalid_credentials"}));
}

#[tokio::test]
async fn cors_preflight_and_actual_requests() {
    let api = Api::new(options(), "127.0.0.1:1");
    let preflight = |origin: &'static str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/api/domains")
            .header("origin", origin)
            .header("access-control-request-method", "DELETE")
            .header("access-control-request-headers", "authorization")
            .body(Body::empty())
            .unwrap()
    };
    let r = api.send(preflight("https://dash.example.org")).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        r.headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://dash.example.org"
    );
    assert_eq!(r.headers[header::ACCESS_CONTROL_ALLOW_METHODS], "DELETE");
    assert_eq!(
        r.headers[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "Authorization"
    );

    let r = api.send(preflight("https://evil.example")).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(!r.headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));

    let r = api
        .get("/api/info", &[("origin", "https://dash.example.org")])
        .await;
    assert_eq!(
        r.headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://dash.example.org"
    );
    assert_eq!(r.headers[header::VARY], "Origin");
}

async fn register_acmedns(api: &Api, allow: &[&str]) -> (String, String, String) {
    let r = api
        .json("POST", "/register", &[], json!({ "allowfrom": allow }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let s = |k: &str| r.body[k].as_str().unwrap().to_owned();
    (s("username"), s("password"), s("subdomain"))
}

#[tokio::test]
async fn acmedns_update_honors_allowfrom_and_forwarded_header() {
    let api = Api::new(options(), "198.51.100.7:4000");
    let (user, key, sub) = register_acmedns(&api, &["198.51.100.0/24"]).await;
    let txt = "a".repeat(43);
    let hdr = [("x-api-user", user.as_str()), ("x-api-key", key.as_str())];
    let body = json!({"subdomain": sub, "txt": txt});
    let r = api.json("POST", "/update", &hdr, body.clone()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, json!({ "txt": txt }));

    // The forwarded header is ignored unless configured...
    let mut hdr_fwd = hdr.to_vec();
    hdr_fwd.push(("x-forwarded-for", "203.0.113.9, 198.51.100.7"));
    let r = api.json("POST", "/update", &hdr_fwd, body.clone()).await;
    assert_eq!(r.status, StatusCode::OK);

    // ...and its first entry is the client when it is.
    let api = Api::new(
        ApiOptions {
            client_ip_header: Some("x-forwarded-for".parse().unwrap()),
            ..options()
        },
        "198.51.100.7:4000",
    );
    let (user, key, sub) = register_acmedns(&api, &["198.51.100.0/24"]).await;
    let hdr = [
        ("x-api-user", user.as_str()),
        ("x-api-key", key.as_str()),
        ("x-forwarded-for", "203.0.113.9, 198.51.100.7"),
    ];
    let r = api
        .json(
            "POST",
            "/update",
            &hdr,
            json!({"subdomain": sub, "txt": txt}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body, json!({"error": "ip_not_allowed"}));
}

#[tokio::test]
async fn registration_can_be_disabled() {
    let api = Api::new(
        ApiOptions {
            allow_registration: false,
            ..options()
        },
        "127.0.0.1:1",
    );
    for path in ["/register", "/api/register"] {
        let r = api.json("POST", path, &[], json!({})).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(r.body, json!({"error": "registration_disabled"}));
    }
}

#[tokio::test]
async fn storage_and_httpreq_challenge_with_basic_realm() {
    let api = Api::new(options(), "127.0.0.1:1");
    let r = api.get("/acmedns/example.com", &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        r.headers[header::WWW_AUTHENTICATE],
        "Basic realm=\"acmedns\""
    );
    let r = api
        .json("POST", "/present", &[], json!({"fqdn": "x", "value": "y"}))
        .await;
    assert_eq!(
        r.headers[header::WWW_AUTHENTICATE],
        "Basic realm=\"httpreq\""
    );
}

async fn session(api: &Api, username: &str) -> String {
    let r = api
        .json(
            "POST",
            "/api/register",
            &[],
            json!({"username": username, "password": "secret1"}),
        )
        .await;
    format!("Bearer {}", r.body["token"].as_str().unwrap())
}

async fn scoped_key(api: &Api, bearer: &str, scope: &[&str]) -> String {
    let r = api
        .json(
            "POST",
            "/api/keys",
            &[("authorization", bearer)],
            json!({"name": "k", "scope": scope}),
        )
        .await;
    format!("Bearer {}", r.body["key"].as_str().unwrap())
}

#[tokio::test]
async fn zone_and_record_lifecycle() {
    let api = Api::new(options(), "127.0.0.1:1");
    let alice = session(&api, "alice").await;
    let auth = [("authorization", alice.as_str())];

    let r = api
        .json("POST", "/api/zones", &auth, json!({"name": "Example.COM."}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["name"], "example.com");
    assert_eq!(r.body["dnssec"], false);
    assert!(api.catalog.contains(&"example.com.".parse().unwrap()));

    let r = api
        .json("POST", "/api/zones", &auth, json!({"name": "example.com"}))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    let r = api
        .json(
            "POST",
            "/api/zones/example.com/records",
            &auth,
            json!({"name": "www", "type": "a", "data": "192.0.2.1"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        r.body,
        json!({"id": r.body["id"], "name": "www.example.com", "type": "A", "ttl": 3600, "data": "192.0.2.1"})
    );
    let id = r.body["id"].as_i64().unwrap();

    let r = api
        .json(
            "POST",
            "/api/zones/example.com/records",
            &auth,
            json!({"name": "www", "type": "CNAME", "data": "x.example.net"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["error"], "cname_conflict");
    assert!(r.body["message"].as_str().unwrap().contains("CNAME"));

    let r = api
        .json(
            "PUT",
            &format!("/api/zones/example.com/records/{id}"),
            &auth,
            json!({"name": "www", "type": "A", "ttl": 60, "data": "192.0.2.2"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["ttl"], 60);

    let r = api.get("/api/zones/example.com", &auth).await;
    let records = r.body["records"].as_array().unwrap();
    assert_eq!(
        records
            .iter()
            .map(|r| r["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["NS", "A"]
    );
    assert_eq!(records[0]["data"], "ns1.auth.example.org.");

    let del = |path: String| {
        Request::builder()
            .method("DELETE")
            .uri(path)
            .header("authorization", alice.as_str())
            .body(Body::empty())
            .unwrap()
    };
    let r = api
        .send(del(format!("/api/zones/example.com/records/{id}")))
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = api
        .send(del(format!("/api/zones/example.com/records/{id}")))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.body["error"], "record_not_found");
    let r = api.send(del("/api/zones/example.com".into())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(!api.catalog.contains(&"example.com.".parse().unwrap()));
}

#[tokio::test]
async fn zones_are_private_and_scoped() {
    let api = Api::new(options(), "127.0.0.1:1");
    let alice = session(&api, "alice").await;
    let bob = session(&api, "bob").await;
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", &alice)],
            json!({"name": "alice.test"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);

    // Another user sees nothing and cannot take a child zone.
    let r = api
        .get("/api/zones/alice.test", &[("authorization", &bob)])
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.body["error"], "zone_not_found");
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", &bob)],
            json!({"name": "x.alice.test"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["error"], "zone_not_allowed");
    let r = api.get("/api/zones", &[("authorization", &bob)]).await;
    assert_eq!(r.body, json!([]));

    // A scoped key only reaches zones its scope covers.
    let key = scoped_key(&api, &alice, &["*.other.test"]).await;
    let r = api
        .get("/api/zones/alice.test", &[("authorization", &key)])
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", &key)],
            json!({"name": "alice2.test"}),
        )
        .await;
    assert_eq!(r.body["error"], "domain_not_in_scope");
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", &key)],
            json!({"name": "sub.other.test"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = api.get("/api/zones", &[("authorization", &key)]).await;
    assert_eq!(r.body.as_array().unwrap().len(), 1);

    // The server's own domain is off limits.
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", &alice)],
            json!({"name": "x.auth.example.org"}),
        )
        .await;
    assert_eq!(r.body["error"], "zone_not_allowed");
}

#[tokio::test]
async fn zone_creation_is_admin_only_by_default() {
    let api = Api::new(
        ApiOptions {
            allow_zone_creation: false,
            ..options()
        },
        "127.0.0.1:1",
    );
    let alice = session(&api, "alice").await;
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", &alice)],
            json!({"name": "alice.test"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["error"], "zone_creation_disabled");
    assert_eq!(
        api.get("/api/info", &[]).await.body["allow_zone_creation"],
        false
    );

    let admin = [("x-admin-key", "admin")];
    let users = api.get("/admin/users", &admin).await;
    let uid = users.body[0]["id"].as_i64().unwrap();
    let r = api
        .json(
            "POST",
            "/admin/zones",
            &admin,
            json!({"user_id": uid, "name": "alice.test"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["owner"], "alice");
    let r = api
        .json(
            "POST",
            "/admin/zones",
            &admin,
            json!({"user_id": 999, "name": "x.test"}),
        )
        .await;
    assert_eq!(r.body["error"], "user_not_found");

    // The owner manages the assigned zone; admin sees it with its owner.
    let r = api
        .json(
            "POST",
            "/api/zones/alice.test/records",
            &[("authorization", &alice)],
            json!({"name": "@", "type": "TXT", "data": "hello world"}),
        )
        .await;
    assert_eq!(r.body["data"], "\"hello\" \"world\"");
    let r = api.get("/admin/zones", &admin).await;
    assert_eq!(r.body[0]["owner"], "alice");

    // Deleting the account removes the zone from DNS.
    let r = api
        .send(
            Request::builder()
                .method("DELETE")
                .uri("/api/profile")
                .header("authorization", alice.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(!api.catalog.contains(&"alice.test.".parse().unwrap()));
    assert_eq!(api.get("/admin/zones", &admin).await.body, json!([]));
}

/// An API with alice owning `example.com`; returns alice's session header value.
async fn with_zone() -> (Api, String) {
    let api = Api::new(options(), "127.0.0.1:1");
    let alice = session(&api, "alice").await;
    let r = api
        .json(
            "POST",
            "/api/zones",
            &[("authorization", alice.as_str())],
            json!({"name": "example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    (api, alice)
}

#[tokio::test]
async fn dnssec_settings() {
    let (api, alice) = with_zone().await;
    let auth = [("authorization", alice.as_str())];
    let admin = [("x-admin-key", "admin")];

    let r = api.get("/api/zones/example.com/dnssec", &auth).await;
    assert_eq!(r.body, json!({"enabled": false, "nsec_mode": "nsec"}));
    let r = api
        .json(
            "PUT",
            "/api/zones/example.com/dnssec",
            &auth,
            json!({"enabled": true, "nsec_mode": "nsec5"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["error"], "invalid_nsec_mode");
    let r = api
        .json(
            "PUT",
            "/api/zones/example.com/dnssec",
            &auth,
            json!({"enabled": true, "nsec_mode": "NSEC3"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["nsec_mode"], "nsec3");
    assert!(r.body["ds"].as_str().unwrap().contains(" 13 2 "));
    let r = api.get("/admin/zones/example.com/dnssec", &admin).await;
    assert_eq!(r.body["enabled"], true);
    assert_eq!(
        api.get("/api/zones/example.com", &auth).await.body["dnssec"],
        true
    );

    let bob = session(&api, "bob").await;
    let r = api
        .get(
            "/api/zones/example.com/dnssec",
            &[("authorization", bob.as_str())],
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn transfer_settings_and_tsig_keys() {
    let (api, alice) = with_zone().await;
    let auth = [("authorization", alice.as_str())];
    let admin = [("x-admin-key", "admin")];
    let r = api.get("/api/zones/example.com/axfr", &auth).await;
    assert_eq!(r.body, json!({"allow_from": [], "tsig_keys": []}));
    let r = api
        .json(
            "PUT",
            "/api/zones/example.com/axfr",
            &auth,
            json!({"allow_from": ["not-an-ip"]}),
        )
        .await;
    assert_eq!(r.body["error"], "invalid_cidr");
    let r = api
        .json(
            "PUT",
            "/api/zones/example.com/axfr",
            &auth,
            json!({"allow_from": ["192.0.2.7/24", "2001:db8::1"]}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["allow_from"],
        json!(["192.0.2.0/24", "2001:db8::1/128"])
    );

    let r = api
        .json(
            "POST",
            "/api/zones/example.com/tsig-keys",
            &auth,
            json!({"name": "xfr.example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["algorithm"], "hmac-sha256");
    assert_eq!(r.body["secret"].as_str().unwrap().len(), 44);
    let r = api
        .json(
            "POST",
            "/admin/zones/example.com/tsig-keys",
            &admin,
            json!({"name": "xfr.example.com", "algorithm": "hmac-sha512"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.body["error"], "tsig_key_exists");
    let r = api.get("/api/zones/example.com/axfr", &auth).await;
    let keys = r.body["tsig_keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].get("secret").is_none());

    let del = |path: &str, headers: &[(&str, &str)]| {
        let mut req = Request::builder().method("DELETE").uri(path);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.body(Body::empty()).unwrap()
    };
    let path = "/api/zones/example.com/tsig-keys/xfr.example.com";
    assert_eq!(
        api.send(del(path, &auth)).await.status,
        StatusCode::NO_CONTENT
    );
    let r = api.send(del(path, &auth)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.body["error"], "tsig_key_not_found");

    let bob = session(&api, "bob").await;
    let r = api
        .get(
            "/api/zones/example.com/axfr",
            &[("authorization", bob.as_str())],
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = api.get("/admin/zones/example.com/axfr", &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn deleting_users_and_domains_clears_challenge_txt() {
    let api = Api::new(options(), "127.0.0.1:1");
    let admin = [("x-admin-key", "admin")];
    let txt = "a".repeat(43);
    let mut accounts = Vec::new();
    for _ in 0..2 {
        let (user, key, sub) = register_acmedns(&api, &[]).await;
        let hdr = [("x-api-user", user.as_str()), ("x-api-key", key.as_str())];
        let r = api
            .json(
                "POST",
                "/update",
                &hdr,
                json!({"subdomain": sub, "txt": txt}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
        accounts.push((user, key, sub));
    }
    assert_eq!(
        api.get("/admin/records", &admin)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Removing a domain drops its TXT values.
    let (_, key, sub) = &accounts[0];
    let r = api
        .send(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/domains/{sub}"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let records = api.get("/admin/records", &admin).await.body;
    assert_eq!(records.as_array().unwrap().len(), 1);
    assert_eq!(
        records[0]["domain"],
        format!("{}.auth.example.org", accounts[1].2)
    );

    // Deleting a user through the admin API drops the rest.
    let users = api.get("/admin/users", &admin).await.body;
    let id = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == accounts[1].0)
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let r = api
        .send(
            Request::builder()
                .method("DELETE")
                .uri(format!("/admin/users/{id}"))
                .header("x-admin-key", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(api.get("/admin/records", &admin).await.body, json!([]));
}
