//! TLS for the HTTP listener: static PEM files or ACME-issued certificates.
use std::{path::Path, sync::Arc};

use axum_server::tls_rustls::RustlsConfig;
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};

/// TLS setup failures.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    /// A certificate or key file could not be read or parsed.
    #[error("{path}: {source}")]
    Pem {
        /// Offending file.
        path: String,
        /// Parse error.
        source: rustls::pki_types::pem::Error,
    },
    /// rustls rejected the certificate/key pair.
    #[error(transparent)]
    Rustls(#[from] rustls::Error),
}

pub(crate) fn finish(mut cfg: ServerConfig) -> RustlsConfig {
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    RustlsConfig::from_config(Arc::new(cfg))
}

/// Loads a PEM certificate chain and private key (`api.tls = "cert"`).
pub fn from_pem_files(fullchain: &Path, privkey: &Path) -> Result<RustlsConfig, TlsError> {
    let pem_err = |path: &Path| {
        let path = path.display().to_string();
        move |source| TlsError::Pem { path, source }
    };
    let certs = CertificateDer::pem_file_iter(fullchain)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(pem_err(fullchain))?;
    let key = PrivateKeyDer::from_pem_file(privkey).map_err(pem_err(privkey))?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(finish(cfg))
}
