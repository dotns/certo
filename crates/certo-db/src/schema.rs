use rusqlite::{Connection, OptionalExtension};

use crate::DbError;

/// Schema version stored in `acmedns.db_version`. There is no upgrade chain: a database
/// with any other version is refused and has to be migrated by hand.
pub const DB_VERSION: u32 = 1;

/// Every table of the current schema; records of hosted zones live in `PowerDNS`.
const TABLES: &str = "
CREATE TABLE IF NOT EXISTS acmedns(
    Name TEXT,
    Value TEXT
);
CREATE TABLE IF NOT EXISTS txt(
    Domain TEXT NOT NULL,
    Value TEXT NOT NULL DEFAULT '',
    LastUpdate INT
);
CREATE TABLE IF NOT EXISTS users(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at INT NOT NULL
);
CREATE TABLE IF NOT EXISTS user_domains(
    user_id INTEGER NOT NULL,
    domain TEXT NOT NULL,
    subdomain TEXT UNIQUE NOT NULL,
    allowfrom TEXT NOT NULL DEFAULT '[]',
    UNIQUE(user_id, domain),
    FOREIGN KEY(user_id) REFERENCES users(id)
);
CREATE TABLE IF NOT EXISTS api_keys(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    key_value TEXT UNIQUE NOT NULL,
    scope TEXT NOT NULL DEFAULT '[\"*\"]',
    created_at INT NOT NULL,
    FOREIGN KEY(user_id) REFERENCES users(id)
);
CREATE TABLE IF NOT EXISTS zones(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    name TEXT UNIQUE NOT NULL,
    active INTEGER NOT NULL DEFAULT 0,
    exempt INTEGER NOT NULL DEFAULT 0,
    verified_at INT,
    last_check INT,
    last_error TEXT,
    failed_checks INTEGER NOT NULL DEFAULT 0,
    snapshot TEXT,
    created_at INT NOT NULL,
    FOREIGN KEY(user_id) REFERENCES users(id)
);
";

/// Creates the tables of a new database and checks the version of an existing one.
pub(crate) fn init(conn: &mut Connection) -> Result<(), DbError> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT Value FROM acmedns WHERE Name='db_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .or_else(|e| match e {
            rusqlite::Error::SqliteFailure(_, Some(ref m)) if m.contains("no such table") => {
                Ok(None)
            }
            e => Err(e),
        })?;
    match stored.filter(|v| !v.is_empty()) {
        None => {
            let tx = conn.transaction()?;
            tx.execute_batch(TABLES)?;
            tx.execute(
                "INSERT INTO acmedns (Name, Value) VALUES ('db_version', ?1)",
                [DB_VERSION.to_string()],
            )?;
            tx.commit()?;
            Ok(())
        }
        Some(stored) => {
            let version: u32 = stored
                .parse()
                .map_err(|_| DbError::InvalidVersion(stored.clone()))?;
            if version != DB_VERSION || !has_column(conn, "zones", "exempt")? {
                return Err(DbError::UnsupportedVersion(version));
            }
            Ok(())
        }
    }
}

/// Stored `db_version`.
pub(crate) fn version(conn: &Connection) -> Result<u32, DbError> {
    let stored: String = conn.query_row(
        "SELECT Value FROM acmedns WHERE Name='db_version'",
        [],
        |r| r.get(0),
    )?;
    stored
        .parse()
        .map_err(|_| DbError::InvalidVersion(stored.clone()))
}

/// Whether `table` has `column`; distinguishes this schema from older ones that reused
/// version numbers.
fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, DbError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|c| c == column);
    Ok(found)
}
