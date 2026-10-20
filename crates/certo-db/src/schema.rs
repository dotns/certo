use rusqlite::{Connection, OptionalExtension};

use crate::DbError;

/// Current schema version stored in `acmedns.db_version`.
pub const DB_VERSION: u32 = 4;

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
";

/// Version 3 zone tables, kept for the upgrade path; `PowerDNS` stores all of this since 4.
const ZONE_TABLES_V3: &str = "
CREATE TABLE IF NOT EXISTS zones(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    name TEXT UNIQUE NOT NULL,
    serial INTEGER NOT NULL,
    dnssec_enabled INTEGER NOT NULL DEFAULT 0,
    nsec_mode TEXT NOT NULL DEFAULT 'nsec',
    axfr_allow_from TEXT NOT NULL DEFAULT '[]',
    created_at INT NOT NULL,
    FOREIGN KEY(user_id) REFERENCES users(id)
);
CREATE TABLE IF NOT EXISTS records(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    zone_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    rtype TEXT NOT NULL,
    ttl INTEGER NOT NULL,
    rdata TEXT NOT NULL,
    created_at INT NOT NULL,
    FOREIGN KEY(zone_id) REFERENCES zones(id)
);
CREATE INDEX IF NOT EXISTS records_zone ON records(zone_id);
CREATE TABLE IF NOT EXISTS zone_keys(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    zone_id INTEGER NOT NULL,
    algorithm TEXT NOT NULL,
    private_key BLOB NOT NULL,
    created_at INT NOT NULL,
    FOREIGN KEY(zone_id) REFERENCES zones(id)
);
CREATE TABLE IF NOT EXISTS tsig_keys(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    zone_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    algorithm TEXT NOT NULL,
    secret BLOB NOT NULL,
    created_at INT NOT NULL,
    UNIQUE(zone_id, name),
    FOREIGN KEY(zone_id) REFERENCES zones(id)
);
";

/// Version 4 zones: ownership and activation state; records live in `PowerDNS`.
const ZONES_V4: &str = "
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

/// Creates missing tables and upgrades older schemas to [`DB_VERSION`].
///
/// A version 3 database that still holds zones is left at version 3: its zones must be
/// imported into `PowerDNS` first ([`crate::Db::legacy_zones`]), then
/// [`crate::Db::complete_import`] finishes the upgrade.
pub(crate) fn init(conn: &mut Connection) -> Result<(), DbError> {
    conn.execute_batch(TABLES)?;
    let stored: Option<String> = conn
        .query_row(
            "SELECT Value FROM acmedns WHERE Name='db_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(stored) = stored.filter(|v| !v.is_empty()) else {
        let tx = conn.transaction()?;
        tx.execute_batch(ZONES_V4)?;
        tx.execute(
            "INSERT INTO acmedns (Name, Value) VALUES ('db_version', ?1)",
            [DB_VERSION.to_string()],
        )?;
        tx.commit()?;
        return Ok(());
    };
    let version: u32 = stored
        .parse()
        .map_err(|_| DbError::InvalidVersion(stored.clone()))?;
    if version > DB_VERSION {
        return Err(DbError::UnsupportedVersion(version));
    }
    if version < 2 {
        upgrade_1_to_2(conn)?;
    }
    if version < 3 {
        upgrade_2_to_3(conn)?;
    }
    if version < 4 && !has_legacy_zones(conn)? {
        upgrade_3_to_4(conn)?;
    }
    Ok(())
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

fn has_legacy_zones(conn: &Connection) -> Result<bool, DbError> {
    let n: i64 = conn.query_row("SELECT count(*) FROM zones", [], |r| r.get(0))?;
    Ok(n > 0)
}

/// Replaces the version 3 zone tables with the version 4 `zones` table, keeping every
/// zone row as active and exempt (it was served before, `PowerDNS` now has its data).
pub(crate) fn upgrade_3_to_4(conn: &mut Connection) -> Result<(), DbError> {
    let now = crate::store::now();
    let tx = conn.transaction()?;
    tx.execute_batch(&ZONES_V4.replace("zones(", "zones_v4("))?;
    tx.execute(
        "INSERT INTO zones_v4 (id, user_id, name, active, exempt, verified_at, last_check, created_at)
         SELECT id, user_id, name, 1, 1, ?1, ?1, created_at FROM zones",
        [now],
    )?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS records;
         DROP TABLE IF EXISTS zone_keys;
         DROP TABLE IF EXISTS tsig_keys;
         DROP TABLE zones;
         ALTER TABLE zones_v4 RENAME TO zones;
         UPDATE acmedns SET Value='4' WHERE Name='db_version';",
    )?;
    tx.commit()?;
    Ok(())
}

fn upgrade_2_to_3(conn: &mut Connection) -> Result<(), DbError> {
    let tx = conn.transaction()?;
    tx.execute_batch(ZONE_TABLES_V3)?;
    tx.execute_batch("UPDATE acmedns SET Value='3' WHERE Name='db_version';")?;
    tx.commit()?;
    Ok(())
}

fn upgrade_1_to_2(conn: &mut Connection) -> Result<(), DbError> {
    let tx = conn.transaction()?;
    tx.execute_batch(
        "ALTER TABLE user_domains ADD COLUMN allowfrom TEXT NOT NULL DEFAULT '[]';
         UPDATE acmedns SET Value='2' WHERE Name='db_version';",
    )?;
    tx.commit()?;
    Ok(())
}
