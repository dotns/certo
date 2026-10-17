//! Hosted zones: `/api/zones` for owners and `/admin/zones` for operators.
use axum::{
    Json,
    extract::{FromRequestParts, Path, RawPathParams, State},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use certo_core::{Zone, ZoneRecord};
use certo_dns::{DnssecInfo, RecordInput, TransferInfo, TsigKeyInfo, ZoneOpError, parse_zone_name};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{Admin, Bearer, JsonBody},
    error::{ApiError, ApiResult},
    state::AppState,
};

impl From<ZoneOpError> for ApiError {
    fn from(e: ZoneOpError) -> Self {
        match &e {
            ZoneOpError::Record(r) => Self::bad_request(r.code()).with_message(r.to_string()),
            ZoneOpError::NotAllowed(why) => Self::forbidden("zone_not_allowed").with_message(*why),
            ZoneOpError::Exists => Self::new(StatusCode::CONFLICT, "zone_already_exists"),
            ZoneOpError::NotFound("record") => Self::new(StatusCode::NOT_FOUND, "record_not_found"),
            ZoneOpError::NotFound("tsig key") => {
                Self::new(StatusCode::NOT_FOUND, "tsig_key_not_found")
            }
            ZoneOpError::NotFound(_) => Self::new(StatusCode::NOT_FOUND, "zone_not_found"),
            ZoneOpError::Invalid { code, message } => {
                Self::bad_request(code).with_message(message.clone())
            }
            ZoneOpError::Db(_)
            | ZoneOpError::Build(_)
            | ZoneOpError::Dnssec(_)
            | ZoneOpError::Task => {
                tracing::error!(error = %e, "zone operation failed");
                Self::internal("zone_error")
            }
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ZoneView {
    name: String,
    serial: u32,
    dnssec: bool,
    created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    records: Option<Vec<RecordView>>,
}

impl ZoneView {
    fn new(z: Zone, with_owner: bool) -> Self {
        Self {
            name: z.name,
            serial: z.serial,
            dnssec: z.dnssec,
            created_at: z.created_at,
            owner: z.owner.filter(|_| with_owner),
            records: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct RecordView {
    id: i64,
    name: String,
    #[serde(rename = "type")]
    rtype: String,
    ttl: u32,
    data: String,
}

impl From<ZoneRecord> for RecordView {
    fn from(r: ZoneRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            rtype: r.rtype,
            ttl: r.ttl,
            data: r.data,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct RecordRequest {
    name: String,
    #[serde(rename = "type")]
    rtype: String,
    ttl: Option<u32>,
    data: String,
}

impl RecordRequest {
    fn input(&self) -> RecordInput<'_> {
        RecordInput {
            name: &self.name,
            rtype: &self.rtype,
            ttl: self.ttl,
            data: &self.data,
        }
    }
}

fn record_id(id: &str) -> ApiResult<i64> {
    id.parse()
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "record_not_found"))
}

/// The zone named in the path, authorized for the caller: `/admin/...` routes need the
/// admin key, the others a session or key of the owner whose scope covers the zone.
/// Other users' zones are reported as missing.
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
        if zone.user_id != auth.user.id {
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
            .map(|z| ZoneView::new(z, false))
            .collect(),
    ))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Bearer,
    JsonBody(req): JsonBody<CreateZone>,
) -> ApiResult<Response> {
    if !state.opts.allow_zone_creation {
        return Err(ApiError::forbidden("zone_creation_disabled"));
    }
    let name = parse_zone_name(&req.name).map_err(ZoneOpError::from)?;
    let name = name.to_string();
    if !auth.may_access(name.trim_end_matches('.')) {
        return Err(ApiError::forbidden("domain_not_in_scope"));
    }
    let zone = state.zones.create(auth.user.id, &name).await?;
    tracing::info!(user = %auth.user.username, zone = %zone.name, "zone created");
    Ok((StatusCode::CREATED, Json(ZoneView::new(zone, false))).into_response())
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
        zones.into_iter().map(|z| ZoneView::new(z, true)).collect(),
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
    let zone = state.zones.create(user.id, &req.name).await?;
    tracing::info!(user = %user.username, zone = %zone.name, "zone created by admin");
    Ok((StatusCode::CREATED, Json(ZoneView::new(zone, true))).into_response())
}

// --- Per-zone endpoints, mounted under both /api/zones and /admin/zones ---

pub(crate) async fn get(State(state): State<AppState>, ctx: ZoneCtx) -> ApiResult<Json<ZoneView>> {
    let records = state.zones.records(&ctx.zone).await?;
    let mut view = ZoneView::new(ctx.zone, ctx.admin);
    view.records = Some(records.into_iter().map(Into::into).collect());
    Ok(Json(view))
}

pub(crate) async fn delete(State(state): State<AppState>, ctx: ZoneCtx) -> ApiResult<StatusCode> {
    state.zones.delete(&ctx.zone).await?;
    tracing::info!(zone = %ctx.zone.name, admin = ctx.admin, "zone deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_records(
    State(state): State<AppState>,
    ctx: ZoneCtx,
) -> ApiResult<Json<Vec<RecordView>>> {
    let records = state.zones.records(&ctx.zone).await?;
    Ok(Json(records.into_iter().map(Into::into).collect()))
}

pub(crate) async fn add_record(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    JsonBody(req): JsonBody<RecordRequest>,
) -> ApiResult<Response> {
    let rec = state.zones.add_record(&ctx.zone, req.input()).await?;
    Ok((StatusCode::CREATED, Json(RecordView::from(rec))).into_response())
}

pub(crate) async fn update_record(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    Path((_, id)): Path<(String, String)>,
    JsonBody(req): JsonBody<RecordRequest>,
) -> ApiResult<Json<RecordView>> {
    let rec = state
        .zones
        .update_record(&ctx.zone, record_id(&id)?, req.input())
        .await?;
    Ok(Json(rec.into()))
}

pub(crate) async fn delete_record(
    State(state): State<AppState>,
    ctx: ZoneCtx,
    Path((_, id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    state
        .zones
        .delete_record(&ctx.zone, record_id(&id)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub(crate) struct DnssecView {
    enabled: bool,
    nsec_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dnskey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ds: Option<String>,
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
    created_at: i64,
    /// Base64 secret, only in the creation response.
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

impl From<TsigKeyInfo> for TsigKeyView {
    fn from(k: TsigKeyInfo) -> Self {
        Self {
            name: k.name,
            algorithm: k.algorithm,
            created_at: k.created_at,
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
    view.secret = Some(STANDARD.encode(secret));
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
