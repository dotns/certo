use secrecy::SecretSlice;

/// A registered account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Row id.
    pub id: i64,
    /// Login name.
    pub username: String,
    /// bcrypt hash, or `!` for accounts that cannot log in.
    pub password_hash: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// A domain owned by a user and its internal subdomain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDomain {
    /// The user's real domain, lowercase.
    pub domain: String,
    /// Internal subdomain label under the base domain.
    pub subdomain: String,
    /// Owner's username (only set by admin listings).
    pub owner: Option<String>,
}

/// An API key and the domains it may act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    /// Row id.
    pub id: i64,
    /// Owning user.
    pub user_id: i64,
    /// Display name.
    pub name: String,
    /// Secret key value.
    pub key: String,
    /// `*`, `*.example.com` or exact domains.
    pub scope: Vec<String>,
    /// Unix seconds.
    pub created_at: i64,
}

impl ApiKey {
    /// Whether the key has the global `*` scope.
    pub fn is_global(&self) -> bool {
        self.scope.iter().any(|s| s == "*")
    }

    /// Whether the key may act on `domain`; `*.example.com` covers the apex and all subdomains.
    pub fn has_domain_access(&self, domain: &str) -> bool {
        if self.is_global() {
            return true;
        }
        let domain = domain.trim().to_lowercase();
        self.scope.iter().any(|s| {
            let s = s.trim().to_lowercase();
            if s == domain {
                return true;
            }
            s.strip_prefix("*.").is_some_and(|root| {
                domain == root
                    || domain
                        .strip_suffix(root)
                        .is_some_and(|rest| rest.ends_with('.'))
            })
        })
    }
}

/// A stored challenge TXT value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxtRecord {
    /// Owner name, lowercase without trailing dot.
    pub domain: String,
    /// TXT value.
    pub value: String,
    /// Unix seconds of the last write.
    pub last_update: i64,
}

/// Lowercases, trims and drops the trailing dot of a domain name.
pub fn sanitize_fqdn(fqdn: &str) -> String {
    let fqdn = fqdn.trim().to_lowercase();
    fqdn.strip_suffix('.')
        .map_or_else(|| fqdn.clone(), str::to_owned)
}

/// An authoritative zone hosted for a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    /// Row id.
    pub id: i64,
    /// Owning user.
    pub user_id: i64,
    /// Owner's username (set by queries that join it).
    pub owner: Option<String>,
    /// Zone apex, lowercase without trailing dot.
    pub name: String,
    /// SOA serial; bumped on every change.
    pub serial: u32,
    /// Whether the zone is DNSSEC-signed.
    pub dnssec: bool,
    /// `nsec` or `nsec3`.
    pub nsec_mode: String,
    /// CIDRs allowed to transfer the zone without TSIG.
    pub axfr_allow_from: Vec<String>,
    /// Unix seconds.
    pub created_at: i64,
}

/// A resource record of a [`Zone`], in presentation format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneRecord {
    /// Row id.
    pub id: i64,
    /// Owner name, lowercase without trailing dot.
    pub name: String,
    /// Record type mnemonic, e.g. `AAAA`.
    pub rtype: String,
    /// TTL in seconds.
    pub ttl: u32,
    /// RDATA in canonical presentation format.
    pub data: String,
}

/// A zone's DNSSEC signing key.
#[derive(Debug)]
pub struct ZoneKey {
    /// Algorithm mnemonic, e.g. `ECDSAP256SHA256`.
    pub algorithm: String,
    /// PKCS#8 private key.
    pub private_key: SecretSlice<u8>,
}

/// A TSIG key allowed to transfer a zone.
#[derive(Debug)]
pub struct TsigKey {
    /// Key name, lowercase without trailing dot.
    pub name: String,
    /// Algorithm, e.g. `hmac-sha256`.
    pub algorithm: String,
    /// Shared secret.
    pub secret: SecretSlice<u8>,
    /// Unix seconds.
    pub created_at: i64,
}

/// Exact length acme-dns requires for an `/update` TXT value.
pub const ACMEDNS_TXT_LENGTH: usize = 43;

/// Longest storable TXT value: one DNS character-string.
pub const MAX_TXT_VALUE_LENGTH: usize = 255;

/// The internal challenge name `<subdomain>.<base_domain>`.
pub fn internal_domain(subdomain: &str, base_domain: &str) -> String {
    let base = base_domain.trim();
    format!("{subdomain}.{}", base.strip_suffix('.').unwrap_or(base))
}

/// The user's domain behind a challenge FQDN: `_acme-challenge.example.com.` → `example.com`.
pub fn extract_domain_from_fqdn(fqdn: &str) -> String {
    let fqdn = sanitize_fqdn(fqdn);
    fqdn.strip_prefix("_acme-challenge.")
        .map_or_else(|| fqdn.clone(), str::to_owned)
}

/// The subdomain label when `fqdn` is exactly one label under `base_domain`.
pub fn extract_subdomain_from_fqdn(fqdn: &str, base_domain: &str) -> Option<String> {
    let fqdn = sanitize_fqdn(fqdn);
    let suffix = format!(".{}", sanitize_fqdn(base_domain));
    fqdn.strip_suffix(&suffix)
        .filter(|sub| !sub.is_empty() && !sub.contains('.'))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(scope: &[&str]) -> ApiKey {
        ApiKey {
            id: 1,
            user_id: 1,
            name: "k".into(),
            key: "v".into(),
            scope: scope.iter().map(|s| (*s).to_owned()).collect(),
            created_at: 0,
        }
    }

    #[test]
    fn scope_matching() {
        assert!(key(&["*"]).has_domain_access("anything.org"));
        let exact = key(&["example.com"]);
        assert!(exact.has_domain_access(" Example.COM "));
        assert!(!exact.has_domain_access("a.example.com"));
        let wild = key(&["*.example.com"]);
        assert!(wild.has_domain_access("example.com"));
        assert!(wild.has_domain_access("a.b.example.com"));
        assert!(!wild.has_domain_access("badexample.com"));
        assert!(!wild.has_domain_access("example.org"));
        assert!(!key(&[]).has_domain_access("example.com"));
    }

    #[test]
    fn fqdn_sanitizing() {
        assert_eq!(
            sanitize_fqdn(" _ACME-challenge.Example.com. "),
            "_acme-challenge.example.com"
        );
        assert_eq!(sanitize_fqdn("example.com"), "example.com");
    }

    #[test]
    fn challenge_name_helpers() {
        assert_eq!(
            internal_domain("abc", " s.example.com. "),
            "abc.s.example.com"
        );
        assert_eq!(
            extract_domain_from_fqdn("_acme-challenge.Example.com."),
            "example.com"
        );
        assert_eq!(extract_domain_from_fqdn("example.com"), "example.com");
        let base = "s.example.com.";
        assert_eq!(
            extract_subdomain_from_fqdn("R0hc4bc6.s.example.com.", base).as_deref(),
            Some("r0hc4bc6")
        );
        assert_eq!(extract_subdomain_from_fqdn("a.b.s.example.com", base), None);
        assert_eq!(extract_subdomain_from_fqdn("s.example.com", base), None);
        assert_eq!(extract_subdomain_from_fqdn("x.example.com", base), None);
    }
}
