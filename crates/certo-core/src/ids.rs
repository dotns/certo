//! Random and derived identifiers: API keys, subdomains, acme-dns usernames.

use std::fmt::Write as _;

use aws_lc_rs::{digest, rand::SecureRandom};

const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const SUBDOMAIN_LENGTH: usize = 8;
const API_KEY_LENGTH: usize = 32;
const ACMEDNS_USERNAME_LENGTH: usize = 16;

/// Length of the random subdomain allocated by acme-dns `/register`.
pub const ACMEDNS_SUBDOMAIN_LENGTH: usize = 10;

/// Prefix of auto-generated acme-dns account usernames.
pub const ACMEDNS_USERNAME_PREFIX: &str = "acme-";

/// Returns a uniformly random lowercase-alphanumeric string.
pub fn nanoid(length: usize) -> String {
    // Largest multiple of 36 that fits in a byte; larger bytes are rejected to stay uniform.
    const LIMIT: u8 = 252;
    let rng = aws_lc_rs::rand::SystemRandom::new();
    let mut out = String::with_capacity(length);
    let mut buf = [0u8; 64];
    while out.len() < length {
        // The system RNG only fails if the OS entropy source is gone; retry rather than panic.
        if rng.fill(&mut buf).is_err() {
            continue;
        }
        for &b in buf.iter().filter(|&&b| b < LIMIT) {
            if out.len() == length {
                break;
            }
            out.push(char::from(ALPHABET[usize::from(b % 36)]));
        }
    }
    out
}

/// Returns `len` random bytes, or `None` if the OS entropy source fails.
pub fn random_bytes(len: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; len];
    aws_lc_rs::rand::SystemRandom::new().fill(&mut buf).ok()?;
    Some(buf)
}

/// Returns a new random API key.
pub fn api_key() -> String {
    nanoid(API_KEY_LENGTH)
}

/// Returns a new random `acme-<nanoid>` username.
pub fn acmedns_username() -> String {
    format!(
        "{ACMEDNS_USERNAME_PREFIX}{}",
        nanoid(ACMEDNS_USERNAME_LENGTH)
    )
}

/// Deterministic subdomain for a user's domain; bump `salt` to resolve collisions.
pub fn subdomain(username: &str, domain: &str, salt: usize) -> String {
    let mut input = format!("{}:{}", username.to_lowercase(), domain.to_lowercase());
    if salt > 0 {
        input.push(':');
        input.push_str(&"x".repeat(salt));
    }
    let hash = digest::digest(&digest::SHA256, input.as_bytes());
    hash.as_ref().iter().take(SUBDOMAIN_LENGTH / 2).fold(
        String::with_capacity(SUBDOMAIN_LENGTH),
        |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nanoid_has_requested_length_and_alphabet() {
        for len in [0, 1, 10, 32, 100] {
            let id = nanoid(len);
            assert_eq!(id.len(), len);
            assert!(id.bytes().all(|b| ALPHABET.contains(&b)), "{id}");
        }
        assert_ne!(api_key(), api_key());
    }

    #[test]
    fn acmedns_username_has_prefix() {
        let name = acmedns_username();
        assert!(name.starts_with("acme-"));
        assert_eq!(name.len(), 5 + ACMEDNS_USERNAME_LENGTH);
    }

    #[test]
    fn subdomain_matches_go_implementation() {
        // First 8 hex chars of sha256("alice:example.com") and sha256("alice:example.com:xx").
        assert_eq!(subdomain("Alice", "Example.COM", 0), "182821e6");
        assert_eq!(subdomain("alice", "example.com", 2), "eae41156");
    }
}
