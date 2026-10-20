//! lego `httpreq` provider endpoints: `POST /present` and `POST /cleanup`.
use axum::{Json, extract::State, http::StatusCode};
use certo_core::{
    MAX_TXT_VALUE_LENGTH, User, extract_domain_from_fqdn, extract_subdomain_from_fqdn,
    internal_domain,
};
use certo_db::DbError;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{HttpreqAuth, JsonBody},
    error::{ApiError, ApiResult},
    state::AppState,
};

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Payload {
    fqdn: String,
    value: String,
}

impl Payload {
    fn checked(self) -> ApiResult<Self> {
        if self.fqdn.is_empty() || self.value.is_empty() {
            return Err(ApiError::bad_request("missing_fqdn_or_value"));
        }
        Ok(self)
    }
}

#[derive(Serialize)]
pub(crate) struct PresentResponse {
    internal_domain: String,
    cname_target: String,
}

/// Resolves the subdomain behind `fqdn`, which is either the CNAME-resolved
/// `<sub>.<base>` name or the original `_acme-challenge.<domain>`.
async fn resolve_subdomain(state: &AppState, user_id: i64, fqdn: &str) -> Result<String, DbError> {
    if let Some(sub) = extract_subdomain_from_fqdn(fqdn, &state.opts.base_domain)
        && state.db.subdomain_owner(&sub).await.ok() == Some(user_id)
    {
        return Ok(sub);
    }
    state
        .db
        .subdomain_by_user_domain(user_id, &extract_domain_from_fqdn(fqdn))
        .await
}

/// Provisions `domain` on first use; never for names under the base domain.
async fn auto_create(state: &AppState, user: &User, fqdn: &str, domain: &str) -> Option<String> {
    if extract_subdomain_from_fqdn(fqdn, &state.opts.base_domain).is_some() {
        return None;
    }
    match state
        .db
        .add_user_domain(user.id, &user.username, domain)
        .await
    {
        Ok(d) => {
            tracing::info!(user = %user.username, domain, "present: domain auto-created");
            Some(d.subdomain)
        }
        // A concurrent create may have won the race.
        Err(_) => state
            .db
            .subdomain_by_user_domain(user.id, domain)
            .await
            .ok(),
    }
}

pub(crate) async fn present(
    State(state): State<AppState>,
    HttpreqAuth { user, key }: HttpreqAuth,
    JsonBody(payload): JsonBody<Payload>,
) -> ApiResult<Json<PresentResponse>> {
    let payload = payload.checked()?;
    if payload.value.len() > MAX_TXT_VALUE_LENGTH {
        return Err(ApiError::bad_request("value_too_long"));
    }
    let domain = extract_domain_from_fqdn(&payload.fqdn);
    if !key.has_domain_access(&domain) {
        tracing::warn!(user = %user.username, fqdn = %payload.fqdn, "present: key scope denied");
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    let subdomain = match resolve_subdomain(&state, user.id, &payload.fqdn).await {
        Ok(sub) => Some(sub),
        Err(_) => auto_create(&state, &user, &payload.fqdn, &domain).await,
    }
    .ok_or_else(|| {
        tracing::warn!(user = %user.username, fqdn = %payload.fqdn, "present: domain not authorized");
        ApiError::forbidden("domain_not_authorized")
    })?;
    let name = internal_domain(&subdomain, &state.opts.base_domain);
    state
        .db
        .present_txt(&name, &payload.value)
        .await
        .map_err(|e| ApiError::db("present", &e))?;
    state.zones.sync_txt(&name).await?;
    tracing::info!(fqdn = %payload.fqdn, internal_domain = %name, "TXT record presented");
    Ok(Json(PresentResponse {
        internal_domain: name.clone(),
        cname_target: name,
    }))
}

pub(crate) async fn cleanup(
    State(state): State<AppState>,
    HttpreqAuth { user, key }: HttpreqAuth,
    JsonBody(payload): JsonBody<Payload>,
) -> ApiResult<StatusCode> {
    let payload = payload.checked()?;
    if !key.has_domain_access(&extract_domain_from_fqdn(&payload.fqdn)) {
        tracing::warn!(user = %user.username, fqdn = %payload.fqdn, "cleanup: key scope denied");
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    let subdomain = resolve_subdomain(&state, user.id, &payload.fqdn)
        .await
        .map_err(|_| ApiError::forbidden("domain_not_authorized"))?;
    let name = internal_domain(&subdomain, &state.opts.base_domain);
    state
        .db
        .cleanup_txt(&name, &payload.value)
        .await
        .map_err(|e| ApiError::db("cleanup", &e))?;
    state.zones.sync_txt(&name).await?;
    tracing::info!(fqdn = %payload.fqdn, internal_domain = %name, "TXT record cleaned up");
    Ok(StatusCode::OK)
}
