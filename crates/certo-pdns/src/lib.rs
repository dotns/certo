//! pdns (`PowerDNS` Authoritative) as certo's data plane: process supervision, the HTTP API
//! client and the zone service built on both.
#![forbid(unsafe_code)]

mod client;
mod config;
mod delegation;
#[cfg(feature = "mock")]
pub mod mock;
pub mod names;
mod supervisor;
mod zones;

pub use client::{
    ChangeType, Client, CryptoKey, Metadata, NewZone, RecordData, RrSet, TsigKey, Zone, ZoneSummary,
};
pub use config::{PdnsConfig, SCHEMA};
pub use delegation::{Delegation, DelegationChecker, DnsDelegationChecker};
pub use names::{RrSetChange, parse_zone_name};
pub use supervisor::Supervisor;
pub use zones::{
    ACTIVE_RECHECK_INTERVAL, CheckReport, DnssecInfo, MAX_FAILED_CHECKS, NSEC_MODES,
    PENDING_CHECK_INTERVAL, PENDING_EXPIRY, TSIG_ALGORITHMS, TransferInfo, TsigKeyInfo,
    VERIFY_COOLDOWN, ZoneOpError, ZonePolicy, Zones,
};

/// Errors from the supervisor and the API client.
#[derive(Debug, thiserror::Error)]
pub enum PdnsError {
    /// Invalid settings.
    #[error("pdns configuration: {0}")]
    Config(String),
    /// File or process error.
    #[error("pdns {context}: {source}")]
    Io {
        /// What was being done.
        context: &'static str,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Creating the pdns SQLite schema failed.
    #[error("pdns schema: {0}")]
    Schema(#[from] rusqlite::Error),
    /// The process exited or did not answer the API in time.
    #[error("pdns_server did not become ready: {0}")]
    NotReady(String),
    /// Transport failure talking to the API.
    #[error("pdns API request failed: {0}")]
    Http(#[from] reqwest::Error),
    /// The API refused the request.
    #[error("pdns API {status}: {message}")]
    Api {
        /// HTTP status.
        status: u16,
        /// Error text from the API.
        message: String,
    },
}

impl PdnsError {
    fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    /// Whether the API answered 404.
    pub const fn is_not_found(&self) -> bool {
        matches!(self, Self::Api { status: 404, .. })
    }
}
