use std::{fmt, sync::Arc};

use axum::http::HeaderName;
use certo_core::{config::Config, ids};
use certo_db::Db;
use certo_pdns::Zones;
use jsonwebtoken::{DecodingKey, EncodingKey};
use secrecy::{ExposeSecret, SecretString};

/// bcrypt cost used for new password hashes (Go parity).
pub(crate) const BCRYPT_COST: u32 = 10;

/// Settings the HTTP API needs, extracted from the server config.
pub struct ApiOptions {
    /// `general.domain`: base domain for internal challenge names.
    pub base_domain: String,
    /// `api.api_domain`, falling back to the base domain.
    pub api_domain: String,
    /// HMAC secret for dashboard JWTs; random per process when unset.
    pub jwt_secret: SecretString,
    /// `X-Admin-Key` value; admin endpoints are disabled when unset.
    pub admin_key: Option<SecretString>,
    /// Whether open registration endpoints are enabled.
    pub allow_registration: bool,
    /// Allowed CORS origins; empty allows all.
    pub cors_origins: Vec<String>,
    /// Trusted header carrying the client IP.
    pub client_ip_header: Option<HeaderName>,
    /// Whether the listener terminates TLS (for `server_url`).
    pub tls: bool,
}

impl fmt::Debug for ApiOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiOptions")
            .field("base_domain", &self.base_domain)
            .field("api_domain", &self.api_domain)
            .field("admin_key", &self.admin_key.is_some())
            .field("allow_registration", &self.allow_registration)
            .field("cors_origins", &self.cors_origins)
            .field("client_ip_header", &self.client_ip_header)
            .field("tls", &self.tls)
            .finish_non_exhaustive()
    }
}

impl ApiOptions {
    /// Derives the API settings from the server config.
    pub fn from_config(cfg: &Config) -> Self {
        let api = &cfg.api;
        let jwt_secret = if api.jwt_secret.is_empty() {
            tracing::warn!(
                "api.jwt_secret is empty; using a random secret, sessions end on restart"
            );
            ids::nanoid(64)
        } else {
            api.jwt_secret.clone()
        };
        let client_ip_header = if api.use_header && !api.header_name.is_empty() {
            let parsed = HeaderName::try_from(api.header_name.as_str()).ok();
            if parsed.is_none() {
                tracing::warn!(header = %api.header_name, "ignoring invalid api.header_name");
            }
            parsed
        } else {
            None
        };
        Self {
            base_domain: cfg.general.domain.clone(),
            api_domain: if api.api_domain.is_empty() {
                cfg.general.domain.clone()
            } else {
                api.api_domain.clone()
            },
            jwt_secret: jwt_secret.into(),
            admin_key: (!api.admin_key.is_empty()).then(|| api.admin_key.clone().into()),
            allow_registration: api.registration_allowed(),
            cors_origins: api.corsorigins.clone(),
            client_ip_header,
            tls: !matches!(api.tls, certo_core::config::TlsMode::None),
        }
    }
}

/// Shared handler state; cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub(crate) db: Db,
    pub(crate) zones: Zones,
    pub(crate) opts: Arc<ApiOptions>,
    pub(crate) jwt: Arc<(EncodingKey, DecodingKey)>,
    /// Valid bcrypt hash compared against on unknown logins to equalize timing.
    pub(crate) dummy_hash: Arc<str>,
}

impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState")
            .field("opts", &self.opts)
            .finish_non_exhaustive()
    }
}

impl AppState {
    /// Builds the state; hashes a dummy password once, so call it off hot paths.
    pub fn new(db: Db, zones: Zones, opts: ApiOptions) -> Result<Self, bcrypt::BcryptError> {
        let secret = opts.jwt_secret.expose_secret().as_bytes();
        let jwt = (
            EncodingKey::from_secret(secret),
            DecodingKey::from_secret(secret),
        );
        let dummy_hash = bcrypt::hash(ids::api_key(), BCRYPT_COST)?;
        Ok(Self {
            db,
            zones,
            opts: Arc::new(opts),
            jwt: Arc::new(jwt),
            dummy_hash: dummy_hash.into(),
        })
    }
}
