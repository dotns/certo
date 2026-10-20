//! HTTP API for certo: lego httpreq, acme-dns, the dashboard API and the SPA.
#![forbid(unsafe_code)]

mod acme;
mod acmedns;
mod admin;
mod auth;
mod cors;
mod error;
mod httpreq;
mod spa;
mod state;
mod tls;
mod views;
mod web;
mod zones;

use std::{io, net::SocketAddr, time::Duration};

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{Method, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{get, post, put},
};
use tower::ServiceExt;

pub use acme::{
    AcmeError, AcmeOptions, ChallengeHook, LETS_ENCRYPT, LETS_ENCRYPT_STAGING, start as start_acme,
};
pub use axum_server::{Handle, tls_rustls::RustlsConfig};
pub use state::{ApiOptions, AppState};
pub use tls::{TlsError, from_pem_files};

/// Request bodies are small JSON documents; cap them at 64 KiB.
const MAX_BODY: usize = 64 << 10;
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(5);

fn api_router(state: AppState) -> Router {
    let cors = cors::Cors::new(&state.opts.cors_origins);
    Router::new()
        .route("/present", post(httpreq::present))
        .route("/cleanup", post(httpreq::cleanup))
        .route("/register", post(acmedns::register))
        .route("/update", post(acmedns::update))
        .route("/acmedns", get(acmedns::storage_fetch_all))
        .route(
            "/acmedns/{domain}",
            get(acmedns::storage_fetch).post(acmedns::storage_fetch),
        )
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/livez", get(|| async { StatusCode::OK }))
        .route("/readyz", get(readyz))
        .route("/llms.txt", get(llms_txt))
        .route("/api/register", post(web::register))
        .route("/api/login", post(web::login))
        .route("/api/info", get(web::info))
        .route(
            "/api/profile",
            get(web::profile).delete(web::delete_account),
        )
        .route("/api/keys", get(web::list_keys).post(web::create_key))
        .route("/api/keys/{id}", axum::routing::delete(web::delete_key))
        .route("/api/domains", get(web::list_domains).post(web::add_domain))
        .route(
            "/api/domains/{domain}",
            axum::routing::delete(web::remove_domain),
        )
        .route("/api/records", get(web::list_records))
        .route(
            "/admin/users",
            get(admin::list_users).post(admin::create_user),
        )
        .route(
            "/admin/users/{id}",
            axum::routing::delete(admin::delete_user),
        )
        .route(
            "/admin/domains",
            get(admin::list_domains).post(admin::add_domain),
        )
        .route(
            "/admin/domains/{domain}",
            axum::routing::delete(admin::remove_domain),
        )
        .route("/admin/records", get(admin::list_records))
        .route("/api/zones", get(zones::list).post(zones::create))
        .route(
            "/admin/zones",
            get(zones::admin_list).post(zones::admin_create),
        )
        .route("/admin/zones/{zone}/activation", put(zones::set_activation))
        .merge(zone_routes("/api/zones"))
        .merge(zone_routes("/admin/zones"))
        .layer(middleware::from_fn_with_state(cors, cors::layer))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

/// Per-zone routes; [`zones::ZoneCtx`] authorizes by prefix.
fn zone_routes(prefix: &str) -> Router<AppState> {
    Router::new()
        .route(
            &format!("{prefix}/{{zone}}"),
            get(zones::get).delete(zones::delete),
        )
        .route(
            &format!("{prefix}/{{zone}}/rrsets"),
            get(zones::list_rrsets).patch(zones::patch_rrsets),
        )
        .route(&format!("{prefix}/{{zone}}/verify"), post(zones::verify))
        .route(
            &format!("{prefix}/{{zone}}/dnssec"),
            get(zones::get_dnssec).put(zones::set_dnssec),
        )
        .route(
            &format!("{prefix}/{{zone}}/axfr"),
            get(zones::get_transfer).put(zones::set_transfer),
        )
        .route(
            &format!("{prefix}/{{zone}}/tsig-keys"),
            post(zones::add_tsig_key),
        )
        .route(
            &format!("{prefix}/{{zone}}/tsig-keys/{{name}}"),
            axum::routing::delete(zones::delete_tsig_key),
        )
}

async fn readyz(State(state): State<AppState>) -> StatusCode {
    match state.db.ping().await {
        Ok(()) if state.zones.ping().await => StatusCode::OK,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn llms_txt() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        include_str!("llms-api.txt"),
    )
}

/// Paths handled by the API; everything else is the SPA. `GET /register` is the
/// dashboard signup page, only `POST /register` is acme-dns.
fn is_api_path(method: &Method, path: &str) -> bool {
    matches!(
        path,
        "/present" | "/cleanup" | "/update" | "/health" | "/livez" | "/readyz" | "/llms.txt"
    ) || (path == "/register" && method == Method::POST)
        || ["/api", "/admin", "/acmedns"]
            .iter()
            .any(|p| path.starts_with(p))
}

/// The complete HTTP application.
pub fn app(state: AppState) -> Router {
    let api = api_router(state);
    let Some(spa) = spa::Spa::detect() else {
        return api;
    };
    Router::new().fallback(move |req: Request| async move {
        if is_api_path(req.method(), req.uri().path()) {
            match api.oneshot(req).await {
                Ok(resp) => resp,
                Err(never) => match never {},
            }
        } else {
            spa.serve(req.uri().path()).await
        }
    })
}

/// Serves `app` on `listener` until `handle` is shut down; TLS when `tls` is set.
pub async fn serve(
    listener: std::net::TcpListener,
    app: Router,
    tls: Option<RustlsConfig>,
    handle: Handle<SocketAddr>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let make = app.into_make_service_with_connect_info::<SocketAddr>();
    match tls {
        None => {
            let mut server = axum_server::from_tcp(listener)?.handle(handle);
            tune(server.http_builder());
            server.serve(make).await
        }
        Some(cfg) => {
            let mut server = axum_server::from_tcp_rustls(listener, cfg)?.handle(handle);
            tune(server.http_builder());
            server.serve(make).await
        }
    }
}

fn tune(builder: &mut hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>) {
    builder
        .http1()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
}
