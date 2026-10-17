//! Validation of hosted zone names and records.
//!
//! Owner names may be relative to the zone (`www`, `@`) or absolute; names inside RDATA
//! (CNAME, NS, PTR, MX, SRV targets) are always absolute, the trailing dot is optional.
use std::{fmt::Write as _, str::FromStr};

use certo_core::ZoneRecord;
use certo_db::NewRecord;
use hickory_server::proto::rr::{Name, RData, RecordType, rdata::TXT};

/// Record types users may create.
pub const SUPPORTED_TYPES: [RecordType; 9] = [
    RecordType::A,
    RecordType::AAAA,
    RecordType::CAA,
    RecordType::CNAME,
    RecordType::MX,
    RecordType::NS,
    RecordType::PTR,
    RecordType::SRV,
    RecordType::TXT,
];

/// TTL used when a record does not specify one.
pub const DEFAULT_TTL: u32 = 3600;

/// Largest TTL allowed by RFC 2181.
pub const MAX_TTL: u32 = 2_147_483_647;

/// Why a zone name or record was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// Not a valid domain name.
    #[error("invalid name: {0}")]
    InvalidName(String),
    /// The owner is not inside the zone.
    #[error("name is outside the zone")]
    OutsideZone,
    /// The owner is inside the server's own domain.
    #[error("name belongs to the server's own domain")]
    Reserved,
    /// The type cannot be managed through the API.
    #[error("unsupported record type {0}")]
    UnsupportedType(String),
    /// RDATA does not parse for the type.
    #[error("invalid record data: {0}")]
    InvalidData(String),
    /// PTR outside `in-addr.arpa` / `ip6.arpa`.
    #[error("PTR records are only allowed in reverse zones")]
    PtrOutsideReverse,
    /// CNAME at the apex (which always has SOA and NS).
    #[error("CNAME is not allowed at the zone apex")]
    CnameAtApex,
    /// CNAME together with other data at one name.
    #[error("a CNAME cannot coexist with other records at the same name")]
    CnameConflict,
    /// TTL above [`MAX_TTL`].
    #[error("ttl must be at most {MAX_TTL}")]
    InvalidTtl,
}

impl RecordError {
    /// Stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidName(_) => "invalid_name",
            Self::OutsideZone => "name_outside_zone",
            Self::Reserved => "name_reserved",
            Self::UnsupportedType(_) => "unsupported_type",
            Self::InvalidData(_) => "invalid_data",
            Self::PtrOutsideReverse => "ptr_outside_reverse_zone",
            Self::CnameAtApex => "cname_at_apex",
            Self::CnameConflict => "cname_conflict",
            Self::InvalidTtl => "invalid_ttl",
        }
    }
}

/// A record as submitted through the API.
#[derive(Debug, Clone, Copy)]
pub struct RecordInput<'a> {
    /// Owner: `@`, relative or absolute.
    pub name: &'a str,
    /// Type mnemonic, case-insensitive.
    pub rtype: &'a str,
    /// TTL, [`DEFAULT_TTL`] when absent.
    pub ttl: Option<u32>,
    /// RDATA in presentation format.
    pub data: &'a str,
}

/// A validated record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidRecord {
    /// Absolute lowercase owner.
    pub name: Name,
    /// Type.
    pub rtype: RecordType,
    /// TTL.
    pub ttl: u32,
    /// Parsed RDATA with absolute names.
    pub data: RData,
}

/// A name as stored: lowercase, no trailing dot.
pub(crate) fn stored_name(name: &Name) -> String {
    let s = name.to_lowercase().to_string();
    s.strip_suffix('.').map_or_else(|| s.clone(), str::to_owned)
}

pub(crate) fn absolute(input: &str) -> Result<Name, RecordError> {
    let mut name = Name::from_str(input.trim())
        .map_err(|e| RecordError::InvalidName(e.to_string()))?
        .to_lowercase();
    name.set_fqdn(true);
    Ok(name)
}

impl ValidRecord {
    /// The storage form.
    pub fn to_new_record(&self) -> NewRecord {
        NewRecord {
            name: stored_name(&self.name),
            rtype: self.rtype.to_string(),
            ttl: self.ttl,
            data: canonical(&self.data),
        }
    }

    /// Re-reads a stored record; `None` if it no longer parses.
    pub fn from_stored(rec: &ZoneRecord) -> Option<Self> {
        let rtype = RecordType::from_str(&rec.rtype).ok()?;
        Some(Self {
            name: absolute(&rec.name).ok()?,
            rtype,
            ttl: rec.ttl,
            data: parse_rdata(rtype, &rec.data).ok()?,
        })
    }
}

/// Parses a zone apex: absolute, lowercase, at least two labels.
pub fn parse_zone_name(input: &str) -> Result<Name, RecordError> {
    let name = absolute(input)?;
    if name.num_labels() < 2 || name.is_wildcard() {
        return Err(RecordError::InvalidName(input.trim().to_owned()));
    }
    Ok(name)
}

/// Whether the zone is under `in-addr.arpa` or `ip6.arpa`.
pub fn is_reverse_zone(zone: &Name) -> bool {
    ["in-addr.arpa.", "ip6.arpa."]
        .iter()
        .filter_map(|s| Name::from_str(s).ok())
        .any(|arpa| arpa.zone_of(zone))
}

/// Resolves an owner given relative to `zone` (`@` and empty mean the apex).
pub fn owner_name(zone: &Name, input: &str) -> Result<Name, RecordError> {
    let input = input.trim();
    let name = if input.is_empty() || input == "@" {
        zone.clone()
    } else if input.ends_with('.') {
        absolute(input)?
    } else {
        let candidate = absolute(input)?;
        if zone.zone_of(&candidate) {
            candidate
        } else {
            Name::from_str(input)
                .map_err(|e| RecordError::InvalidName(e.to_string()))?
                .append_domain(zone)
                .map_err(|e| RecordError::InvalidName(e.to_string()))?
                .to_lowercase()
        }
    };
    if !zone.zone_of(&name) {
        return Err(RecordError::OutsideZone);
    }
    Ok(name)
}

/// Presentation format that parses back to the same RDATA. hickory's TXT `Display`
/// joins the strings unquoted, so TXT is rendered here.
pub(crate) fn canonical(rdata: &RData) -> String {
    let RData::TXT(txt) = rdata else {
        return rdata.to_string();
    };
    let mut out = String::new();
    for (i, part) in txt.txt_data.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        for &b in part {
            match b {
                b'"' | b'\\' => {
                    out.push('\\');
                    out.push(char::from(b));
                }
                0x20..=0x7e => out.push(char::from(b)),
                _ => {
                    let _ = write!(out, "\\{b:03}");
                }
            }
        }
        out.push('"');
    }
    out
}

/// Longest DNS character-string.
const MAX_STRING: usize = 255;

/// Parses RDATA and makes every embedded name absolute. TXT strings longer than one
/// character-string are split into 255-byte pieces.
pub fn parse_rdata(rtype: RecordType, data: &str) -> Result<RData, RecordError> {
    let mut rdata = RData::try_from_str(rtype, data.trim())
        .map_err(|e| RecordError::InvalidData(e.to_string()))?;
    match &mut rdata {
        RData::CNAME(n) => n.0.set_fqdn(true),
        RData::NS(n) => n.0.set_fqdn(true),
        RData::PTR(n) => n.0.set_fqdn(true),
        RData::MX(mx) => mx.exchange.set_fqdn(true),
        RData::SRV(srv) => srv.target.set_fqdn(true),
        RData::TXT(txt) if txt.txt_data.iter().any(|p| p.len() > MAX_STRING) => {
            let parts: Vec<&[u8]> = txt
                .txt_data
                .iter()
                .flat_map(|p| p.chunks(MAX_STRING))
                .collect();
            return Ok(RData::TXT(TXT::from_bytes(parts)));
        }
        _ => {}
    }
    Ok(rdata)
}

/// Validates `input` for `zone`; `reserved` is the server's own domain. CNAME
/// exclusivity at a name is enforced by the store.
pub fn validate(
    zone: &Name,
    reserved: &Name,
    input: RecordInput<'_>,
) -> Result<ValidRecord, RecordError> {
    let rtype = RecordType::from_str(&input.rtype.trim().to_uppercase())
        .ok()
        .filter(|t| SUPPORTED_TYPES.contains(t))
        .ok_or_else(|| RecordError::UnsupportedType(input.rtype.trim().to_owned()))?;
    let name = owner_name(zone, input.name)?;
    if reserved.zone_of(&name) {
        return Err(RecordError::Reserved);
    }
    let ttl = input.ttl.unwrap_or(DEFAULT_TTL);
    if ttl > MAX_TTL {
        return Err(RecordError::InvalidTtl);
    }
    if rtype == RecordType::PTR && !is_reverse_zone(zone) {
        return Err(RecordError::PtrOutsideReverse);
    }
    if rtype == RecordType::CNAME && name == *zone {
        return Err(RecordError::CnameAtApex);
    }
    let data = parse_rdata(rtype, input.data)?;
    Ok(ValidRecord {
        name,
        rtype,
        ttl,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Name {
        Name::from_str(s).unwrap_or_else(|_| Name::root())
    }

    fn input<'a>(name: &'a str, rtype: &'a str, data: &'a str) -> RecordInput<'a> {
        RecordInput {
            name,
            rtype,
            ttl: None,
            data,
        }
    }

    #[test]
    fn owner_names() {
        let zone = n("example.com.");
        for (input, want) in [
            ("@", "example.com."),
            ("", "example.com."),
            ("www", "www.example.com."),
            ("WWW.Example.com", "www.example.com."),
            ("www.example.com.", "www.example.com."),
            ("a.b", "a.b.example.com."),
            ("*", "*.example.com."),
            ("_dmarc", "_dmarc.example.com."),
        ] {
            assert_eq!(owner_name(&zone, input), Ok(n(want)), "{input}");
        }
        assert_eq!(
            owner_name(&zone, "www.example.org."),
            Err(RecordError::OutsideZone)
        );
        assert!(matches!(
            owner_name(&zone, "a..b"),
            Err(RecordError::InvalidName(_))
        ));
    }

    #[test]
    fn zone_names() {
        assert_eq!(parse_zone_name(" Example.COM. "), Ok(n("example.com.")));
        assert!(parse_zone_name("com").is_err());
        assert!(parse_zone_name("*.example.com").is_err());
        assert!(is_reverse_zone(&n("23.251.162.in-addr.arpa.")));
        assert!(is_reverse_zone(&n("8.b.d.0.1.0.0.2.ip6.arpa.")));
        assert!(!is_reverse_zone(&n("example.com.")));
    }

    #[test]
    fn rdata_is_canonical_with_absolute_names() {
        let canon = |t, d| parse_rdata(t, d).map(|r| canonical(&r));
        assert_eq!(
            canon(RecordType::CNAME, "Target.Example.net"),
            Ok("target.example.net.".to_owned())
        );
        assert_eq!(
            canon(RecordType::MX, "10 mail.example.com"),
            Ok("10 mail.example.com.".to_owned())
        );
        assert_eq!(
            canon(RecordType::SRV, "0 5 443 sip.example.com"),
            Ok("0 5 443 sip.example.com.".to_owned())
        );
        assert_eq!(
            canon(RecordType::TXT, "\"v=spf1 -all\""),
            Ok("\"v=spf1 -all\"".to_owned())
        );
        assert_eq!(
            canon(RecordType::TXT, "v=spf1 -all"),
            Ok("\"v=spf1\" \"-all\"".to_owned())
        );
        let tricky = r#""say \"hi\" \\ ok" "two""#;
        let parsed = parse_rdata(RecordType::TXT, tricky).unwrap();
        assert_eq!(
            parse_rdata(RecordType::TXT, &canonical(&parsed)),
            Ok(parsed)
        );
        let long = format!("\"{}\"", "k".repeat(600));
        let RData::TXT(split) = parse_rdata(RecordType::TXT, &long).unwrap() else {
            panic!("not TXT");
        };
        assert_eq!(
            split.txt_data.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [255, 255, 90]
        );
        assert!(canon(RecordType::A, "300.1.1.1").is_err());
        assert!(canon(RecordType::AAAA, "192.0.2.1").is_err());
    }

    #[test]
    fn record_rules() {
        let zone = n("example.com.");
        let rev = n("2.0.192.in-addr.arpa.");
        let reserved = n("auth.example.com.");
        let ok = |z: &Name, i| validate(z, &reserved, i);

        let a = ok(&zone, input("www", "a", "192.0.2.1")).unwrap();
        assert_eq!((a.rtype, a.ttl), (RecordType::A, DEFAULT_TTL));
        assert_eq!(a.to_new_record().name, "www.example.com");
        assert_eq!(a.to_new_record().rtype, "A");

        assert_eq!(
            ok(&zone, input("x", "SOA", "a. b. 1 2 3 4 5")),
            Err(RecordError::UnsupportedType("SOA".into()))
        );
        assert_eq!(
            ok(&zone, input("x.auth", "A", "192.0.2.1")),
            Err(RecordError::Reserved)
        );
        assert_eq!(
            ok(&zone, input("1", "PTR", "host.example.com")),
            Err(RecordError::PtrOutsideReverse)
        );
        assert!(ok(&rev, input("1", "PTR", "host.example.com")).is_ok());
        assert_eq!(
            ok(&zone, input("@", "CNAME", "other.net")),
            Err(RecordError::CnameAtApex)
        );
        assert_eq!(
            validate(
                &zone,
                &reserved,
                RecordInput {
                    ttl: Some(MAX_TTL + 1),
                    ..input("www", "A", "192.0.2.1")
                },
            ),
            Err(RecordError::InvalidTtl)
        );
    }
}
