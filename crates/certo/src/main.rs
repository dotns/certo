//! certo server binary.
#![forbid(unsafe_code)]

mod logging;

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use anyhow::Context as _;
use certo_api::{ApiOptions, AppState};
use certo_core::config::{Config, TlsMode};
use certo_db::Db;
use certo_dns::{
    ChallengeConfig, ChallengeResponder, DnsHandler, DynamicCatalog, ZonePolicy, Zones,
};
use clap::Parser;
use tracing::{error, info};

/// Authoritative DNS server with ACME DNS-01 challenge support.
#[derive(Debug, Parser)]
#[command(version = certo_core::VERSION, about)]
struct Cli {
    /// Config file location.
    #[arg(short = 'c', long = "config", default_value = "./data/config.toml")]
    config: PathBuf,
}

const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

fn harden_process() {
    #[cfg(unix)]
    {
        rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));
        // Core dumps could leak keys and secrets.
        let _ = rlimit::setrlimit(rlimit::Resource::CORE, 0, 0);
    }
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        error!(panic = %info, %backtrace, "panic");
    }));
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    error!(error = %e, "cannot listen for SIGTERM");
                    let _ = ctrl_c.await;
                    return;
                }
            };
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}

async fn run(config: Config, config_path: &std::path::Path) -> anyhow::Result<()> {
    info!(file = %config_path.display(), "using config file");
    info!("starting up");
    let db = Db::open(&config.database.connection).context("database initialization failed")?;

    let challenge = ChallengeResponder::new(
        &ChallengeConfig {
            domain: config.general.domain.clone(),
            nsname: config.general.nsname.clone(),
            nsadmin: config.general.nsadmin.clone(),
            records: config.general.records.clone(),
        },
        db.clone(),
    )
    .map_err(|e| anyhow::anyhow!("invalid general.domain: {e}"))?;
    let catalog = DynamicCatalog::new();
    let general = &config.general;
    let nsname = if general.nsname.is_empty() {
        general.domain.clone()
    } else {
        general.nsname.clone()
    };
    let nsadmin = if general.nsadmin.is_empty() {
        format!("hostmaster.{}", general.domain)
    } else {
        general.nsadmin.clone()
    };
    let policy = ZonePolicy::new(
        challenge.domain(),
        &nsname,
        &nsadmin,
        challenge.static_owners(),
    )
    .context("invalid general.nsname / general.nsadmin")?;
    let zones = Zones::new(db.clone(), catalog.clone(), policy);
    let served = zones.load_all().await.context("cannot load zones")?;
    info!(zones = served, "hosted zones loaded");
    tokio::spawn(resign_periodically(zones.clone()));
    let api_state = AppState::new(db, zones, ApiOptions::from_config(&config))
        .context("API initialization failed")?;
    let handler = DnsHandler::new(challenge.clone(), catalog);
    let (mut dns, _) = certo_dns::listen(&config.general.listen, &config.general.protocol, handler)
        .await
        .context("DNS server failed")?;

    let api_addr = format!("{}:{}", config.api.ip, config.api.port);
    let listener = std::net::TcpListener::bind(&api_addr)
        .with_context(|| format!("cannot listen on {api_addr}"))?;
    let tls = match config.api.tls {
        TlsMode::None => None,
        TlsMode::Cert => Some(
            certo_api::from_pem_files(
                config.api.tls_cert_fullchain.as_ref(),
                config.api.tls_cert_privkey.as_ref(),
            )
            .context("cannot load TLS certificate")?,
        ),
        TlsMode::LetsEncrypt | TlsMode::LetsEncryptStaging => {
            let directory = match (&config.api.tls, config.api.acme_directory.as_str()) {
                (_, url) if !url.is_empty() => url.to_owned(),
                (TlsMode::LetsEncrypt, _) => certo_api::LETS_ENCRYPT.to_owned(),
                _ => certo_api::LETS_ENCRYPT_STAGING.to_owned(),
            };
            let opts = certo_api::AcmeOptions {
                domain: config.general.domain.clone(),
                directory,
                email: config.api.notification_email.clone(),
                cache_dir: config.api.acme_cache_dir.clone().into(),
                root_ca: (!config.api.acme_root_ca.is_empty())
                    .then(|| config.api.acme_root_ca.clone().into()),
            };
            let hook: certo_api::ChallengeHook =
                Arc::new(move |key| challenge.set_own_auth_key(key.unwrap_or_default()));
            Some(certo_api::start_acme(opts, hook).await)
        }
    };
    info!(host = %api_addr, tls = tls.is_some(), "listening HTTP");
    let handle = certo_api::Handle::new();
    let mut api = tokio::spawn(certo_api::serve(
        listener,
        certo_api::app(api_state),
        tls,
        handle.clone(),
    ));

    tokio::select! {
        res = dns.block_until_done() => res.context("DNS server failed")?,
        res = &mut api => res.context("HTTP server task failed")?.context("HTTP server failed")?,
        () = shutdown_signal() => {
            info!("shutting down");
            handle.graceful_shutdown(Some(SHUTDOWN_GRACE));
            dns.shutdown_gracefully().await.context("DNS shutdown")?;
            api.await.context("HTTP server task failed")?.context("HTTP server failed")?;
        }
    }
    Ok(())
}

/// Keeps RRSIGs of signed zones far from expiry.
async fn resign_periodically(zones: Zones) {
    let mut tick = tokio::time::interval(certo_dns::RESIGN_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick fires immediately; zones were just signed by `load_all`.
    tick.tick().await;
    loop {
        tick.tick().await;
        match zones.resign_all().await {
            Ok(n) => info!(zones = n, "re-signed DNSSEC zones"),
            Err(e) => error!(error = %e, "re-signing failed"),
        }
    }
}

fn main() -> ExitCode {
    harden_process();
    let cli = Cli::parse();
    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            #[allow(clippy::print_stderr, reason = "logging is not configured yet")]
            {
                eprintln!("Error: {e}");
            }
            return ExitCode::FAILURE;
        }
    };
    // Only fails if another provider was installed first, which is fine.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    if let Err(e) = logging::init(&config.logconfig) {
        #[allow(clippy::print_stderr, reason = "logging could not be configured")]
        {
            eprintln!("Could not set up logging: {e}");
        }
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!(error = %e, "cannot start async runtime");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(config, &cli.config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = format!("{e:#}"), "fatal");
            ExitCode::FAILURE
        }
    }
}
