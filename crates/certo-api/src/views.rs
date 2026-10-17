//! JSON shapes of the public API (Go-compatible field names).
use certo_core::{ApiKey, TxtRecord, User, UserDomain, internal_domain};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Serialize)]
pub(crate) struct UserView {
    id: i64,
    username: String,
    created_at: i64,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            created_at: u.created_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct DomainView {
    domain: String,
    subdomain: String,
    cname_target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
}

impl DomainView {
    pub(crate) fn new(d: UserDomain, base_domain: &str) -> Self {
        Self {
            cname_target: internal_domain(&d.subdomain, base_domain),
            domain: d.domain,
            subdomain: d.subdomain,
            owner: d.owner.filter(|o| !o.is_empty()),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct KeyView {
    id: i64,
    name: String,
    key: String,
    scope: Vec<String>,
    created_at: i64,
}

impl From<ApiKey> for KeyView {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id,
            name: k.name,
            key: k.key,
            scope: k.scope,
            created_at: k.created_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TxtView {
    domain: String,
    value: String,
    last_update: String,
}

impl From<TxtRecord> for TxtView {
    fn from(r: TxtRecord) -> Self {
        let last_update = OffsetDateTime::from_unix_timestamp(r.last_update)
            .ok()
            .and_then(|t| t.format(&Rfc3339).ok())
            .unwrap_or_default();
        Self {
            domain: r.domain,
            value: r.value,
            last_update,
        }
    }
}

pub(crate) fn txt_views(records: Vec<TxtRecord>) -> Vec<TxtView> {
    records.into_iter().map(Into::into).collect()
}
