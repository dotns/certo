use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use certo_core::{ApiKey, TxtRecord, User, UserDomain, ids, internal_domain, sanitize_fqdn};
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, params, params_from_iter};

use crate::schema;

/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Only local paths and `file:` URLs are accepted.
    #[error("invalid sqlite connection {0:?}: only local paths and file: URLs are supported")]
    InvalidConnection(String),
    /// `db_version` is not a number.
    #[error("invalid database version {0:?}")]
    InvalidVersion(String),
    /// `db_version` is newer than this build understands.
    #[error(
        "unsupported database version {0}, expected {expected}; regenerate the database",
        expected = schema::DB_VERSION
    )]
    UnsupportedVersion(u32),
    /// The requested row does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness constraint was violated.
    #[error("{0} already exists")]
    Conflict(&'static str),
    /// Retries for a random identifier were exhausted.
    #[error("{0}: identifier collision after retries")]
    Exhausted(&'static str),
    /// Any other SQLite failure.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Creating the database directory failed.
    #[error("create database directory: {0}")]
    Io(#[from] std::io::Error),
    /// The blocking task was cancelled or panicked, or a previous holder panicked.
    #[error("database worker failed")]
    Worker,
}

/// Handle to the SQLite database; cheap to clone.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

const NO_LOGIN_HASH: &str = "!";
const RETRIES: usize = 10;

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

pub(crate) fn normalize(domain: &str) -> String {
    domain.trim().to_lowercase()
}

pub(crate) fn is_unique_violation(err: &rusqlite::Error) -> bool {
    err.sqlite_error_code() == Some(ErrorCode::ConstraintViolation)
}

pub(crate) fn scope_json(scope: &[String]) -> String {
    serde_json::to_string(scope).unwrap_or_else(|_| "[]".to_owned())
}

pub(crate) fn parse_list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn api_key_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ApiKey> {
    Ok(ApiKey {
        id: r.get(0)?,
        user_id: r.get(1)?,
        name: r.get(2)?,
        key: r.get(3)?,
        scope: parse_list(&r.get::<_, String>(4)?),
        created_at: r.get(5)?,
    })
}

fn user_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        created_at: r.get(3)?,
    })
}

/// Removes the challenge TXT values of `<subdomain>.<base_domain>`.
fn delete_subdomain_txt(
    tx: &rusqlite::Transaction<'_>,
    subdomain: &str,
    base_domain: &str,
) -> Result<(), DbError> {
    tx.execute(
        "DELETE FROM txt WHERE Domain=?1",
        [internal_domain(subdomain, base_domain)],
    )?;
    Ok(())
}

fn txt_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TxtRecord> {
    Ok(TxtRecord {
        domain: r.get(0)?,
        value: r.get(1)?,
        last_update: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
    })
}

/// Splits a connection string into the path to create directories for and the open target.
fn resolve(connection: &str) -> Result<Option<(String, bool)>, DbError> {
    if connection.is_empty() || connection == ":memory:" {
        return Ok(None);
    }
    if let Some(rest) = connection.strip_prefix("file:") {
        let path = rest.split('?').next().unwrap_or_default();
        let path = path.strip_prefix("//").unwrap_or(path);
        return Ok(Some((path.to_owned(), true)));
    }
    if connection.contains("://") {
        return Err(DbError::InvalidConnection(connection.to_owned()));
    }
    let path = connection.split('?').next().unwrap_or_default();
    Ok(Some((path.to_owned(), false)))
}

impl Db {
    /// Opens (creating if needed) and migrates the database.
    ///
    /// `connection` is a file path, a `file:` URL, or `:memory:` / empty for an in-memory DB.
    pub fn open(connection: &str) -> Result<Self, DbError> {
        let mut conn = match resolve(connection)? {
            None => Connection::open_in_memory()?,
            Some((path, is_uri)) => {
                if let Some(dir) = Path::new(&path)
                    .parent()
                    .filter(|d| !d.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(dir)?;
                }
                if is_uri {
                    Connection::open_with_flags(
                        connection,
                        OpenFlags::default() | OpenFlags::SQLITE_OPEN_URI,
                    )?
                } else {
                    Connection::open(&path)?
                }
            }
        };
        conn.busy_timeout(Duration::from_secs(5))?;
        schema::init(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub(crate) async fn run<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().map_err(|_| DbError::Worker)?;
            f(&mut guard)
        })
        .await
        .map_err(|_| DbError::Worker)?
    }

    /// Stored schema version (3 while zones still await their import into `PowerDNS`).
    pub async fn schema_version(&self) -> Result<u32, DbError> {
        self.run(|c| schema::version(c)).await
    }

    /// Every challenge name with its current TXT values.
    pub async fn txt_by_domain(&self) -> Result<Vec<(String, Vec<String>)>, DbError> {
        self.run(|c| {
            let mut stmt = c.prepare("SELECT Domain, Value FROM txt ORDER BY Domain, rowid")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out: Vec<(String, Vec<String>)> = Vec::new();
            for row in rows {
                let (domain, value) = row?;
                match out.last_mut() {
                    Some((d, values)) if *d == domain => values.push(value),
                    _ => out.push((domain, vec![value])),
                }
            }
            Ok(out)
        })
        .await
    }

    /// Checks that the database answers queries.
    pub async fn ping(&self) -> Result<(), DbError> {
        self.run(|c| Ok(c.query_row("SELECT 1", [], |_| Ok(()))?))
            .await
    }

    // --- TXT records ---

    /// Adds a challenge TXT value.
    pub async fn present_txt(&self, fqdn: &str, value: &str) -> Result<(), DbError> {
        let (domain, value) = (sanitize_fqdn(fqdn), value.to_owned());
        self.run(move |c| {
            c.execute(
                "INSERT INTO txt (Domain, Value, LastUpdate) VALUES (?1, ?2, ?3)",
                params![domain, value, now()],
            )?;
            Ok(())
        })
        .await
    }

    /// Removes one challenge TXT value.
    pub async fn cleanup_txt(&self, fqdn: &str, value: &str) -> Result<(), DbError> {
        let (domain, value) = (sanitize_fqdn(fqdn), value.to_owned());
        self.run(move |c| {
            c.execute(
                "DELETE FROM txt WHERE Domain=?1 AND Value=?2",
                params![domain, value],
            )?;
            Ok(())
        })
        .await
    }

    /// Stores an acme-dns TXT value, keeping only the two newest values of the name.
    pub async fn update_acmedns_txt(&self, fqdn: &str, value: &str) -> Result<(), DbError> {
        let (domain, value) = (sanitize_fqdn(fqdn), value.to_owned());
        self.run(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO txt (Domain, Value, LastUpdate) VALUES (?1, ?2, ?3)",
                params![domain, value, now()],
            )?;
            tx.execute(
                "DELETE FROM txt WHERE Domain=?1 AND rowid NOT IN (
                    SELECT rowid FROM txt WHERE Domain=?1 ORDER BY LastUpdate DESC, rowid DESC LIMIT 2)",
                params![domain],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// TXT values for an exact owner name (already normalized).
    pub async fn txt_for_domain(&self, domain: &str) -> Result<Vec<String>, DbError> {
        let domain = domain.to_owned();
        self.run(move |c| {
            let mut stmt = c.prepare_cached("SELECT Value FROM txt WHERE Domain=?1")?;
            let rows = stmt.query_map([domain], |r| r.get(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// All TXT records, newest first.
    pub async fn list_txt_records(&self) -> Result<Vec<TxtRecord>, DbError> {
        self.run(|c| {
            let mut stmt = c.prepare(
                "SELECT Domain, Value, LastUpdate FROM txt ORDER BY LastUpdate DESC, rowid DESC",
            )?;
            let rows = stmt.query_map([], txt_row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// TXT records for the given owner names, newest first.
    pub async fn list_txt_records_by_domains(
        &self,
        domains: &[String],
    ) -> Result<Vec<TxtRecord>, DbError> {
        if domains.is_empty() {
            return Ok(Vec::new());
        }
        let domains: Vec<String> = domains.iter().map(|d| sanitize_fqdn(d)).collect();
        self.run(move |c| {
            let placeholders = vec!["?"; domains.len()].join(",");
            let sql = format!(
                "SELECT Domain, Value, LastUpdate FROM txt WHERE Domain IN ({placeholders}) ORDER BY LastUpdate DESC, rowid DESC"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(domains), txt_row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    // --- Users ---

    /// Creates a user together with a global `Default` API key.
    pub async fn create_user(&self, username: &str, password_hash: &str) -> Result<User, DbError> {
        let (username, password_hash) = (username.to_owned(), password_hash.to_owned());
        self.run(move |c| {
            let created_at = now();
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO users (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
                params![username, password_hash, created_at],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    DbError::Conflict("user")
                } else {
                    e.into()
                }
            })?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO api_keys (user_id, name, key_value, scope, created_at) VALUES (?1, 'Default', ?2, ?3, ?4)",
                params![id, ids::api_key(), scope_json(&["*".to_owned()]), created_at],
            )?;
            tx.commit()?;
            Ok(User {
                id,
                username,
                password_hash,
                created_at,
            })
        })
        .await
    }

    /// Allocates an anonymous acme-dns account; returns `(username, api_key, subdomain)`.
    pub async fn create_acmedns_account(
        &self,
        allow_from: Vec<String>,
    ) -> Result<(String, String, String), DbError> {
        self.run(move |c| {
            let created_at = now();
            let allow_from = scope_json(&allow_from);
            for _ in 0..RETRIES {
                let username = ids::acmedns_username();
                let subdomain = ids::nanoid(ids::ACMEDNS_SUBDOMAIN_LENGTH);
                let api_key = ids::api_key();
                let tx = c.transaction()?;
                let inserted = tx
                    .execute(
                        "INSERT INTO users (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
                        params![username, NO_LOGIN_HASH, created_at],
                    )
                    .and_then(|_| {
                        let uid = tx.last_insert_rowid();
                        tx.execute(
                            "INSERT INTO api_keys (user_id, name, key_value, scope, created_at) VALUES (?1, 'acme-dns', ?2, ?3, ?4)",
                            params![uid, api_key, scope_json(&["*".to_owned()]), created_at],
                        )?;
                        tx.execute(
                            "INSERT INTO user_domains (user_id, domain, subdomain, allowfrom) VALUES (?1, ?2, ?2, ?3)",
                            params![uid, subdomain, allow_from],
                        )
                    });
                match inserted {
                    Ok(_) => {
                        tx.commit()?;
                        return Ok((username, api_key, subdomain));
                    }
                    Err(e) if is_unique_violation(&e) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            Err(DbError::Exhausted("acme-dns account"))
        })
        .await
    }

    /// Looks up a user by login name.
    pub async fn user_by_username(&self, username: &str) -> Result<User, DbError> {
        let username = username.to_owned();
        self.run(move |c| {
            c.query_row(
                "SELECT id, username, password_hash, created_at FROM users WHERE username=?1",
                [username],
                user_row,
            )
            .optional()?
            .ok_or(DbError::NotFound("user"))
        })
        .await
    }

    /// Looks up a user by id.
    pub async fn user_by_id(&self, id: i64) -> Result<User, DbError> {
        self.run(move |c| {
            c.query_row(
                "SELECT id, username, password_hash, created_at FROM users WHERE id=?1",
                [id],
                user_row,
            )
            .optional()?
            .ok_or(DbError::NotFound("user"))
        })
        .await
    }

    /// All users ordered by id.
    pub async fn list_users(&self) -> Result<Vec<User>, DbError> {
        self.run(|c| {
            let mut stmt =
                c.prepare("SELECT id, username, password_hash, created_at FROM users ORDER BY id")?;
            let rows = stmt.query_map([], user_row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// Deletes a user with their keys, domains (and their challenge TXT values) and zones.
    /// Returns the internal challenge names whose TXT values were removed.
    pub async fn delete_user(
        &self,
        user_id: i64,
        base_domain: &str,
    ) -> Result<Vec<String>, DbError> {
        let base_domain = base_domain.to_owned();
        self.run(move |c| {
            let tx = c.transaction()?;
            let subdomains: Vec<String> = tx
                .prepare("SELECT subdomain FROM user_domains WHERE user_id=?1")?
                .query_map([user_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for subdomain in &subdomains {
                delete_subdomain_txt(&tx, subdomain, &base_domain)?;
            }
            crate::zones::delete_user_zones(&tx, user_id)?;
            tx.execute("DELETE FROM api_keys WHERE user_id=?1", [user_id])?;
            tx.execute("DELETE FROM user_domains WHERE user_id=?1", [user_id])?;
            tx.execute("DELETE FROM users WHERE id=?1", [user_id])?;
            tx.commit()?;
            Ok(subdomains
                .iter()
                .map(|s| internal_domain(s, &base_domain))
                .collect())
        })
        .await
    }

    // --- Domains ---

    /// Registers a domain for a user and allocates its subdomain.
    pub async fn add_user_domain(
        &self,
        user_id: i64,
        username: &str,
        domain: &str,
    ) -> Result<UserDomain, DbError> {
        let (username, domain) = (username.to_owned(), normalize(domain));
        self.run(move |c| {
            let taken: bool = c
                .query_row(
                    "SELECT 1 FROM user_domains WHERE user_id=?1 AND domain=?2",
                    params![user_id, domain],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if taken {
                return Err(DbError::Conflict("domain"));
            }
            for salt in 0..RETRIES {
                let subdomain = ids::subdomain(&username, &domain, salt);
                match c.execute(
                    "INSERT INTO user_domains (user_id, domain, subdomain) VALUES (?1, ?2, ?3)",
                    params![user_id, domain, subdomain],
                ) {
                    Ok(_) => {
                        return Ok(UserDomain {
                            domain,
                            subdomain,
                            owner: None,
                        });
                    }
                    Err(e) if is_unique_violation(&e) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            Err(DbError::Exhausted("subdomain"))
        })
        .await
    }

    /// Removes a user's domain together with its challenge TXT values; returns the
    /// internal challenge name when the domain existed.
    pub async fn remove_user_domain(
        &self,
        user_id: i64,
        domain: &str,
        base_domain: &str,
    ) -> Result<Option<String>, DbError> {
        let (domain, base_domain) = (normalize(domain), base_domain.to_owned());
        self.run(move |c| {
            let tx = c.transaction()?;
            let subdomain: Option<String> = tx
                .query_row(
                    "SELECT subdomain FROM user_domains WHERE user_id=?1 AND domain=?2",
                    params![user_id, domain],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(subdomain) = &subdomain {
                delete_subdomain_txt(&tx, subdomain, &base_domain)?;
                tx.execute(
                    "DELETE FROM user_domains WHERE user_id=?1 AND domain=?2",
                    params![user_id, domain],
                )?;
            }
            tx.commit()?;
            Ok(subdomain.map(|s| internal_domain(&s, &base_domain)))
        })
        .await
    }

    /// A user's domains ordered by name.
    pub async fn user_domains(&self, user_id: i64) -> Result<Vec<UserDomain>, DbError> {
        self.run(move |c| {
            let mut stmt = c.prepare(
                "SELECT domain, subdomain FROM user_domains WHERE user_id=?1 ORDER BY domain",
            )?;
            let rows = stmt.query_map([user_id], |r| {
                Ok(UserDomain {
                    domain: r.get(0)?,
                    subdomain: r.get(1)?,
                    owner: None,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// The subdomain allocated to a user's domain.
    pub async fn subdomain_by_user_domain(
        &self,
        user_id: i64,
        domain: &str,
    ) -> Result<String, DbError> {
        let domain = normalize(domain);
        self.run(move |c| {
            c.query_row(
                "SELECT subdomain FROM user_domains WHERE user_id=?1 AND domain=?2",
                params![user_id, domain],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(DbError::NotFound("domain"))
        })
        .await
    }

    /// The user owning a subdomain.
    pub async fn subdomain_owner(&self, subdomain: &str) -> Result<i64, DbError> {
        let subdomain = normalize(subdomain);
        self.run(move |c| {
            c.query_row(
                "SELECT user_id FROM user_domains WHERE subdomain=?1",
                [subdomain],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(DbError::NotFound("subdomain"))
        })
        .await
    }

    /// Owner id, real domain and source allow list of a subdomain.
    pub async fn user_domain_by_subdomain(
        &self,
        subdomain: &str,
    ) -> Result<(i64, String, Vec<String>), DbError> {
        let subdomain = normalize(subdomain);
        self.run(move |c| {
            c.query_row(
                "SELECT user_id, domain, allowfrom FROM user_domains WHERE subdomain=?1",
                [subdomain],
                |r| Ok((r.get(0)?, r.get(1)?, parse_list(&r.get::<_, String>(2)?))),
            )
            .optional()?
            .ok_or(DbError::NotFound("subdomain"))
        })
        .await
    }

    /// Every domain with its owner's username, ordered by domain.
    pub async fn list_all_domains(&self) -> Result<Vec<UserDomain>, DbError> {
        self.run(|c| {
            let mut stmt = c.prepare(
                "SELECT ud.domain, ud.subdomain, u.username FROM user_domains ud
                 JOIN users u ON ud.user_id = u.id ORDER BY ud.domain",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(UserDomain {
                    domain: r.get(0)?,
                    subdomain: r.get(1)?,
                    owner: Some(r.get(2)?),
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    // --- API keys ---

    /// Creates an API key for a user.
    pub async fn create_api_key(
        &self,
        user_id: i64,
        name: &str,
        scope: Vec<String>,
    ) -> Result<ApiKey, DbError> {
        let name = name.to_owned();
        self.run(move |c| {
            let key = ids::api_key();
            let created_at = now();
            c.execute(
                "INSERT INTO api_keys (user_id, name, key_value, scope, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, name, key, scope_json(&scope), created_at],
            )?;
            Ok(ApiKey {
                id: c.last_insert_rowid(),
                user_id,
                name,
                key,
                scope,
                created_at,
            })
        })
        .await
    }

    /// A user's keys, oldest first.
    pub async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>, DbError> {
        self.run(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, user_id, name, key_value, scope, created_at FROM api_keys
                 WHERE user_id=?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([user_id], api_key_row)?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .await
    }

    /// Deletes one of a user's keys.
    pub async fn delete_api_key(&self, user_id: i64, key_id: i64) -> Result<(), DbError> {
        self.run(move |c| {
            c.execute(
                "DELETE FROM api_keys WHERE id=?1 AND user_id=?2",
                params![key_id, user_id],
            )?;
            Ok(())
        })
        .await
    }

    /// Looks up a key by its secret value.
    pub async fn api_key_by_value(&self, value: &str) -> Result<ApiKey, DbError> {
        let value = value.to_owned();
        self.run(move |c| {
            c.query_row(
                "SELECT id, user_id, name, key_value, scope, created_at FROM api_keys WHERE key_value=?1",
                [value],
                api_key_row,
            )
            .optional()?
            .ok_or(DbError::NotFound("api key"))
        })
        .await
    }

    /// Replaces a key's scope.
    pub async fn update_api_key_scope(
        &self,
        key_id: i64,
        scope: Vec<String>,
    ) -> Result<(), DbError> {
        self.run(move |c| {
            c.execute(
                "UPDATE api_keys SET scope=?1 WHERE id=?2",
                params![scope_json(&scope), key_id],
            )?;
            Ok(())
        })
        .await
    }
}
