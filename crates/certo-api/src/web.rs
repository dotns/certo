//! Dashboard API under `/api`.
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use certo_core::internal_domain;
use certo_db::DbError;
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::{Bearer, GlobalBearer, JsonBody, hash_password, issue_token, verify_password},
    error::{ApiError, ApiResult},
    state::AppState,
    views::{DomainView, KeyView, txt_views},
};

/// Minimum password length for new accounts.
pub(crate) const MIN_PASSWORD_LEN: usize = 6;

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Credentials {
    pub(crate) username: String,
    pub(crate) password: String,
}

impl Credentials {
    /// Trims the username and enforces the password policy.
    pub(crate) fn validated(mut self) -> ApiResult<Self> {
        self.username = self.username.trim().to_owned();
        if self.username.is_empty() || self.password.len() < MIN_PASSWORD_LEN {
            return Err(ApiError::bad_request(
                "username required, password min 6 chars",
            ));
        }
        Ok(self)
    }
}

fn session(state: &AppState, user: &certo_core::User, status: StatusCode) -> ApiResult<Response> {
    let token = issue_token(state, user)?;
    Ok((
        status,
        Json(json!({ "token": token, "username": user.username })),
    )
        .into_response())
}

pub(crate) async fn register(
    State(state): State<AppState>,
    body: ApiResult<JsonBody<Credentials>>,
) -> ApiResult<Response> {
    if !state.opts.allow_registration {
        return Err(ApiError::forbidden("registration_disabled"));
    }
    let creds = body?.0.validated()?;
    let hash = hash_password(creds.password).await?;
    let user = state
        .db
        .create_user(&creds.username, &hash)
        .await
        .map_err(|e| match e {
            DbError::Conflict(_) => ApiError::new(StatusCode::CONFLICT, "username_taken"),
            e => ApiError::db("register", &e),
        })?;
    session(&state, &user, StatusCode::CREATED)
}

pub(crate) async fn login(
    State(state): State<AppState>,
    JsonBody(creds): JsonBody<Credentials>,
) -> ApiResult<Response> {
    let invalid = || ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials");
    let Ok(user) = state.db.user_by_username(creds.username.trim()).await else {
        verify_password(creds.password, state.dummy_hash.to_string()).await;
        return Err(invalid());
    };
    if !verify_password(creds.password, user.password_hash.clone()).await {
        return Err(invalid());
    }
    session(&state, &user, StatusCode::OK)
}

pub(crate) async fn info(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "provider": "ns-certo",
        "version": certo_core::VERSION,
        "base_domain": state.opts.base_domain,
        "api_domain": state.opts.api_domain,
        "allow_zone_creation": state.opts.allow_zone_creation,
        "capabilities": [
            "multi_key",
            "scoped_key",
            "domain_management",
            "wildcard_scope",
            "account_deletion",
            "acmedns",
            "zones",
        ],
    }))
}

pub(crate) async fn profile(GlobalBearer(auth): GlobalBearer) -> Json<serde_json::Value> {
    Json(json!({ "username": auth.user.username }))
}

/// Deletes a user with everything they own and stops serving their zones.
pub(crate) async fn delete_user_for(state: &AppState, user_id: i64) -> ApiResult<()> {
    let zones = state
        .db
        .zones(Some(user_id))
        .await
        .map_err(|e| ApiError::db("delete user", &e))?;
    state
        .db
        .delete_user(user_id, &state.opts.base_domain)
        .await
        .map_err(|e| ApiError::db("delete user", &e))?;
    for zone in zones {
        state.zones.publish(&zone.name).await?;
    }
    Ok(())
}

pub(crate) async fn delete_account(
    State(state): State<AppState>,
    GlobalBearer(auth): GlobalBearer,
) -> ApiResult<StatusCode> {
    delete_user_for(&state, auth.user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_domains(
    State(state): State<AppState>,
    auth: Bearer,
) -> ApiResult<Json<Vec<DomainView>>> {
    let domains = state
        .db
        .user_domains(auth.user.id)
        .await
        .map_err(|e| ApiError::db("list domains", &e))?;
    Ok(Json(
        domains
            .into_iter()
            .filter(|d| auth.may_access(&d.domain))
            .map(|d| DomainView::new(d, &state.opts.base_domain))
            .collect(),
    ))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct DomainRequest {
    domain: String,
}

/// Inserts a domain, mapping a duplicate to `409 domain_already_exists`.
pub(crate) async fn add_domain_for(
    state: &AppState,
    user_id: i64,
    username: &str,
    domain: &str,
) -> ApiResult<Response> {
    match state.db.add_user_domain(user_id, username, domain).await {
        Ok(d) => Ok((
            StatusCode::CREATED,
            Json(DomainView::new(d, &state.opts.base_domain)),
        )
            .into_response()),
        Err(DbError::Conflict(_)) => {
            Err(ApiError::new(StatusCode::CONFLICT, "domain_already_exists"))
        }
        Err(e) => Err(ApiError::db("add domain", &e)),
    }
}

pub(crate) async fn add_domain(
    State(state): State<AppState>,
    auth: Bearer,
    JsonBody(req): JsonBody<DomainRequest>,
) -> ApiResult<Response> {
    let domain = req.domain.trim().to_lowercase();
    if domain.is_empty() {
        return Err(ApiError::bad_request("domain_required"));
    }
    // A scoped key may only add in-scope domains; scopes change only via /api/keys.
    if !auth.may_access(&domain) {
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    add_domain_for(&state, auth.user.id, &auth.user.username, &domain).await
}

pub(crate) async fn remove_domain(
    State(state): State<AppState>,
    auth: Bearer,
    Path(domain): Path<String>,
) -> ApiResult<StatusCode> {
    let domain = domain.trim().to_lowercase();
    if !auth.may_access(&domain) {
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    state
        .db
        .remove_user_domain(auth.user.id, &domain, &state.opts.base_domain)
        .await
        .map_err(|e| ApiError::db("remove domain", &e))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_records(
    State(state): State<AppState>,
    auth: Bearer,
) -> ApiResult<Response> {
    let domains = state
        .db
        .user_domains(auth.user.id)
        .await
        .map_err(|e| ApiError::db("list records", &e))?;
    let names: Vec<String> = domains
        .iter()
        .filter(|d| auth.may_access(&d.domain))
        .map(|d| internal_domain(&d.subdomain, &state.opts.base_domain))
        .collect();
    let records = state
        .db
        .list_txt_records_by_domains(&names)
        .await
        .map_err(|e| ApiError::db("list records", &e))?;
    Ok(Json(txt_views(records)).into_response())
}

pub(crate) async fn list_keys(
    State(state): State<AppState>,
    GlobalBearer(auth): GlobalBearer,
) -> ApiResult<Json<Vec<KeyView>>> {
    let keys = state
        .db
        .list_api_keys(auth.user.id)
        .await
        .map_err(|e| ApiError::db("list keys", &e))?;
    Ok(Json(keys.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct KeyRequest {
    name: String,
    scope: Vec<String>,
}

pub(crate) async fn create_key(
    State(state): State<AppState>,
    GlobalBearer(auth): GlobalBearer,
    JsonBody(req): JsonBody<KeyRequest>,
) -> ApiResult<Response> {
    if req.name.is_empty() {
        return Err(ApiError::bad_request("name_required"));
    }
    let scope = if req.scope.is_empty() {
        vec!["*".to_owned()]
    } else {
        req.scope
    };
    let key = state
        .db
        .create_api_key(auth.user.id, &req.name, scope)
        .await
        .map_err(|e| ApiError::db("create key", &e))?;
    Ok((StatusCode::CREATED, Json(KeyView::from(key))).into_response())
}

pub(crate) async fn delete_key(
    State(state): State<AppState>,
    GlobalBearer(auth): GlobalBearer,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("invalid_id"))?;
    state
        .db
        .delete_api_key(auth.user.id, id)
        .await
        .map_err(|e| ApiError::db("delete key", &e))?;
    Ok(StatusCode::NO_CONTENT)
}
