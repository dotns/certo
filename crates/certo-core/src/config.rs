//! Server configuration, read from the same TOML layout as the Go server.

use std::path::Path;

use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::Deserialize;

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read or parsed.
    #[error("error reading configuration file {path}: {source}")]
    Read {
        /// Config file path.
        path: String,
        /// Underlying parse error.
        #[source]
        source: Box<figment::Error>,
    },
    /// A value failed validation.
    #[error("{0}")]
    Invalid(String),
}

/// How the HTTP API obtains its TLS certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    /// Plain HTTP.
    #[default]
    None,
    /// Certificate and key files on disk.
    Cert,
    /// Let's Encrypt production, via DNS-01 against this server.
    LetsEncrypt,
    /// Let's Encrypt staging.
    LetsEncryptStaging,
}

/// Full server configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `[general]`
    pub general: General,
    /// `[database]`
    pub database: Database,
    /// `[api]`
    pub api: Api,
    /// `[logconfig]`
    pub logconfig: LogConfig,
}

/// `[general]`: DNS listener and base zone.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct General {
    /// DNS listen address, e.g. `127.0.0.1:53` or `:53`.
    pub listen: String,
    /// `both`, `both4`, `both6`, `udp`, `udp4`, `udp6`, `tcp`, `tcp4` or `tcp6`.
    pub protocol: String,
    /// Base domain.
    pub domain: String,
    /// SOA MNAME.
    pub nsname: String,
    /// SOA RNAME.
    pub nsadmin: String,
    /// Verbose request logging.
    pub debug: bool,
    /// Static records served in addition to dynamic data.
    pub records: Vec<String>,
}

/// `[database]`
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Database {
    /// Only `sqlite`.
    pub engine: String,
    /// File path or `file:` URL.
    pub connection: String,
}

/// `[api]`: HTTP listener and auth settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Api {
    /// Domain shown for the httpreq endpoint; defaults to `general.domain`.
    pub api_domain: String,
    /// Listen IP.
    pub ip: String,
    /// Listen port (a string, as in the Go config).
    pub port: String,
    /// TLS mode.
    pub tls: TlsMode,
    /// Key file for `tls = "cert"`.
    pub tls_cert_privkey: String,
    /// Certificate chain for `tls = "cert"`.
    pub tls_cert_fullchain: String,
    /// ACME account and certificate cache.
    pub acme_cache_dir: String,
    /// Let's Encrypt contact address.
    pub notification_email: String,
    /// ACME directory URL overriding the Let's Encrypt one (e.g. a private CA).
    pub acme_directory: String,
    /// PEM root certificate to trust for `acme_directory`.
    pub acme_root_ca: String,
    /// Allowed CORS origins.
    pub corsorigins: Vec<String>,
    /// Take the client IP from `header_name`.
    pub use_header: bool,
    /// Header carrying the client IP.
    pub header_name: String,
    /// HS256 secret for dashboard tokens; random per start when empty.
    pub jwt_secret: String,
    /// Key for the admin API; empty disables it.
    pub admin_key: String,
    /// Open registration (dashboard and acme-dns `/register`); defaults to true.
    pub allow_registration: Option<bool>,
    /// Let users create hosted zones themselves; admins always can. Defaults to false.
    pub allow_zone_creation: bool,
}

impl Api {
    /// Whether open registration endpoints are enabled.
    pub fn registration_allowed(&self) -> bool {
        self.allow_registration.unwrap_or(true)
    }
}

/// `[logconfig]`
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// `debug`, `info`, `warn`, `error`, ...
    pub loglevel: String,
    /// `stdout` or `file`.
    pub logtype: String,
    /// Log file path when `logtype = "file"`.
    pub logfile: String,
    /// `json` or text.
    pub logformat: String,
}

impl Config {
    /// Reads `path`, applies `PORT` and `CERTO_<SECTION>__<KEY>` overrides, then validates.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let read_err = |source| ConfigError::Read {
            path: path.display().to_string(),
            source: Box::new(source),
        };
        if !path.is_file() {
            return Err(read_err(figment::Error::from(format!(
                "{} not found",
                path.display()
            ))));
        }
        let config: Self = Figment::new()
            .merge(Toml::file_exact(path))
            .merge(Env::prefixed("CERTO_").split("__"))
            .merge(Env::raw().only(&["PORT"]).map(|_| "api.port".into()))
            .extract()
            .map_err(read_err)?;
        config.validated()
    }

    fn validated(mut self) -> Result<Self, ConfigError> {
        if self.database.engine.is_empty() {
            "sqlite".clone_into(&mut self.database.engine);
        }
        if self.database.engine != "sqlite" {
            return Err(ConfigError::Invalid(format!(
                "invalid database engine {:?}: only sqlite is supported",
                self.database.engine
            )));
        }
        if self.database.connection.is_empty() {
            return Err(ConfigError::Invalid(
                "missing database configuration option \"connection\"".into(),
            ));
        }
        if self.api.acme_cache_dir.is_empty() {
            "data/certs".clone_into(&mut self.api.acme_cache_dir);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn load(content: &str) -> Result<Config, ConfigError> {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        Config::load(file.path())
    }

    #[test]
    fn reads_the_go_config_layout() {
        let cfg = Config::load(Path::new("testdata/fallback.toml")).unwrap();
        assert_eq!(cfg.general.listen, "127.0.0.1:53");
        assert_eq!(cfg.general.domain, "test.example.org");
        assert_eq!(cfg.general.records.len(), 2);
        assert!(cfg.general.debug);
        assert_eq!(cfg.database.engine, "sqlite");
        assert_eq!(cfg.api.port, "443");
        assert_eq!(cfg.api.acme_cache_dir, "api-certs");
        assert_eq!(cfg.api.corsorigins, ["*"]);
        assert!(cfg.api.use_header);
        assert_eq!(cfg.api.header_name, "X-is-gonna-give-it-to-ya");
        assert!(cfg.api.registration_allowed());
    }

    #[test]
    fn repo_example_config_parses() {
        let cfg = Config::load(Path::new("../../config.toml")).unwrap();
        assert_eq!(cfg.api.tls, TlsMode::None);
        assert_eq!(cfg.logconfig.logformat, "json");
        assert_eq!(cfg.api.allow_registration, Some(true));
    }

    #[test]
    fn validation_matches_go() {
        let base = "[database]\nconnection = \"test.db\"\n";
        let cfg = load(&format!("{base}[api]\ntls = \"none\"")).unwrap();
        assert_eq!(cfg.database.engine, "sqlite");
        assert_eq!(cfg.api.acme_cache_dir, "data/certs");
        assert!(load("[database]\nengine = \"\"\nconnection = \"test.db\"").is_ok());
        assert!(load("[database]\nengine = \"sqlite\"\nconnection = \"\"").is_err());
        assert!(load("[database]\nengine = \"mysql\"\nconnection = \"x\"").is_err());
        assert!(load(&format!("{base}[api]\ntls = \"invalid\"")).is_err());
        assert_eq!(
            load(&format!("{base}[api]\ntls = \"letsencryptstaging\""))
                .unwrap()
                .api
                .tls,
            TlsMode::LetsEncryptStaging
        );
        assert!(load("[\u{0}[[[[de\nlisten =]").is_err());
        assert!(Config::load(Path::new("/nonexistent/certo.toml")).is_err());
        assert!(
            !load(&format!("{base}[api]\nallow_registration = false"))
                .unwrap()
                .api
                .registration_allowed()
        );
    }
}
