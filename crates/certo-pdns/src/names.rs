//! Validation of zone names and rrset changes before they reach `PowerDNS`.

use crate::{ChangeType, RecordData, RrSet, ZoneOpError};

/// Record types users may manage.
pub const SUPPORTED_TYPES: [&str; 9] =
    ["A", "AAAA", "CAA", "CNAME", "MX", "NS", "PTR", "SRV", "TXT"];

/// TTL used when a change does not specify one.
pub const DEFAULT_TTL: u32 = 3600;

/// Largest TTL allowed by RFC 2181.
pub const MAX_TTL: u32 = 2_147_483_647;

/// An rrset change as submitted through the API.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct RrSetChange {
    /// Owner: `@`, relative to the zone, or absolute.
    pub name: String,
    /// Type mnemonic, case-insensitive.
    #[serde(rename = "type")]
    pub rtype: String,
    /// TTL, [`DEFAULT_TTL`] when absent.
    pub ttl: Option<u32>,
    /// `REPLACE` (default) or `DELETE`.
    pub changetype: Option<ChangeType>,
    /// Record contents in presentation format.
    pub records: Vec<String>,
}

fn invalid(code: &'static str, message: impl Into<String>) -> ZoneOpError {
    ZoneOpError::Invalid {
        code,
        message: message.into(),
    }
}

fn valid_label(label: &str, allow_wildcard: bool) -> bool {
    if label.is_empty() || label.len() > 63 {
        return false;
    }
    if label == "*" {
        return allow_wildcard;
    }
    label
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

/// Lowercases and validates an absolute or relative DNS name; the result has no trailing dot.
fn normalize(input: &str, allow_wildcard: bool) -> Result<String, ZoneOpError> {
    let name = input.trim().to_ascii_lowercase();
    let name = name.strip_suffix('.').unwrap_or(&name);
    if name.is_empty() || name.len() > 253 {
        return Err(invalid("invalid_name", format!("invalid name {input:?}")));
    }
    for (i, label) in name.split('.').enumerate() {
        if !valid_label(label, allow_wildcard && i == 0) {
            return Err(invalid("invalid_name", format!("invalid name {input:?}")));
        }
    }
    Ok(name.to_owned())
}

/// Canonical zone name: lowercase, no trailing dot, at least two labels.
pub fn parse_zone_name(input: &str) -> Result<String, ZoneOpError> {
    let name = normalize(input, false)?;
    if name.split('.').count() < 2 {
        return Err(invalid("invalid_name", "a zone needs at least two labels"));
    }
    Ok(name)
}

/// A TSIG key name: a valid lowercase DNS name without trailing dot.
pub fn parse_key_name(input: &str) -> Result<String, ZoneOpError> {
    normalize(input, false)
        .map_err(|_| invalid("invalid_name", format!("invalid key name {input:?}")))
}

/// Whether `zone` is under `in-addr.arpa` or `ip6.arpa`.
pub fn is_reverse_zone(zone: &str) -> bool {
    zone.ends_with(".in-addr.arpa") || zone.ends_with(".ip6.arpa")
}

/// Whether `name` is `zone` or below it (both without trailing dots).
pub fn inside(name: &str, zone: &str) -> bool {
    name == zone || name.ends_with(&format!(".{zone}"))
}

/// Resolves an owner (`@`, relative or absolute) to an absolute lowercase name inside the
/// zone, without trailing dot.
pub fn owner_name(zone: &str, input: &str) -> Result<String, ZoneOpError> {
    let input = input.trim();
    if input.is_empty() || input == "@" {
        return Ok(zone.to_owned());
    }
    let name = normalize(input, true)?;
    let absolute = input.ends_with('.') || inside(&name, zone);
    let name = if absolute {
        name
    } else {
        format!("{name}.{zone}")
    };
    if !inside(&name, zone) {
        return Err(invalid(
            "name_outside_zone",
            format!("{name} is outside {zone}"),
        ));
    }
    Ok(name)
}

/// Turns API changes into validated `PowerDNS` rrsets for `zone`.
pub fn validate_changes(zone: &str, changes: &[RrSetChange]) -> Result<Vec<RrSet>, ZoneOpError> {
    if changes.is_empty() {
        return Err(invalid("empty_change", "no rrsets given"));
    }
    let mut out = Vec::with_capacity(changes.len());
    for change in changes {
        let name = owner_name(zone, &change.name)?;
        let rtype = change.rtype.trim().to_ascii_uppercase();
        if !SUPPORTED_TYPES.contains(&rtype.as_str()) {
            return Err(invalid(
                "unsupported_type",
                format!("unsupported record type {:?}", change.rtype),
            ));
        }
        let ttl = change.ttl.unwrap_or(DEFAULT_TTL);
        if ttl > MAX_TTL {
            return Err(invalid(
                "invalid_ttl",
                format!("ttl must be at most {MAX_TTL}"),
            ));
        }
        let changetype = change.changetype.unwrap_or(ChangeType::Replace);
        let records: Vec<RecordData> = change
            .records
            .iter()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .map(|c| {
                if rtype == "TXT" && !c.starts_with('"') {
                    RecordData::new(txt_content(c))
                } else {
                    RecordData::new(c)
                }
            })
            .collect();
        if changetype == ChangeType::Replace {
            if records.is_empty() {
                return Err(invalid(
                    "invalid_data",
                    "a REPLACE needs at least one record",
                ));
            }
            if rtype == "CNAME" {
                if name == zone {
                    return Err(invalid(
                        "cname_at_apex",
                        "CNAME is not allowed at the zone apex",
                    ));
                }
                if records.len() > 1 {
                    return Err(invalid("invalid_data", "a name can carry only one CNAME"));
                }
            }
            if rtype == "PTR" && !is_reverse_zone(zone) {
                return Err(invalid(
                    "ptr_outside_reverse_zone",
                    "PTR records are only allowed in reverse zones",
                ));
            }
        }
        out.push(RrSet {
            name: format!("{name}."),
            rtype,
            ttl,
            changetype: Some(changetype),
            records: if changetype == ChangeType::Replace {
                records
            } else {
                Vec::new()
            },
        });
    }
    Ok(out)
}

/// Quotes a TXT value the way `PowerDNS` expects it in rrset content.
pub fn txt_content(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
#[allow(clippy::assert_is_empty, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn zone_names() {
        assert_eq!(parse_zone_name(" Example.COM. ").unwrap(), "example.com");
        assert_eq!(
            parse_zone_name("23.251.162.in-addr.arpa").unwrap(),
            "23.251.162.in-addr.arpa"
        );
        for bad in ["com", "", "-a.com", "a..com", "a b.com", "*.example.com"] {
            assert!(parse_zone_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn owners() {
        let z = "example.com";
        assert_eq!(owner_name(z, "@").unwrap(), z);
        assert_eq!(owner_name(z, "").unwrap(), z);
        assert_eq!(owner_name(z, "www").unwrap(), "www.example.com");
        assert_eq!(
            owner_name(z, "WWW.Example.com.").unwrap(),
            "www.example.com"
        );
        assert_eq!(owner_name(z, "www.example.com").unwrap(), "www.example.com");
        assert_eq!(owner_name(z, "*.mail").unwrap(), "*.mail.example.com");
        assert_eq!(
            owner_name(z, "_acme-challenge").unwrap(),
            "_acme-challenge.example.com"
        );
        assert!(matches!(
            owner_name(z, "www.example.org."),
            Err(ZoneOpError::Invalid {
                code: "name_outside_zone",
                ..
            })
        ));
        assert!(owner_name(z, "a.*.b").is_err());
    }

    fn change(name: &str, rtype: &str, records: &[&str]) -> RrSetChange {
        RrSetChange {
            name: name.into(),
            rtype: rtype.into(),
            ttl: None,
            changetype: None,
            records: records.iter().map(|r| (*r).to_owned()).collect(),
        }
    }

    #[test]
    fn changes_become_canonical_rrsets() {
        let out = validate_changes(
            "example.com",
            &[change("www", "a", &["192.0.2.1", " 192.0.2.2 "])],
        )
        .unwrap();
        assert_eq!(out[0].name, "www.example.com.");
        assert_eq!(out[0].rtype, "A");
        assert_eq!(out[0].ttl, DEFAULT_TTL);
        assert_eq!(out[0].changetype, Some(ChangeType::Replace));
        assert_eq!(out[0].records.len(), 2);

        let del = validate_changes(
            "example.com",
            &[RrSetChange {
                changetype: Some(ChangeType::Delete),
                ..change("www", "A", &["x"])
            }],
        )
        .unwrap();
        assert!(del[0].records.is_empty());

        let txt = validate_changes(
            "example.com",
            &[change("@", "TXT", &["hello world", "\"already quoted\""])],
        )
        .unwrap();
        assert_eq!(txt[0].records[0].content, "\"hello world\"");
        assert_eq!(txt[0].records[1].content, "\"already quoted\"");
    }

    #[test]
    fn changes_are_rejected() {
        let code = |changes: &[RrSetChange]| match validate_changes("example.com", changes) {
            Err(ZoneOpError::Invalid { code, .. }) => code,
            other => panic!("{other:?}"),
        };
        assert_eq!(code(&[]), "empty_change");
        assert_eq!(code(&[change("www", "SOA", &["x"])]), "unsupported_type");
        assert_eq!(code(&[change("www", "A", &[])]), "invalid_data");
        assert_eq!(
            code(&[change("@", "CNAME", &["a.example.net."])]),
            "cname_at_apex"
        );
        assert_eq!(
            code(&[change("www", "CNAME", &["a.", "b."])]),
            "invalid_data"
        );
        assert_eq!(
            code(&[change("5", "PTR", &["host.example.net."])]),
            "ptr_outside_reverse_zone"
        );
        assert_eq!(
            code(&[RrSetChange {
                ttl: Some(MAX_TTL + 1),
                ..change("www", "A", &["x"])
            }]),
            "invalid_ttl"
        );
        assert_eq!(
            code(&[change("www.other.org.", "A", &["x"])]),
            "name_outside_zone"
        );
        assert!(
            validate_changes(
                "2.0.192.in-addr.arpa",
                &[change("5", "PTR", &["host.example.net."])]
            )
            .is_ok()
        );
    }

    #[test]
    fn txt_quoting() {
        assert_eq!(txt_content("abc"), "\"abc\"");
        assert_eq!(txt_content("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
