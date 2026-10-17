//! Automatic certificates for the API domain over ACME DNS-01.
//!
//! The challenge TXT is served by certo's own DNS server through a hook, so no external
//! DNS provider is involved. Certificates use certmagic's storage layout, which lets a
//! cache written by the Go server be reused as is.
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum_server::tls_rustls::RustlsConfig;
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, NewAccount,
    NewOrder, OrderStatus, RetryPolicy,
};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};

use crate::tls::finish;

/// Let's Encrypt production directory.
pub const LETS_ENCRYPT: &str = "https://acme-v02.api.letsencrypt.org/directory";
/// Let's Encrypt staging directory.
pub const LETS_ENCRYPT_STAGING: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 3600);
const MIN_RETRY: Duration = Duration::from_secs(60);
const MAX_RETRY: Duration = Duration::from_secs(3600);
const ORDER_POLL: RetryPolicy = RetryPolicy::new().timeout(Duration::from_secs(180));

/// Publishes (`Some`) or withdraws (`None`) the DNS-01 TXT value for the API domain.
pub type ChallengeHook = Arc<dyn Fn(Option<&str>) + Send + Sync>;

/// ACME settings for the API certificate.
#[derive(Debug, Clone)]
pub struct AcmeOptions {
    /// Name to certify (`general.domain`).
    pub domain: String,
    /// ACME directory URL.
    pub directory: String,
    /// Optional account contact address.
    pub email: String,
    /// Account and certificate cache directory.
    pub cache_dir: PathBuf,
    /// Extra PEM root to trust for the directory (private CAs, tests).
    pub root_ca: Option<PathBuf>,
}

/// ACME failures.
#[derive(Debug, thiserror::Error)]
pub enum AcmeError {
    /// Protocol or transport failure.
    #[error("acme: {0}")]
    Acme(#[from] instant_acme::Error),
    /// Cache read/write failure.
    #[error("acme cache {path}: {source}")]
    Io {
        /// File involved.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// Malformed certificate, key or account data.
    #[error("acme: {0}")]
    Invalid(String),
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> AcmeError + '_ {
    move |source| AcmeError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// certmagic's storage key sanitizer.
fn safe(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .replace(' ', "_")
        .replace('+', "_plus_")
        .replace('*', "wildcard_")
        .replace(':', "-")
        .replace("..", "")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '@' | '.' | '-'))
        .collect()
}

/// certmagic's issuer key: `host[-path]`, e.g. `acme-v02.api.letsencrypt.org-directory`.
fn issuer_key(directory: &str) -> String {
    let rest = directory.split_once("://").map_or(directory, |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.replace(['/', '\\'], "-");
    let path = path.trim_matches('-');
    if path.is_empty() {
        host.to_owned()
    } else {
        format!("{host}-{path}")
    }
}

struct Paths {
    cert: PathBuf,
    key: PathBuf,
    account: PathBuf,
}

impl Paths {
    fn new(opts: &AcmeOptions) -> Self {
        let issuer = safe(&issuer_key(&opts.directory));
        let domain = safe(&opts.domain);
        let site = opts
            .cache_dir
            .join("certificates")
            .join(&issuer)
            .join(&domain);
        Self {
            cert: site.join(format!("{domain}.crt")),
            key: site.join(format!("{domain}.key")),
            account: opts
                .cache_dir
                .join("acme")
                .join(&issuer)
                .join("certo-account.json"),
        }
    }
}

/// A loaded certificate and its validity window (unix seconds).
struct Loaded {
    key: Arc<CertifiedKey>,
    not_before: i64,
    not_after: i64,
}

impl Loaded {
    fn parse(chain_pem: &[u8], key_pem: &[u8]) -> Result<Self, AcmeError> {
        let invalid = |e: &dyn fmt::Display| AcmeError::Invalid(e.to_string());
        let chain = CertificateDer::pem_slice_iter(chain_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid(&e))?;
        let leaf = chain
            .first()
            .ok_or_else(|| AcmeError::Invalid("empty certificate chain".into()))?;
        let (_, cert) = x509_parser::parse_x509_certificate(leaf).map_err(|e| invalid(&e))?;
        let validity = cert.validity();
        let (not_before, not_after) = (
            validity.not_before.timestamp(),
            validity.not_after.timestamp(),
        );
        let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|e| invalid(&e))?;
        let signer =
            rustls::crypto::aws_lc_rs::sign::any_supported_type(&key).map_err(|e| invalid(&e))?;
        Ok(Self {
            key: Arc::new(CertifiedKey::new(chain, signer)),
            not_before,
            not_after,
        })
    }

    const fn needs_renewal(&self, now: i64) -> bool {
        renewal_due(self.not_before, self.not_after, now)
    }
}

/// Renew once less than a third of the lifetime remains (certmagic's default).
const fn renewal_due(not_before: i64, not_after: i64, now: i64) -> bool {
    now >= not_after - (not_after - not_before) / 3
}

/// Serves whatever certificate the manager last installed.
#[derive(Default)]
struct Resolver(RwLock<Option<Arc<CertifiedKey>>>);

impl fmt::Debug for Resolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Resolver")
    }
}

impl Resolver {
    fn set(&self, key: Arc<CertifiedKey>) {
        match self.0.write() {
            Ok(mut g) => *g = Some(key),
            Err(e) => *e.into_inner() = Some(key),
        }
    }
}

impl ResolvesServerCert for Resolver {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        match self.0.read() {
            Ok(g) => g.clone(),
            Err(e) => e.into_inner().clone(),
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Writes through a temp file so readers never see partial content.
async fn write_atomic(path: &Path, data: &[u8]) -> Result<(), AcmeError> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(io_err(dir))?;
    }
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, data).await.map_err(io_err(&tmp))?;
    tokio::fs::rename(&tmp, path).await.map_err(io_err(path))
}

struct Manager {
    opts: AcmeOptions,
    paths: Paths,
    resolver: Arc<Resolver>,
    hook: ChallengeHook,
}

impl Manager {
    async fn load_cached(&self) -> Option<Loaded> {
        let chain = tokio::fs::read(&self.paths.cert).await.ok()?;
        let key = tokio::fs::read(&self.paths.key).await.ok()?;
        match Loaded::parse(&chain, &key) {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!(error = %e, path = %self.paths.cert.display(), "ignoring cached certificate");
                None
            }
        }
    }

    fn builder(&self) -> Result<instant_acme::AccountBuilder, AcmeError> {
        Ok(match &self.opts.root_ca {
            Some(root) => Account::builder_with_root(root)?,
            None => Account::builder()?,
        })
    }

    async fn account(&self) -> Result<Account, AcmeError> {
        if let Ok(data) = tokio::fs::read(&self.paths.account).await {
            let creds: AccountCredentials = serde_json::from_slice(&data)
                .map_err(|e| AcmeError::Invalid(format!("account file: {e}")))?;
            return Ok(self.builder()?.from_credentials(creds).await?);
        }
        let contact = (!self.opts.email.is_empty()).then(|| format!("mailto:{}", self.opts.email));
        let contact: Vec<&str> = contact.iter().map(String::as_str).collect();
        let (account, creds) = self
            .builder()?
            .create(
                &NewAccount {
                    contact: &contact,
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                self.opts.directory.clone(),
                None,
            )
            .await?;
        let data = serde_json::to_vec(&creds)
            .map_err(|e| AcmeError::Invalid(format!("account file: {e}")))?;
        write_atomic(&self.paths.account, &data).await?;
        Ok(account)
    }

    async fn issue(&self) -> Result<Loaded, AcmeError> {
        let account = self.account().await?;
        let identifiers = [Identifier::Dns(self.opts.domain.clone())];
        let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;
        let mut authorizations = order.authorizations();
        while let Some(authz) = authorizations.next().await {
            let mut authz = authz?;
            if authz.status == AuthorizationStatus::Valid {
                continue;
            }
            let mut challenge = authz
                .challenge(ChallengeType::Dns01)
                .ok_or_else(|| AcmeError::Invalid("no dns-01 challenge offered".into()))?;
            (self.hook)(Some(&challenge.key_authorization().dns_value()));
            challenge.set_ready().await?;
        }
        let status = order.poll_ready(&ORDER_POLL).await;
        (self.hook)(None);
        let status = status?;
        if status != OrderStatus::Ready {
            return Err(AcmeError::Invalid(format!("order is {status:?}")));
        }
        let key_pem = order.finalize().await?;
        let chain_pem = order.poll_certificate(&ORDER_POLL).await?;
        let loaded = Loaded::parse(chain_pem.as_bytes(), key_pem.as_bytes())?;
        write_atomic(&self.paths.key, key_pem.as_bytes()).await?;
        write_atomic(&self.paths.cert, chain_pem.as_bytes()).await?;
        Ok(loaded)
    }

    async fn run(self, mut current: Option<Loaded>) {
        let mut retry = MIN_RETRY;
        loop {
            if current.as_ref().is_none_or(|c| c.needs_renewal(now())) {
                tracing::info!(domain = %self.opts.domain, "obtaining certificate");
                match self.issue().await {
                    Ok(cert) => {
                        tracing::info!(domain = %self.opts.domain, not_after = cert.not_after, "certificate installed");
                        self.resolver.set(cert.key.clone());
                        current = Some(cert);
                        retry = MIN_RETRY;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, retry_in = ?retry, "certificate request failed");
                        tokio::time::sleep(retry).await;
                        retry = (retry * 2).min(MAX_RETRY);
                        continue;
                    }
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }
}

/// Serves the cached certificate (if any) right away and keeps it issued and renewed in
/// the background; handshakes fail until the first certificate is installed.
pub async fn start(opts: AcmeOptions, hook: ChallengeHook) -> RustlsConfig {
    let resolver = Arc::new(Resolver::default());
    let manager = Manager {
        paths: Paths::new(&opts),
        opts,
        resolver: resolver.clone(),
        hook,
    };
    let cached = manager.load_cached().await;
    if let Some(c) = &cached {
        resolver.set(c.key.clone());
    }
    tokio::spawn(manager.run(cached));
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    finish(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_window() {
        let day = 86_400;
        assert!(!renewal_due(0, 90 * day, 59 * day));
        assert!(renewal_due(0, 90 * day, 60 * day));
        assert!(renewal_due(0, 90 * day, 91 * day));
    }

    #[test]
    fn certmagic_layout() {
        assert_eq!(
            safe(&issuer_key(LETS_ENCRYPT)),
            "acme-v02.api.letsencrypt.org-directory"
        );
        assert_eq!(
            safe(&issuer_key("https://pebble:14000/dir/")),
            "pebble-14000-dir"
        );
        assert_eq!(safe(&issuer_key("https://ca.test")), "ca.test");
        assert_eq!(safe(" *.Ex..ample.com "), "wildcard_.example.com");
        let p = Paths::new(&AcmeOptions {
            domain: "Auth.Example.org".into(),
            directory: LETS_ENCRYPT_STAGING.into(),
            email: String::new(),
            cache_dir: "data/certs".into(),
            root_ca: None,
        });
        assert_eq!(
            p.cert,
            Path::new(
                "data/certs/certificates/acme-staging-v02.api.letsencrypt.org-directory/auth.example.org/auth.example.org.crt"
            )
        );
    }
}
