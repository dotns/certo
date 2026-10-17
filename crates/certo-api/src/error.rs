use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use certo_db::DbError;

/// A JSON `{"error": code}` response.
#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    realm: Option<&'static str>,
    message: Option<String>,
}

impl ApiError {
    pub(crate) const fn new(status: StatusCode, code: &'static str) -> Self {
        Self {
            status,
            code,
            realm: None,
            message: None,
        }
    }

    /// Adds a human-readable `message` next to the code.
    pub(crate) fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub(crate) const fn bad_request(code: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }

    pub(crate) const fn forbidden(code: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code)
    }

    pub(crate) const fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized")
    }

    /// 401 with a `WWW-Authenticate: Basic` challenge for `realm`.
    pub(crate) const fn basic(realm: &'static str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            realm: Some(realm),
            message: None,
        }
    }

    pub(crate) const fn internal(code: &'static str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, code)
    }

    /// Logs a storage failure and maps it to `500 db_error`.
    pub(crate) fn db(context: &str, err: &DbError) -> Self {
        tracing::error!(error = %err, "{context}: database error");
        Self::internal("db_error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = match self.message {
            Some(message) => serde_json::json!({ "error": self.code, "message": message }),
            None => serde_json::json!({ "error": self.code }),
        };
        let mut resp = (self.status, Json(body)).into_response();
        if let Some(realm) = self.realm
            && let Ok(v) = HeaderValue::from_str(&format!("Basic realm=\"{realm}\""))
        {
            resp.headers_mut().insert(header::WWW_AUTHENTICATE, v);
        }
        resp
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;
