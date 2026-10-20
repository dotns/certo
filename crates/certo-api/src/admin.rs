//! Operator endpoints under `/admin`, guarded by `X-Admin-Key`.
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use certo_db::DbError;
use serde::Deserialize;

use crate::{
    auth::{Admin, JsonBody, hash_password},
    error::{ApiError, ApiResult},
    state::AppState,
    views::{DomainView, TxtView, UserView, txt_views},
    web::{Credentials, add_domain_for, delete_user_for, sync_txt},
};

pub(crate) async fn list_users(
    _: Admin,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<UserView>>> {
    let users = state
        .db
        .list_users()
        .await
        .map_err(|e| ApiError::db("admin list users", &e))?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

pub(crate) async fn create_user(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(creds): JsonBody<Credentials>,
) -> ApiResult<Response> {
    let creds = creds.validated()?;
    let hash = hash_password(creds.password).await?;
    let user = state
        .db
        .create_user(&creds.username, &hash)
        .await
        .map_err(|e| match e {
            DbError::Conflict(_) => ApiError::new(StatusCode::CONFLICT, "username_taken"),
            e => ApiError::db("admin create user", &e),
        })?;
    Ok((StatusCode::CREATED, Json(UserView::from(user))).into_response())
}

pub(crate) async fn delete_user(
    _: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("invalid_user_id"))?;
    delete_user_for(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_domains(
    _: Admin,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<DomainView>>> {
    let domains = state
        .db
        .list_all_domains()
        .await
        .map_err(|e| ApiError::db("admin list domains", &e))?;
    Ok(Json(
        domains
            .into_iter()
            .map(|d| DomainView::new(d, &state.opts.base_domain))
            .collect(),
    ))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct AddDomain {
    user_id: i64,
    domain: String,
}

pub(crate) async fn add_domain(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(req): JsonBody<AddDomain>,
) -> ApiResult<Response> {
    let domain = req.domain.trim().to_lowercase();
    if req.user_id == 0 || domain.is_empty() {
        return Err(ApiError::bad_request("user_id and domain required"));
    }
    let user = state
        .db
        .user_by_id(req.user_id)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "user_not_found"))?;
    add_domain_for(&state, user.id, &user.username, &domain).await
}

pub(crate) async fn remove_domain(
    _: Admin,
    State(state): State<AppState>,
    Path(domain): Path<String>,
) -> StatusCode {
    let domain = domain.trim().to_lowercase();
    for u in state.db.list_users().await.unwrap_or_default() {
        if let Ok(Some(name)) = state
            .db
            .remove_user_domain(u.id, &domain, &state.opts.base_domain)
            .await
        {
            sync_txt(&state, &name).await;
        }
    }
    StatusCode::NO_CONTENT
}

pub(crate) async fn list_records(
    _: Admin,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<TxtView>>> {
    let records = state
        .db
        .list_txt_records()
        .await
        .map_err(|e| ApiError::db("admin list records", &e))?;
    Ok(Json(txt_views(records)))
}
