//! Starts `pdns_server`, keeps it running and stops it with certo.

use std::{
    io::Write as _,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::{Child, Command},
    sync::Notify,
    task::JoinHandle,
};
use tracing::{error, info, warn};

use crate::{Client, PdnsConfig, PdnsError, SCHEMA};

const READY_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// More than this many crashes within [`CRASH_WINDOW`] gives up.
const CRASH_LIMIT: u32 = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(60);

struct Shared {
    config: PdnsConfig,
    client: Client,
    pid: AtomicU32,
    stopping: AtomicBool,
    failed: Notify,
    gave_up: AtomicBool,
}

/// A running, supervised `pdns_server`.
#[derive(Debug)]
pub struct Supervisor {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("pid", &self.pid.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Supervisor {
    /// Prepares the data directory, writes the config, starts pdns and waits for its API.
    pub async fn start(config: PdnsConfig) -> Result<Self, PdnsError> {
        if !config.binary.is_file() {
            return Err(PdnsError::Config(format!(
                "pdns_server not found at {}",
                config.binary.display()
            )));
        }
        prepare_dir(&config)?;
        let api_key = certo_core::ids::nanoid(32);
        write_conf(&config, &api_key)?;
        let client = Client::new(&config.api_url(), &api_key)?;
        let shared = Arc::new(Shared {
            config,
            client,
            pid: AtomicU32::new(0),
            stopping: AtomicBool::new(false),
            failed: Notify::new(),
            gave_up: AtomicBool::new(false),
        });
        let child = spawn_ready(&shared).await?;
        let task = tokio::spawn(supervise(Arc::clone(&shared), child));
        Ok(Self { shared, task })
    }

    /// API client bound to this server.
    pub fn client(&self) -> Client {
        self.shared.client.clone()
    }

    /// Resolves when pdns crashed too often to be restarted; certo should exit then.
    pub async fn failed(&self) {
        if self.shared.gave_up.load(Ordering::SeqCst) {
            return;
        }
        self.shared.failed.notified().await;
    }

    /// Stops pdns (SIGTERM, then SIGKILL after a grace period).
    pub async fn shutdown(self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        signal(&self.shared, rustix::process::Signal::TERM);
        if tokio::time::timeout(STOP_TIMEOUT, self.task).await.is_err() {
            warn!("pdns_server did not stop in time, killing it");
            signal(&self.shared, rustix::process::Signal::KILL);
        }
    }
}

fn signal(shared: &Shared, sig: rustix::process::Signal) {
    let pid = shared.pid.load(Ordering::SeqCst);
    let Some(pid) = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap_or(0)) else {
        return;
    };
    if let Err(e) = rustix::process::kill_process(pid, sig) {
        warn!(error = %e, pid = pid.as_raw_nonzero(), "cannot signal pdns_server");
    }
}

fn prepare_dir(config: &PdnsConfig) -> Result<(), PdnsError> {
    std::fs::create_dir_all(&config.dir).map_err(PdnsError::io("creating the data directory"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&config.dir, std::fs::Permissions::from_mode(0o700))
            .map_err(PdnsError::io("securing the data directory"))?;
    }
    let db = config.database();
    if !db.exists() {
        info!(path = %db.display(), "creating the PowerDNS database");
        let conn = rusqlite::Connection::open(&db)?;
        conn.execute_batch(SCHEMA)?;
    }
    Ok(())
}

fn write_conf(config: &PdnsConfig, api_key: &str) -> Result<(), PdnsError> {
    let content = config.render(api_key)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(config.conf_path())
        .map_err(PdnsError::io("writing pdns.conf"))?;
    file.write_all(content.as_bytes())
        .map_err(PdnsError::io("writing pdns.conf"))
}

async fn spawn_ready(shared: &Shared) -> Result<Child, PdnsError> {
    let config = &shared.config;
    let mut child = Command::new(&config.binary)
        .arg(format!("--config-dir={}", config.dir.display()))
        .arg("--config-name=")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(PdnsError::io("starting pdns_server"))?;
    let pid = child.id().unwrap_or(0);
    shared.pid.store(pid, Ordering::SeqCst);
    if let Some(out) = child.stdout.take() {
        tokio::spawn(forward(out));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(forward(err));
    }
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(PdnsError::io("waiting for pdns_server"))?
        {
            return Err(PdnsError::NotReady(format!(
                "exited during startup ({status})"
            )));
        }
        if shared.client.ping().await {
            info!(pid, "pdns_server ready");
            return Ok(child);
        }
        if Instant::now() >= deadline {
            let _ = child.start_kill();
            return Err(PdnsError::NotReady(format!(
                "no API answer within {}s",
                READY_TIMEOUT.as_secs()
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn forward<R: tokio::io::AsyncRead + Unpin>(reader: R) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.to_ascii_lowercase().contains("error") || line.contains("fatal") {
            warn!(target: "pdns", "{line}");
        } else {
            info!(target: "pdns", "{line}");
        }
    }
}

async fn supervise(shared: Arc<Shared>, first: Child) {
    let mut child = Some(first);
    let mut crashes: Vec<Instant> = Vec::new();
    let mut backoff = shared.config.restart_backoff;
    loop {
        if let Some(c) = child.as_mut() {
            let status = c.wait().await;
            if shared.stopping.load(Ordering::SeqCst) {
                info!("pdns_server stopped");
                return;
            }
            error!(status = ?status.ok(), "pdns_server exited unexpectedly");
        }
        let now = Instant::now();
        crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
        crashes.push(now);
        if crashes.len() > CRASH_LIMIT as usize {
            error!(
                crashes = crashes.len(),
                "pdns_server keeps crashing, giving up"
            );
            shared.gave_up.store(true, Ordering::SeqCst);
            shared.failed.notify_waiters();
            return;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        match spawn_ready(&shared).await {
            Ok(c) => {
                child = Some(c);
                backoff = shared.config.restart_backoff;
            }
            Err(e) => {
                error!(error = %e, "pdns_server restart failed");
                child = None;
            }
        }
    }
}
