//! Shared configuration and domain types for certo.
#![forbid(unsafe_code)]

pub mod config;
pub mod ids;
mod types;

pub use types::{
    ACMEDNS_TXT_LENGTH, ApiKey, MAX_TXT_VALUE_LENGTH, TxtRecord, User, UserDomain, Zone,
    extract_domain_from_fqdn, extract_subdomain_from_fqdn, internal_domain, sanitize_fqdn,
};

/// Release version: `$CERTO_VERSION` at build time (set from the git tag), else the
/// crate version.
pub const VERSION: &str = match option_env!("CERTO_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => env!("CARGO_PKG_VERSION"),
};
