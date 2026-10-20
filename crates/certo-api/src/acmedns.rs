//! acme-dns protocol: native `/register` + `/update`, and lego's HTTP storage under `/acmedns`.
//!
//! Both act on the same records as the httpreq endpoints. Storage lets an existing certo
//! user drive acme-dns without local state: lego fetches the account (username, API key,
//! subdomain) from `/acmedns/{domain}` and then calls `/update`.
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};

use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, FromRequestParts, Path, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use certo_core::{ACMEDNS_TXT_LENGTH, ApiKey, User, internal_domain};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{JsonBody, basic_credentials, user_key},
    error::{ApiError, ApiResult},
    state::AppState,
};

/// The caller's IP, honoring the configured client-IP header.
#[derive(Debug)]
pub(crate) struct ClientIp(pub(crate) String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(name) = &state.opts.client_ip_header
            && let Some(v) = parts.headers.get(name).and_then(|v| v.to_str().ok())
            && !v.is_empty()
        {
            let first = v.split(',').next().unwrap_or_default();
            return Ok(Self(first.trim().to_owned()));
        }
        let peer = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .map(|c| c.0.ip().to_canonical().to_string())
            .unwrap_or_default();
        Ok(Self(peer))
    }
}

/// Whether `ip` is inside any CIDR of `allow_from`; an empty list allows all.
fn ip_allowed(ip: &str, allow_from: &[String]) -> bool {
    if allow_from.is_empty() {
        return true;
    }
    let Ok(ip) = ip.parse::<IpAddr>().map(|ip| ip.to_canonical()) else {
        return false;
    };
    allow_from
        .iter()
        .filter_map(|c| c.trim().parse::<IpNet>().ok())
        .any(|net| net.contains(&ip))
}

/// Credentials authenticated by Basic auth for the storage endpoints.
#[derive(Debug)]
pub(crate) struct StorageAuth {
    user: User,
    key: ApiKey,
    username: String,
    secret: String,
}

impl FromRequestParts<AppState> for StorageAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let (username, secret) =
            basic_credentials(&parts.headers).ok_or(ApiError::basic("acmedns"))?;
        let (user, key) = user_key(state, &username, &secret)
            .await
            .ok_or(ApiError::basic("acmedns"))?;
        Ok(Self {
            user,
            key,
            username,
            secret,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct Account {
    fulldomain: String,
    subdomain: String,
    username: String,
    password: String,
    server_url: String,
}

/// External base URL of this API (the acme-dns `/update` base, not the storage path).
fn server_url(state: &AppState, headers: &HeaderMap, parts_host: Option<&str>) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .unwrap_or(if state.opts.tls { "https" } else { "http" });
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .or(parts_host)
        .unwrap_or_default();
    format!("{scheme}://{host}")
}

/// Request metadata needed to build accounts.
#[derive(Debug)]
pub(crate) struct Origin(String);

impl FromRequestParts<AppState> for Origin {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let authority = parts
            .uri
            .authority()
            .map(axum::http::uri::Authority::as_str);
        std::future::ready(Ok(Self(server_url(state, &parts.headers, authority))))
    }
}

impl StorageAuth {
    fn account(&self, state: &AppState, origin: &Origin, subdomain: String) -> Account {
        Account {
            fulldomain: internal_domain(&subdomain, &state.opts.base_domain),
            subdomain,
            username: self.username.clone(),
            password: self.secret.clone(),
            server_url: origin.0.clone(),
        }
    }

    /// The subdomain for `domain`, provisioning it on first use when in scope.
    async fn subdomain(&self, state: &AppState, domain: &str) -> ApiResult<String> {
        if !self.key.has_domain_access(domain) {
            return Err(ApiError::forbidden("domain_not_in_scope"));
        }
        if let Ok(sub) = state
            .db
            .subdomain_by_user_domain(self.user.id, domain)
            .await
        {
            return Ok(sub);
        }
        let d = state
            .db
            .add_user_domain(self.user.id, &self.user.username, domain)
            .await
            .map_err(|e| ApiError::db("acme-dns auto-provision", &e))?;
        tracing::info!(user = %self.user.username, domain, subdomain = %d.subdomain, "acme-dns: domain auto-provisioned");
        Ok(d.subdomain)
    }
}

/// Maps a lego storage key onto a certo domain: `*.Example.com.` → `example.com`.
fn normalize_domain(domain: &str) -> String {
    let d = domain.trim().to_lowercase();
    let d = d.strip_suffix('.').unwrap_or(&d);
    d.strip_prefix("*.").unwrap_or(d).to_owned()
}

/// `GET` and `POST /acmedns/{domain}`: the account for one domain (get-or-create).
pub(crate) async fn storage_fetch(
    State(state): State<AppState>,
    auth: StorageAuth,
    origin: Origin,
    Path(domain): Path<String>,
) -> ApiResult<Json<Account>> {
    let domain = normalize_domain(&domain);
    if domain.is_empty() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "domain_not_found"));
    }
    let sub = auth.subdomain(&state, &domain).await?;
    Ok(Json(auth.account(&state, &origin, sub)))
}

/// `GET /acmedns`: every in-scope domain of the user.
pub(crate) async fn storage_fetch_all(
    State(state): State<AppState>,
    auth: StorageAuth,
    origin: Origin,
) -> ApiResult<Json<BTreeMap<String, Account>>> {
    let domains = state
        .db
        .user_domains(auth.user.id)
        .await
        .map_err(|e| ApiError::db("acme-dns fetch all", &e))?;
    Ok(Json(
        domains
            .into_iter()
            .filter(|d| auth.key.has_domain_access(&d.domain))
            .map(|d| {
                let account = auth.account(&state, &origin, d.subdomain);
                (d.domain, account)
            })
            .collect(),
    ))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RegisterRequest {
    allowfrom: Option<Vec<String>>,
}

#[derive(Serialize)]
struct RegisterResponse {
    username: String,
    password: String,
    fulldomain: String,
    subdomain: String,
    allowfrom: Vec<String>,
}

/// `POST /register`: anonymous acme-dns account with a random subdomain.
pub(crate) async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> ApiResult<Response> {
    if !state.opts.allow_registration {
        return Err(ApiError::forbidden("registration_disabled"));
    }
    // The body is optional; malformed input registers without restrictions.
    let req: RegisterRequest = body
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let allow_from = req.allowfrom.unwrap_or_default();
    let (username, password, subdomain) = state
        .db
        .create_acmedns_account(allow_from.clone())
        .await
        .map_err(|e| ApiError::db("acme-dns register", &e))?;
    tracing::info!(%username, %subdomain, client_ip = %ip, "acme-dns account registered");
    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            username,
            password,
            fulldomain: internal_domain(&subdomain, &state.opts.base_domain),
            subdomain,
            allowfrom: allow_from,
        }),
    )
        .into_response())
}

/// `X-Api-User` / `X-Api-Key` authentication for `/update`.
#[derive(Debug)]
pub(crate) struct UpdateAuth {
    user: User,
    key: ApiKey,
}

impl FromRequestParts<AppState> for UpdateAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let get = |name| {
            parts
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
        };
        let (user, key) = user_key(state, get("x-api-user"), get("x-api-key"))
            .await
            .ok_or(ApiError::basic("acmedns"))?;
        Ok(Self { user, key })
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct UpdateRequest {
    subdomain: String,
    txt: String,
}

/// `POST /update`: sets the challenge TXT of the caller's subdomain.
pub(crate) async fn update(
    State(state): State<AppState>,
    UpdateAuth { user, key }: UpdateAuth,
    ClientIp(ip): ClientIp,
    JsonBody(req): JsonBody<UpdateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if req.txt.len() != ACMEDNS_TXT_LENGTH {
        return Err(ApiError::bad_request("bad_txt"));
    }
    let subdomain = req.subdomain.trim().to_lowercase();
    let (_, domain, allow_from) = state
        .db
        .user_domain_by_subdomain(&subdomain)
        .await
        .ok()
        .filter(|(owner, ..)| *owner == user.id)
        .ok_or(ApiError::forbidden("forbidden"))?;
    if !key.has_domain_access(&domain) {
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    if !ip_allowed(&ip, &allow_from) {
        tracing::warn!(user = %user.username, %subdomain, client_ip = %ip, "acme-dns update: client IP not in allowfrom");
        return Err(ApiError::forbidden("ip_not_allowed"));
    }
    let name = internal_domain(&subdomain, &state.opts.base_domain);
    state
        .db
        .update_acmedns_txt(&name, &req.txt)
        .await
        .map_err(|e| ApiError::db("acme-dns update", &e))?;
    state.zones.sync_txt(&name).await?;
    tracing::info!(user = %user.username, %subdomain, "acme-dns TXT updated");
    Ok(Json(serde_json::json!({ "txt": req.txt })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list() {
        assert!(ip_allowed("anything", &[]));
        let list = vec![" 198.51.100.0/24 ".to_owned(), "bogus".to_owned()];
        assert!(ip_allowed("198.51.100.7", &list));
        assert!(!ip_allowed("203.0.113.1", &list));
        assert!(!ip_allowed("not-an-ip", &list));
        assert!(ip_allowed("2001:db8::1", &["2001:db8::/32".to_owned()]));
    }

    #[test]
    fn storage_keys() {
        assert_eq!(normalize_domain(" *.Example.COM. "), "example.com");
        assert_eq!(normalize_domain("example.com"), "example.com");
        assert_eq!(normalize_domain("*."), "*");
    }
}
