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

/// An authoritative zone served by `PowerDNS` and owned by a user (or by the server itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    /// Row id.
    pub id: i64,
    /// Owning user; `None` for the server's own base zone.
    pub user_id: Option<i64>,
    /// Owner's username (set by queries that join it).
    pub owner: Option<String>,
    /// Zone apex, lowercase without trailing dot.
    pub name: String,
    /// Served by `PowerDNS` (delegation verified or exempt).
    pub active: bool,
    /// Activated by an administrator without a delegation check; never re-checked.
    pub exempt: bool,
    /// Unix seconds of the last successful delegation check.
    pub verified_at: Option<i64>,
    /// Unix seconds of the last delegation check.
    pub last_check: Option<i64>,
    /// Why the last check failed.
    pub last_error: Option<String>,
    /// Consecutive failed checks since the last success.
    pub failed_checks: u32,
    /// Unix seconds.
    pub created_at: i64,
}

impl Zone {
    /// Whether this is the server's own base zone.
    pub const fn is_system(&self) -> bool {
        self.user_id.is_none()
    }
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
