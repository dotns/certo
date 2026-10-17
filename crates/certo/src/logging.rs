use std::{fs::OpenOptions, path::Path, sync::Mutex};

use certo_core::config::LogConfig;
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt};

fn level(name: &str) -> anyhow::Result<Level> {
    Ok(match name.to_lowercase().as_str() {
        "debug" => Level::DEBUG,
        "" | "info" => Level::INFO,
        "warn" | "warning" => Level::WARN,
        "error" | "dpanic" | "panic" | "fatal" => Level::ERROR,
        other => anyhow::bail!("unrecognized level: {other:?}"),
    })
}

/// Installs the global subscriber: stdout or a log file, JSON or text.
pub(crate) fn init(config: &LogConfig) -> anyhow::Result<()> {
    let level = level(&config.loglevel)?;
    // hickory logs every request at info; keep that for debug level only.
    let hickory = if level == Level::DEBUG {
        "info"
    } else {
        "warn"
    };
    let filter = EnvFilter::new(format!(
        "{level},hickory_server={hickory},hickory_proto=warn"
    ));
    let builder = fmt().with_env_filter(filter).with_target(false);
    let json = config.logformat == "json";
    let result = if config.logtype == "file" {
        if let Some(dir) = Path::new(&config.logfile)
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
        {
            std::fs::create_dir_all(dir)?;
        }
        let file = Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&config.logfile)?,
        );
        let builder = builder.with_ansi(false).with_writer(file);
        if json {
            builder.json().try_init()
        } else {
            builder.try_init()
        }
    } else if json {
        builder.json().try_init()
    } else {
        builder.try_init()
    };
    result.map_err(|e| anyhow::anyhow!(e))
}
