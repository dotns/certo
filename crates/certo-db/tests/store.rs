//! Storage behavior, ported from the Go implementation's tests plus migration coverage.
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
    for table in ["zones", "records", "zone_keys", "tsig_keys"] {
        assert!(!columns(&path, table).is_empty(), "{table}");
    }

    // Reopening is a no-op.
    drop(db);
    Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(version(&path), DB_VERSION.to_string());
}

#[tokio::test]
async fn v1_database_is_upgraded_and_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE acmedns(Name TEXT, Value TEXT);
             INSERT INTO acmedns (Name, Value) VALUES ('db_version', '1');
             CREATE TABLE users(id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT UNIQUE NOT NULL, password_hash TEXT NOT NULL, created_at INT NOT NULL);
             CREATE TABLE user_domains(user_id INTEGER NOT NULL, domain TEXT NOT NULL, subdomain TEXT UNIQUE NOT NULL, UNIQUE(user_id, domain));
             INSERT INTO users (username, password_hash, created_at) VALUES ('old', 'h', 1);
             INSERT INTO user_domains (user_id, domain, subdomain) VALUES (1, 'old.com', 'abcd1234');",
        )
        .unwrap();

    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(version(&path), DB_VERSION.to_string());
    let (uid, domain, allow) = db.user_domain_by_subdomain("abcd1234").await.unwrap();
    assert_eq!((uid, domain.as_str()), (1, "old.com"));
    assert!(allow.is_empty());
}

#[test]
fn newer_database_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE acmedns(Name TEXT, Value TEXT);
             INSERT INTO acmedns (Name, Value) VALUES ('db_version', '6');",
        )
        .unwrap();
    let err = Db::open(path.to_str().unwrap()).unwrap_err();
    assert!(err.to_string().contains("regenerate the database"), "{err}");
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

/// `fixtures/go-v2.db` was written by the Go implementation (schema v2): user `gouser` with
/// domain `go.example.com`, a scoped key, one TXT value and one acme-dns account.
#[tokio::test]
async fn reads_and_extends_database_written_by_go() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("go.db");
    std::fs::copy("tests/fixtures/go-v2.db", &path).unwrap();
    let db = Db::open(path.to_str().unwrap()).unwrap();

    let user = db.user_by_username("gouser").await.unwrap();
    assert_eq!(user.password_hash, "$2a$10$abcdefghijklmnopqrstuv");
    let domains = db.user_domains(user.id).await.unwrap();
    assert_eq!(domains[0].domain, "go.example.com");
    assert_eq!(domains[0].subdomain, "f1c667bb");
    assert_eq!(
        domains[0].subdomain,
        certo_core::ids::subdomain("gouser", "go.example.com", 0)
    );

    let key = db
        .api_key_by_value("z3x04v1nxflou3e2hf07k5hk0x8jg5r9")
        .await
        .unwrap();
    assert_eq!(key.scope, ["*.go.example.com", "x.org"]);
    assert!(key.has_domain_access("a.go.example.com"));
    assert_eq!(
        db.txt_for_domain("_acme-challenge.go.example.com")
            .await
            .unwrap(),
        ["go-token"]
    );

    let (uid, domain, allow) = db.user_domain_by_subdomain("hru2zenlwr").await.unwrap();
    assert_eq!(domain, "hru2zenlwr");
    assert_eq!(allow, ["198.51.100.0/24"]);
    let acme = db
        .api_key_by_value("fjj98fj19j5q7upaj9zpgq9xiznj45vt")
        .await
        .unwrap();
    assert_eq!(acme.user_id, uid);

    let added = db.create_user("rustuser", "h").await.unwrap();
    assert!(added.id > user.id);
    assert_eq!(version(&path), DB_VERSION.to_string());
    // The v2 -> v3 upgrade added the zone tables.
    let zone = db
        .create_zone(user.id, "go.example.com", vec![])
        .await
        .unwrap();
    assert_eq!(zone.owner.as_deref(), Some("gouser"));
}
