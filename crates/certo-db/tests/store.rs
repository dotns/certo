//! Storage behavior, ported from the Go implementation's tests, plus schema checks.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    reason = "test code"
)]

use certo_db::{DB_VERSION, Db, DbError};
use rusqlite::Connection;

fn mem() -> Db {
    Db::open(":memory:").unwrap()
}

fn columns(path: &std::path::Path, table: &str) -> Vec<String> {
    let c = Connection::open(path).unwrap();
    let mut stmt = c.prepare(&format!("PRAGMA table_info({table})")).unwrap();
    stmt.query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn version(path: &std::path::Path) -> String {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT Value FROM acmedns WHERE Name='db_version'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn fresh_database_has_current_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/dir/certo.db");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    db.ping().await.unwrap();
    assert_eq!(version(&path), DB_VERSION.to_string());
    assert!(columns(&path, "user_domains").contains(&"allowfrom".to_owned()));
    assert!(!columns(&path, "users").contains(&"api_key".to_owned()));
    assert!(columns(&path, "zones").contains(&"version".to_owned()));
    assert!(!columns(&path, "zones").contains(&"snapshot".to_owned()));
    assert!(columns(&path, "zone_rrsets").contains(&"records".to_owned()));
    assert!(columns(&path, "zone_dnssec").contains(&"private_key".to_owned()));
    assert!(columns(&path, "zone_tsig_keys").contains(&"secret".to_owned()));
    for table in ["records", "zone_keys", "tsig_keys"] {
        assert!(columns(&path, table).is_empty(), "{table}");
    }

    // Reopening is a no-op.
    drop(db);
    Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(version(&path), DB_VERSION.to_string());
}

#[test]
fn earlier_generation_database_is_rejected() {
    // A database of the previous certo generation at its last schema (4).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE acmedns(Name TEXT, Value TEXT);
             INSERT INTO acmedns (Name, Value) VALUES ('db_version', '4');",
        )
        .unwrap();
    let err = Db::open(path.to_str().unwrap()).unwrap_err();
    assert!(matches!(err, DbError::UnsupportedVersion(4)), "{err}");
    assert!(
        err.to_string().contains("migrate the database by hand"),
        "{err}"
    );
    assert_eq!(version(&path), "4");
}

#[test]
fn go_database_with_the_same_version_number_is_rejected() {
    // The Go implementation also wrote db_version 1, with a different layout.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("go.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE acmedns(Name TEXT, Value TEXT);
             INSERT INTO acmedns (Name, Value) VALUES ('db_version', '1');
             CREATE TABLE users(id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT UNIQUE NOT NULL, password_hash TEXT NOT NULL, created_at INT NOT NULL);
             CREATE TABLE user_domains(user_id INTEGER NOT NULL, domain TEXT NOT NULL, subdomain TEXT UNIQUE NOT NULL, UNIQUE(user_id, domain));",
        )
        .unwrap();
    let err = Db::open(path.to_str().unwrap()).unwrap_err();
    assert!(matches!(err, DbError::UnsupportedVersion(1)), "{err}");
    assert!(columns(&path, "zones").is_empty());
}

#[test]
fn hand_migrated_database_gets_the_zone_data_tables() {
    // deployment.md: version set to 1 and the two columns added; tables come from certo.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migrated.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE acmedns(Name TEXT, Value TEXT);
             INSERT INTO acmedns (Name, Value) VALUES ('db_version', '1');
             CREATE TABLE zones(id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER, name TEXT UNIQUE NOT NULL,
                 active INTEGER NOT NULL DEFAULT 0, exempt INTEGER NOT NULL DEFAULT 0, verified_at INT, last_check INT,
                 last_error TEXT, failed_checks INTEGER NOT NULL DEFAULT 0, snapshot TEXT, created_at INT NOT NULL,
                 version INTEGER NOT NULL DEFAULT 1, axfr_allow_from TEXT NOT NULL DEFAULT '[]');",
        )
        .unwrap();
    Db::open(path.to_str().unwrap()).unwrap();
    assert!(columns(&path, "zone_rrsets").contains(&"records".to_owned()));
    assert!(columns(&path, "users").contains(&"username".to_owned()));
}

#[test]
fn newer_database_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE acmedns(Name TEXT, Value TEXT);
             INSERT INTO acmedns (Name, Value) VALUES ('db_version', '2');",
        )
        .unwrap();
    let err = Db::open(path.to_str().unwrap()).unwrap_err();
    assert!(matches!(err, DbError::UnsupportedVersion(2)), "{err}");
}

#[test]
fn connection_strings() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("file:{}?cache=shared", dir.path().join("u.db").display());
    Db::open(&url).unwrap();
    assert!(dir.path().join("u.db").exists());
    Db::open("").unwrap();
    assert!(matches!(
        Db::open("postgres://localhost/db"),
        Err(DbError::InvalidConnection(_))
    ));
}

#[tokio::test]
async fn present_and_cleanup_txt() {
    let db = mem();
    db.present_txt("_acme-challenge.EXAMPLE.com.", "v1")
        .await
        .unwrap();
    db.present_txt("_acme-challenge.example.com.", "v2")
        .await
        .unwrap();
    let mut got = db
        .txt_for_domain("_acme-challenge.example.com")
        .await
        .unwrap();
    got.sort();
    assert_eq!(got, ["v1", "v2"]);

    db.cleanup_txt("_acme-challenge.example.com.", "v1")
        .await
        .unwrap();
    assert_eq!(
        db.txt_for_domain("_acme-challenge.example.com")
            .await
            .unwrap(),
        ["v2"]
    );
    db.cleanup_txt("_acme-challenge.example.com.", "v2")
        .await
        .unwrap();
    assert!(
        db.txt_for_domain("_acme-challenge.example.com")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.txt_for_domain("does-not-exist")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn acmedns_update_keeps_two_newest_values() {
    let db = mem();
    for v in ["a", "b", "c"] {
        db.update_acmedns_txt("sub.dns.example.com", v)
            .await
            .unwrap();
    }
    let mut got = db.txt_for_domain("sub.dns.example.com").await.unwrap();
    got.sort();
    assert_eq!(got, ["b", "c"]);
}

#[tokio::test]
async fn list_txt_records() {
    let db = mem();
    assert!(db.list_txt_records().await.unwrap().is_empty());
    for d in [
        "a.com.example.com",
        "b.com.example.com",
        "c.com.example.com",
    ] {
        db.present_txt(d, "v").await.unwrap();
    }
    assert_eq!(db.list_txt_records().await.unwrap().len(), 3);
    let some = db
        .list_txt_records_by_domains(&["a.com.example.com".into(), "B.com.example.com.".into()])
        .await
        .unwrap();
    assert_eq!(some.len(), 2);
    assert!(some.iter().all(|r| r.last_update > 0));
    assert!(
        db.list_txt_records_by_domains(&[])
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn users_and_default_key() {
    let db = mem();
    let user = db.create_user("testuser", "$2a$10$hash").await.unwrap();
    assert!(user.id > 0);
    assert_eq!(db.user_by_username("testuser").await.unwrap(), user);
    assert_eq!(db.user_by_id(user.id).await.unwrap(), user);
    assert!(matches!(
        db.user_by_id(999).await,
        Err(DbError::NotFound(_))
    ));
    assert!(matches!(
        db.create_user("testuser", "x").await,
        Err(DbError::Conflict("user"))
    ));

    let keys = db.list_api_keys(user.id).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "Default");
    assert!(keys[0].is_global());
    assert_eq!(db.list_users().await.unwrap(), [user]);
}

#[tokio::test]
async fn user_domains() {
    let db = mem();
    let u1 = db.create_user("domuser", "h").await.unwrap();
    let d1 = db
        .add_user_domain(u1.id, "domuser", " Example.com ")
        .await
        .unwrap();
    assert_eq!(d1.domain, "example.com");
    assert_eq!(
        d1.subdomain,
        certo_core::ids::subdomain("domuser", "example.com", 0)
    );
    let d2 = db
        .add_user_domain(u1.id, "domuser", "other.com")
        .await
        .unwrap();
    assert_ne!(d1.subdomain, d2.subdomain);
    assert!(matches!(
        db.add_user_domain(u1.id, "domuser", "example.com").await,
        Err(DbError::Conflict("domain"))
    ));

    let names: Vec<_> = db
        .user_domains(u1.id)
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.domain)
        .collect();
    assert_eq!(names, ["example.com", "other.com"]);
    assert_eq!(
        db.subdomain_by_user_domain(u1.id, "EXAMPLE.com")
            .await
            .unwrap(),
        d1.subdomain
    );
    assert_eq!(db.subdomain_owner(&d1.subdomain).await.unwrap(), u1.id);

    let u2 = db.create_user("domuser2", "h").await.unwrap();
    let d3 = db
        .add_user_domain(u2.id, "domuser2", "example.com")
        .await
        .unwrap();
    assert_ne!(d3.subdomain, d1.subdomain);

    let all = db.list_all_domains().await.unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|d| d.owner.is_some()));

    let name = certo_core::internal_domain(&d1.subdomain, "auth.example.org");
    db.present_txt(&name, "token").await.unwrap();
    db.remove_user_domain(u1.id, "example.com", "auth.example.org")
        .await
        .unwrap();
    assert_eq!(db.user_domains(u1.id).await.unwrap().len(), 1);
    assert!(db.txt_for_domain(&name).await.unwrap().is_empty());
    assert!(matches!(
        db.subdomain_by_user_domain(u1.id, "example.com").await,
        Err(DbError::NotFound(_))
    ));
}

#[tokio::test]
async fn subdomain_collision_uses_next_salt() {
    let db = mem();
    // Usernames are case-sensitive but the subdomain hash lowercases them, so "b" and "B"
    // derive the same salt-0 subdomain for x.com.
    let lower = db.create_user("b", "h").await.unwrap();
    let upper = db.create_user("B", "h").await.unwrap();
    let first = db.add_user_domain(lower.id, "b", "x.com").await.unwrap();
    let second = db.add_user_domain(upper.id, "B", "x.com").await.unwrap();
    assert_eq!(first.subdomain, certo_core::ids::subdomain("b", "x.com", 0));
    assert_eq!(
        second.subdomain,
        certo_core::ids::subdomain("b", "x.com", 1)
    );
}

#[tokio::test]
async fn acmedns_accounts() {
    let db = mem();
    let (username, key, sub) = db
        .create_acmedns_account(vec!["192.0.2.0/24".into()])
        .await
        .unwrap();
    assert!(username.starts_with("acme-"));
    assert_eq!(sub.len(), certo_core::ids::ACMEDNS_SUBDOMAIN_LENGTH);
    let (uid, domain, allow) = db.user_domain_by_subdomain(&sub).await.unwrap();
    assert_eq!(domain, sub);
    assert_eq!(allow, ["192.0.2.0/24"]);
    let api = db.api_key_by_value(&key).await.unwrap();
    assert_eq!((api.user_id, api.name.as_str()), (uid, "acme-dns"));
    assert!(api.is_global());
    assert_eq!(db.user_by_id(uid).await.unwrap().password_hash, "!");
}

#[tokio::test]
async fn api_keys() {
    let db = mem();
    let u = db.create_user("k", "h").await.unwrap();
    let k = db
        .create_api_key(u.id, "ci", vec!["*.example.com".into()])
        .await
        .unwrap();
    assert_eq!(k.key.len(), 32);
    assert_eq!(db.api_key_by_value(&k.key).await.unwrap(), k);
    db.update_api_key_scope(k.id, vec!["example.com".into()])
        .await
        .unwrap();
    assert_eq!(
        db.api_key_by_value(&k.key).await.unwrap().scope,
        ["example.com"]
    );

    let other = db.create_user("other", "h").await.unwrap();
    db.delete_api_key(other.id, k.id).await.unwrap();
    assert!(
        db.api_key_by_value(&k.key).await.is_ok(),
        "other users cannot delete"
    );
    db.delete_api_key(u.id, k.id).await.unwrap();
    assert!(matches!(
        db.api_key_by_value(&k.key).await,
        Err(DbError::NotFound(_))
    ));
}

#[tokio::test]
async fn delete_user_removes_keys_and_domains() {
    let db = mem();
    let u = db.create_user("gone", "h").await.unwrap();
    let d = db.add_user_domain(u.id, "gone", "gone.com").await.unwrap();
    let key = db.list_api_keys(u.id).await.unwrap().remove(0);
    let name = certo_core::internal_domain(&d.subdomain, "auth.example.org");
    db.present_txt(&name, "token").await.unwrap();
    db.delete_user(u.id, "auth.example.org").await.unwrap();
    assert!(db.user_by_id(u.id).await.is_err());
    assert!(db.txt_for_domain(&name).await.unwrap().is_empty());
    assert!(db.subdomain_owner(&d.subdomain).await.is_err());
    assert!(db.api_key_by_value(&key.key).await.is_err());
}

#[tokio::test]
async fn stale_acmedns_accounts_and_expired_txt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("certo.db");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let sql = |q: &str| Connection::open(&path).unwrap().execute_batch(q).unwrap();
    let base = "dns.example.net";
    let alice = db.create_user("alice", "h").await.unwrap();
    db.add_user_domain(alice.id, "alice", "example.com")
        .await
        .unwrap();
    let (used, _, used_sub) = db.create_acmedns_account(Vec::new()).await.unwrap();
    let (unused, _, _) = db.create_acmedns_account(Vec::new()).await.unwrap();
    let (fresh, _, _) = db.create_acmedns_account(Vec::new()).await.unwrap();
    db.present_txt(&format!("{used_sub}.{base}"), "v")
        .await
        .unwrap();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    // Age everything but `fresh` (and alice) by eight days.
    sql(&format!(
        "UPDATE users SET created_at = created_at - 8*24*3600 WHERE username IN ('{used}','{unused}','alice')"
    ));
    let stale = db
        .stale_acmedns_accounts(now - 7 * 24 * 3600, base)
        .await
        .unwrap();
    assert_eq!(
        stale
            .iter()
            .map(|u| u.username.as_str())
            .collect::<Vec<_>>(),
        [unused.as_str()]
    );
    assert!(fresh.starts_with("acme-"));

    // Expiry: values older than the cut-off (or never stamped) go, newer ones stay.
    db.present_txt("old.dns.example.net", "o").await.unwrap();
    sql("UPDATE txt SET LastUpdate = LastUpdate - 3*3600 WHERE Domain='old.dns.example.net';
         INSERT INTO txt (Domain, Value, LastUpdate) VALUES ('nostamp.dns.example.net', 'n', NULL);");
    let mut names = db.expire_txt(now - 2 * 3600).await.unwrap();
    names.sort();
    assert_eq!(names, ["nostamp.dns.example.net", "old.dns.example.net"]);
    assert!(
        db.txt_for_domain("old.dns.example.net")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.txt_for_domain(&format!("{used_sub}.{base}"))
            .await
            .unwrap(),
        ["v"]
    );
    assert!(db.expire_txt(now - 2 * 3600).await.unwrap().is_empty());
}
