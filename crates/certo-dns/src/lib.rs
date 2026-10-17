//! Authoritative DNS serving for certo.
#![forbid(unsafe_code)]

mod catalog;
mod challenge;
mod handler;
mod records;
mod server;
mod zone;
mod zones;

pub use catalog::DynamicCatalog;
pub use challenge::{ChallengeConfig, ChallengeResponder};
pub use handler::DnsHandler;
pub use records::{
    DEFAULT_TTL, MAX_TTL, RecordError, RecordInput, SUPPORTED_TYPES, ValidRecord, is_reverse_zone,
    owner_name, parse_rdata, parse_zone_name, validate,
};
pub use server::{ListenError, listen};
pub use zone::{AxfrAccess, DnssecConfig, ZoneConfig, ZoneError, build_zone};
pub use zones::{
    DnssecInfo, NSEC_MODES, RESIGN_INTERVAL, TSIG_ALGORITHMS, TransferInfo, TsigKeyInfo,
    ZoneOpError, ZonePolicy, Zones,
};
