use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Request},
    http::{HeaderMap, StatusCode, header, request::Parts},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use certo_core::{ApiKey, User};
use jsonwebtoken::{Algorithm, Header, Validation};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use subtle::ConstantTimeEq;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

const TOKEN_TTL: Duration = Duration::from_secs(72 * 3600);

#[derive(Serialize, Deserialize)]
struct Claims {
    uid: i64,
    usr: String,
    exp: u64,
    iat: u64,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Issues a dashboard session token.
pub(crate) fn issue_token(state: &AppState, user: &User) -> ApiResult<String> {
    let iat = unix_now();
    let claims = Claims {
        uid: user.id,
        usr: user.username.clone(),
        exp: iat + TOKEN_TTL.as_secs(),
        iat,
    };
    jsonwebtoken::encode(&Header::default(), &claims, &state.jwt.0).map_err(|e| {
        tracing::error!(error = %e, "sign token");
        ApiError::internal("token_error")
    })
}

fn verify_token(state: &AppState, token: &str) -> Option<i64> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.algorithms = vec![Algorithm::HS256, Algorithm::HS384, Algorithm::HS512];
    validation.leeway = 0;
    jsonwebtoken::decode::<Claims>(token, &state.jwt.1, &validation)
        .ok()
        .map(|t| t.claims.uid)
}

/// Checks a password against a bcrypt hash off the async runtime.
pub(crate) async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash).unwrap_or(false))
        .await
        .unwrap_or(false)
}

/// Hashes a new password off the async runtime.
pub(crate) async fn hash_password(password: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || {
        bcrypt::non_truncating_hash_with_result(password, crate::state::BCRYPT_COST)
            .map(|h| h.format_for_version(bcrypt::Version::TwoA))
    })
    .await
    .ok()
    .and_then(Result::ok)
    .ok_or_else(|| ApiError::internal("internal_error"))
}

/// Parses `Authorization: Basic` credentials.
pub(crate) fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_at_checked(6)?;
    if !scheme.eq_ignore_ascii_case("Basic ") {
        return None;
    }
    let decoded = String::from_utf8(STANDARD.decode(encoded).ok()?).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    Some((user.to_owned(), pass.to_owned()))
}

/// Validates a username + API key pair; the key must belong to the user.
pub(crate) async fn user_key(
    state: &AppState,
    username: &str,
    secret: &str,
) -> Option<(User, ApiKey)> {
    if username.is_empty() || secret.is_empty() {
        return None;
    }
    let user = state.db.user_by_username(username).await.ok()?;
    let key = state.db.api_key_by_value(secret).await.ok()?;
    (key.user_id == user.id).then_some((user, key))
}

/// A user authenticated by a dashboard JWT or any API key (`Authorization: Bearer`).
#[derive(Debug)]
pub(crate) struct Bearer {
    pub(crate) user: User,
    /// Set when authenticated by API key; scope checks apply.
    pub(crate) key: Option<ApiKey>,
}

impl Bearer {
    /// Whether the caller may act on `domain` (JWT sessions may act on all).
    pub(crate) fn may_access(&self, domain: &str) -> bool {
        self.key
            .as_ref()
            .is_none_or(|k| k.has_domain_access(domain))
    }
}

impl FromRequestParts<AppState> for Bearer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthorized)?;
        if let Some(uid) = verify_token(state, token)
            && let Ok(user) = state.db.user_by_id(uid).await
        {
            return Ok(Self { user, key: None });
        }
        if let Ok(key) = state.db.api_key_by_value(token).await
            && let Ok(user) = state.db.user_by_id(key.user_id).await
        {
            return Ok(Self {
                user,
                key: Some(key),
            });
        }
        Err(ApiError::unauthorized())
    }
}

/// A [`Bearer`] limited to JWT sessions and global (`*`) API keys.
#[derive(Debug)]
pub(crate) struct GlobalBearer(pub(crate) Bearer);

impl FromRequestParts<AppState> for GlobalBearer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let bearer = Bearer::from_request_parts(parts, state).await?;
        if bearer.key.as_ref().is_some_and(|k| !k.is_global()) {
            return Err(ApiError::forbidden("global_key_required"));
        }
        Ok(Self(bearer))
    }
}

/// httpreq Basic auth: username + one of the user's API keys.
#[derive(Debug)]
pub(crate) struct HttpreqAuth {
    pub(crate) user: User,
    pub(crate) key: ApiKey,
}

impl FromRequestParts<AppState> for HttpreqAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let (username, secret) =
            basic_credentials(&parts.headers).ok_or(ApiError::basic("httpreq"))?;
        let (user, key) = user_key(state, &username, &secret)
            .await
            .ok_or(ApiError::basic("httpreq"))?;
        Ok(Self { user, key })
    }
}

/// `X-Admin-Key` authentication for `/admin/*`.
#[derive(Debug)]
pub(crate) struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = ApiResult<Self>> + Send {
        std::future::ready(check_admin(parts, state))
    }
}

fn check_admin(parts: &Parts, state: &AppState) -> ApiResult<Admin> {
    let expected = state
        .opts
        .admin_key
        .as_ref()
        .ok_or(ApiError::forbidden("admin_key_not_configured"))?;
    let given = parts
        .headers
        .get("x-admin-key")
        .map_or(&[][..], |v| v.as_bytes());
    if given.ct_eq(expected.expose_secret().as_bytes()).into() {
        Ok(Admin)
    } else {
        Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_admin_key"))
    }
}

/// A JSON body; any read or decode failure (including the size limit) is `400 invalid_json`.
#[derive(Debug)]
pub(crate) struct JsonBody<T>(pub(crate) T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for JsonBody<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> ApiResult<Self> {
        let body = Bytes::from_request(req, state)
            .await
            .map_err(|_| ApiError::bad_request("invalid_json"))?;
        serde_json::from_slice(&body)
            .map(Self)
            .map_err(|_| ApiError::bad_request("invalid_json"))
    }
}
