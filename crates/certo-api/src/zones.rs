//! Hosted zones: `/api/zones` for owners and `/admin/zones` for operators. Records live in
//! `PowerDNS` and are exposed as rrsets.
use axum::{
    Json,
    extract::{FromRequestParts, Path, RawPathParams, State},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use certo_core::Zone;
use certo_pdns::{DnssecInfo, RrSet, RrSetChange, TransferInfo, TsigKeyInfo, ZoneOpError};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{Admin, Bearer, JsonBody},
    error::{ApiError, ApiResult},
    state::AppState,
};

impl From<ZoneOpError> for ApiError {
    fn from(e: ZoneOpError) -> Self {
        match &e {
            ZoneOpError::Invalid { code, message } => {
                Self::bad_request(code).with_message(message.clone())
            }
            ZoneOpError::NotAllowed(why) => Self::forbidden("zone_not_allowed").with_message(*why),
            ZoneOpError::Exists => Self::new(StatusCode::CONFLICT, "zone_already_exists"),
            ZoneOpError::NotFound("tsig key") => {
                Self::new(StatusCode::NOT_FOUND, "tsig_key_not_found")
            }
            ZoneOpError::NotFound(_) => Self::new(StatusCode::NOT_FOUND, "zone_not_found"),
            ZoneOpError::NotActive => Self::new(StatusCode::CONFLICT, "zone_not_active")
                .with_message("the zone is served once its delegation is verified"),
            ZoneOpError::RateLimited => {
                Self::new(StatusCode::TOO_MANY_REQUESTS, "check_rate_limited")
            }
            ZoneOpError::Check(why) => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, "check_failed").with_message(why.clone())
            }
            ZoneOpError::Db(_) | ZoneOpError::Pdns(_) => {
                tracing::error!(error = %e, "zone operation failed");
                Self::internal("zone_error")
            }
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ZoneView {
    name: String,
    status: &'static str,
    exempt: bool,
    system: bool,
    verified_at: Option<i64>,
    last_check: Option<i64>,
    last_error: Option<String>,
    failed_checks: u32,
    created_at: i64,
    ns_target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rrsets: Option<Vec<RrSetView>>,
}

impl ZoneView {
    fn new(state: &AppState, z: Zone, with_owner: bool) -> Self {
        Self {
            name: z.name,
            status: if z.active { "active" } else { "pending" },
            exempt: z.exempt,
            system: z.user_id.is_none(),
            verified_at: z.verified_at,
            last_check: z.last_check,
            last_error: z.last_error,
            failed_checks: z.failed_checks,
            created_at: z.created_at,
            ns_target: state.zones.policy().nsname.clone(),
            owner: z.owner.filter(|_| with_owner),
            rrsets: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct RrSetView {
    name: String,
    #[serde(rename = "type")]
    rtype: String,
    ttl: u32,
    records: Vec<String>,
}

impl From<RrSet> for RrSetView {
    fn from(rr: RrSet) -> Self {
        Self {
            name: rr.name,
            rtype: rr.rtype,
            ttl: rr.ttl,
            records: rr.records.into_iter().map(|r| r.content).collect(),
        }
    }
}

fn rrset_views(rrsets: Vec<RrSet>) -> Vec<RrSetView> {
    rrsets.into_iter().map(Into::into).collect()
}

/// The zone named in the path, authorized for the caller: `/admin/...` routes need the
/// admin key, the others a session or key of the owner whose scope covers the zone.
/// Other users' zones (and the base zone) are reported as missing.
#[derive(Debug)]
pub(crate) struct ZoneCtx {
    zone: Zone,
    admin: bool,
}

impl FromRequestParts<AppState> for ZoneCtx {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let not_found = || ApiError::from(ZoneOpError::NotFound("zone"));
        let params = RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|_| not_found())?;
        let name = params
            .iter()
            .find_map(|(k, v)| (k == "zone").then(|| v.to_owned()))
            .ok_or_else(not_found)?;
        if parts.uri.path().starts_with("/admin/") {
            Admin::from_request_parts(parts, state).await?;
            let zone = state.zones.get(&name).await?;
            return Ok(Self { zone, admin: true });
        }
        let auth = Bearer::from_request_parts(parts, state).await?;
        let zone = state.zones.get(&name).await?;
        if zone.user_id != Some(auth.user.id) {
            return Err(not_found());
        }
        if !auth.may_access(&zone.name) {
            return Err(ApiError::forbidden("domain_not_in_scope"));
        }
        Ok(Self { zone, admin: false })
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct CreateZone {
    name: String,
    user_id: i64,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Bearer,
) -> ApiResult<Json<Vec<ZoneView>>> {
    let zones = state
        .db
        .zones(Some(auth.user.id))
        .await
        .map_err(|e| ApiError::db("list zones", &e))?;
    Ok(Json(
        zones
            .into_iter()
            .filter(|z| auth.may_access(&z.name))
            .map(|z| ZoneView::new(&state, z, false))
            .collect(),
    ))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Bearer,
    JsonBody(req): JsonBody<CreateZone>,
) -> ApiResult<Response> {
    let name = certo_pdns::parse_zone_name(&req.name)?;
    if !auth.may_access(&name) {
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    let zone = state.zones.create(Some(auth.user.id), &name).await?;
    tracing::info!(user = %auth.user.username, zone = %zone.name, active = zone.active, "zone created");
    Ok((
        StatusCode::CREATED,
        Json(ZoneView::new(&state, zone, false)),
    )
        .into_response())
}

pub(crate) async fn admin_list(
    _: Admin,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<ZoneView>>> {
    let zones = state
        .db
        .zones(None)
        .await
        .map_err(|e| ApiError::db("admin list zones", &e))?;
    Ok(Json(
        zones
            .into_iter()
            .map(|z| ZoneView::new(&state, z, true))
            .collect(),
    ))
}

pub(crate) async fn admin_create(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(req): JsonBody<CreateZone>,
) -> ApiResult<Response> {
    if req.user_id == 0 || req.name.trim().is_empty() {
        return Err(ApiError::bad_request("user_id and name required"));
    }
    let user = state
        .db
        .user_by_id(req.user_id)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "user_not_found"))?;
    let zone = state.zones.create(Some(user.id), &req.name).await?;
    tracing::info!(user = %user.username, zone = %zone.name, "zone created by admin");
    Ok((StatusCode::CREATED, Json(ZoneView::new(&state, zone, true))).into_response())
}

#[derive(Deserialize)]
pub(crate) struct ActivationRequest {
    exempt: bool,
}

/// `PUT /admin/zones/{zone}/activation`: exempt a zone from the delegation check (or not).
pub(crate) async fn set_activation(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    JsonBody(req): JsonBody<ActivationRequest>,
) -> ApiResult<Json<ZoneView>> {
    let zone = state.zones.set_exempt(&ctx.zone, req.exempt).await?;
    tracing::info!(zone = %zone.name, exempt = req.exempt, "zone activation changed by admin");
    Ok(Json(ZoneView::new(&state, zone, true)))
}

// --- Per-zone endpoints, mounted under both /api/zones and /admin/zones ---

pub(crate) async fn get(State(state): State<AppState>, ctx: ZoneCtx) -> ApiResult<Json<ZoneView>> {
    let rrsets = if ctx.zone.active {
        Some(rrset_views(state.zones.rrsets(&ctx.zone).await?))
    } else {
        None
    };
    let mut view = ZoneView::new(&state, ctx.zone, ctx.admin);
    view.rrsets = rrsets;
    Ok(Json(view))
}

pub(crate) async fn delete(State(state): State<AppState>, ctx: ZoneCtx) -> ApiResult<StatusCode> {
    state.zones.delete(&ctx.zone).await?;
    tracing::info!(zone = %ctx.zone.name, admin = ctx.admin, "zone deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// `POST .../verify`: run the delegation check now.
pub(crate) async fn verify(
    State(state): State<AppState>,
    ctx: ZoneCtx,
) -> ApiResult<Json<ZoneView>> {
    let zone = state.zones.verify(&ctx.zone).await?;
    Ok(Json(ZoneView::new(&state, zone, ctx.admin)))
}

pub(crate) async fn list_rrsets(
    State(state): State<AppState>,
    ctx: ZoneCtx,
) -> ApiResult<Json<Vec<RrSetView>>> {
    Ok(Json(rrset_views(state.zones.rrsets(&ctx.zone).await?)))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct PatchRequest {
    rrsets: Vec<RrSetChange>,
}

/// `PATCH .../rrsets`: REPLACE or DELETE whole rrsets; answers with the zone's rrsets.
pub(crate) async fn patch_rrsets(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    JsonBody(req): JsonBody<PatchRequest>,
) -> ApiResult<Json<Vec<RrSetView>>> {
    state.zones.patch_rrsets(&ctx.zone, &req.rrsets).await?;
    Ok(Json(rrset_views(state.zones.rrsets(&ctx.zone).await?)))
}

#[derive(Serialize)]
pub(crate) struct DnssecView {
    enabled: bool,
    nsec_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dnskey: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ds: Vec<String>,
}

impl From<DnssecInfo> for DnssecView {
    fn from(i: DnssecInfo) -> Self {
        Self {
            enabled: i.enabled,
            nsec_mode: i.nsec_mode,
            dnskey: i.dnskey,
            ds: i.ds,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct DnssecRequest {
    enabled: bool,
    #[serde(default = "default_nsec_mode")]
    nsec_mode: String,
}

fn default_nsec_mode() -> String {
    "nsec".to_owned()
}

pub(crate) async fn get_dnssec(
    State(state): State<AppState>,
    ctx: ZoneCtx,
) -> ApiResult<Json<DnssecView>> {
    Ok(Json(state.zones.dnssec(&ctx.zone).await?.into()))
}

pub(crate) async fn set_dnssec(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    JsonBody(req): JsonBody<DnssecRequest>,
) -> ApiResult<Json<DnssecView>> {
    let mode = req.nsec_mode.trim().to_lowercase();
    let info = state
        .zones
        .set_dnssec(&ctx.zone, req.enabled, &mode)
        .await?;
    tracing::info!(zone = %ctx.zone.name, enabled = req.enabled, nsec_mode = %mode, "dnssec changed");
    Ok(Json(info.into()))
}

#[derive(Serialize)]
pub(crate) struct TsigKeyView {
    name: String,
    algorithm: String,
    /// Base64 secret, only in the creation response.
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

impl From<TsigKeyInfo> for TsigKeyView {
    fn from(k: TsigKeyInfo) -> Self {
        Self {
            name: k.name,
            algorithm: k.algorithm,
            secret: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TransferView {
    allow_from: Vec<String>,
    tsig_keys: Vec<TsigKeyView>,
}

impl From<TransferInfo> for TransferView {
    fn from(t: TransferInfo) -> Self {
        Self {
            allow_from: t.allow_from,
            tsig_keys: t.tsig_keys.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct TransferRequest {
    allow_from: Vec<String>,
}

pub(crate) async fn get_transfer(
    State(state): State<AppState>,
    ctx: ZoneCtx,
) -> ApiResult<Json<TransferView>> {
    Ok(Json(state.zones.transfer(&ctx.zone).await?.into()))
}

pub(crate) async fn set_transfer(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    JsonBody(req): JsonBody<TransferRequest>,
) -> ApiResult<Json<TransferView>> {
    let info = state
        .zones
        .set_transfer_allow_from(&ctx.zone, &req.allow_from)
        .await?;
    Ok(Json(info.into()))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct TsigRequest {
    name: String,
    algorithm: Option<String>,
}

pub(crate) async fn add_tsig_key(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    JsonBody(req): JsonBody<TsigRequest>,
) -> ApiResult<Response> {
    let algorithm = req.algorithm.as_deref().unwrap_or("hmac-sha256");
    let (key, secret) = state
        .zones
        .add_tsig_key(&ctx.zone, &req.name, algorithm)
        .await?;
    let mut view = TsigKeyView::from(key);
    view.secret = Some(secret);
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

pub(crate) async fn delete_tsig_key(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    Path((_, name)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    state.zones.delete_tsig_key(&ctx.zone, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}
