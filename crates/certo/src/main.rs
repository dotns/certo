//! certo server binary.
#![forbid(unsafe_code)]

mod logging;

use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc};

use anyhow::Context as _;
use certo_api::{ApiOptions, AppState};
use certo_core::config::{Config, TlsMode};
use certo_db::Db;
use certo_pdns::{
    DnsDelegationChecker, PENDING_CHECK_INTERVAL, PdnsConfig, Supervisor, ZonePolicy, Zones,
};
use clap::Parser;
use tracing::{error, info, warn};

/// Control plane for a bundled `PowerDNS` authoritative server with ACME DNS-01 support.
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

/// Starts pdns and brings its zone data in line with the database.
async fn start_zones(config: &Config, db: Db) -> anyhow::Result<(Supervisor, Zones)> {
    let pdns_config = PdnsConfig::from_config(config).context("invalid [pdns] / general.listen")?;
    let supervisor = Supervisor::start(pdns_config)
        .await
        .context("cannot start pdns_server")?;
    let resolver = match config.zones.resolver.trim() {
        "" => None,
        addr => Some(
            addr.parse::<SocketAddr>()
                .with_context(|| format!("invalid zones.resolver {addr:?}"))?,
        ),
    };
    let checker = DnsDelegationChecker::new(resolver).context("cannot set up the resolver")?;
    let policy = ZonePolicy {
        base_domain: config.general.acme_domain.clone(),
        base_zone: config.general.domain.clone(),
        nsname: supervisor_nsname(config),
    };
    let zones = Zones::new(db, supervisor.client(), policy, Arc::new(checker));
    let imported = zones
        .import_legacy()
        .await
        .context("legacy zone import failed")?;
    if imported > 0 {
        info!(zones = imported, "imported zones from the previous schema");
    }
    zones
        .ensure_base_zone(&config.general.records)
        .await
        .context("cannot set up the base zone")?;
    let synced = zones
        .reconcile_txt()
        .await
        .context("challenge TXT reconciliation failed")?;
    info!(names = synced, "challenge records reconciled");
    Ok((supervisor, zones))
}

fn supervisor_nsname(config: &Config) -> String {
    if config.general.nsname.is_empty() {
        config.general.domain.clone()
    } else {
        config.general.nsname.clone()
    }
}

/// Runs the delegation checks on a fixed schedule.
async fn check_periodically(zones: Zones) {
    let mut tick = tokio::time::interval(PENDING_CHECK_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        match zones.run_checks().await {
            Ok(report) if report.checked > 0 => info!(?report, "zone checks done"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "zone checks failed"),
        }
    }
}

async fn run(config: Config, config_path: &std::path::Path) -> anyhow::Result<()> {
    info!(file = %config_path.display(), "using config file");
    info!("starting up");
    let db = Db::open(&config.database.connection).context("database initialization failed")?;
    let (supervisor, zones) = start_zones(&config, db.clone()).await?;
    let checks = tokio::spawn(check_periodically(zones.clone()));
    let api_state = AppState::new(db, zones.clone(), ApiOptions::from_config(&config))
        .context("API initialization failed")?;

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
            let hook: certo_api::ChallengeHook = Arc::new(move |key| {
                let zones = zones.clone();
                let value = key.map(str::to_owned);
                tokio::spawn(async move {
                    if let Err(e) = zones.set_own_challenge(value.as_deref()).await {
                        error!(error = %e, "cannot publish the server's own ACME challenge");
                    }
                });
            });
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

    let result = tokio::select! {
        () = supervisor.failed() => Err(anyhow::anyhow!("pdns_server keeps crashing")),
        res = &mut api => res.context("HTTP server task failed")?.context("HTTP server failed"),
        () = shutdown_signal() => {
            info!("shutting down");
            handle.graceful_shutdown(Some(SHUTDOWN_GRACE));
            api.await.context("HTTP server task failed")?.context("HTTP server failed")
        }
    };
    checks.abort();
    supervisor.shutdown().await;
    result
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
