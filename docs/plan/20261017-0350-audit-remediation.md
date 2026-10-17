# 20261017-0350-audit-remediation Remediate repository audit findings

- **status**: completed
- **createdAt**: 2026-10-17 03:50
- **approvedAt**: 2026-10-17 03:52
- **relatedTask**: 20261017-0350-audit-remediation

## Context

Audit of commit 3636cae (all gates green, 74 tests) found five issues:

1. `crates/certo-dns/src/catalog.rs`: `DynamicCatalog` wraps hickory's `Catalog` in a tokio `RwLock` and awaits the whole `handle_request` (including `send_response`) while holding the read guard. tokio's `RwLock` is write-preferring, so a `publish` waiting for the write lock stalls every new DNS query until the slowest in-flight request (a TCP AXFR at worst) finishes.
2. `crates/certo-dns/src/zones.rs` `publish`: `materialize` + `build_zone` (parsing all records, `secure_zone_mut` signing every RRset) run synchronously on a tokio worker while holding the `publish` mutex. Records per zone are unbounded.
3. User deletion lives in `certo_dns::Zones::delete_user` (deletes keys, domains, zones) and is called from `web::delete_account` and `admin::delete_user`. Only `web::delete_account` clears the user's challenge TXT rows; admin deletion and both `remove_domain` handlers leave TXT rows keyed by `<subdomain>.<base>`. Dashboard subdomains are deterministic (`sha256(username:domain)`), so stale values resurface.
4. `Zones::create` loads all zones and checks overlap, then `Db::create_zone` inserts in a separate `Db::run`; `Zones::checked` reads records and checks CNAME exclusivity, then `Db::add_record`/`update_record` write separately. Both are check-then-write races; `Db::add_user_domain` already does check and insert inside one closure.
5. `web::register` and `admin::create_user` map any `DbError` to `409 username_taken`, although `Db::create_user` distinguishes `DbError::Conflict("user")`.

Call sites touched: `catalog.rs` consumers (`handler.rs`, `zones.rs`, tests in `certo-dns/tests` and `certo-api/tests`), `Db::delete_user` / `remove_user_domain` callers (`web.rs`, `admin.rs`, `certo-db/tests`), `records::validate` callers (`zones.rs`, `records.rs` tests).

## Proposal

1. **Catalog snapshot.** `DynamicCatalog` keeps `std::sync::RwLock<State>` with `State { zones: FxHashMap<LowerName, Arc<dyn ZoneHandler>>, catalog: Arc<Catalog> }`. `upsert`/`remove` mutate the map and rebuild a fresh `Catalog` from it; `handle_request` clones the `Arc<Catalog>` under a short read lock and drops the guard before awaiting. All `DynamicCatalog` methods become synchronous. No new dependency.
2. **Signing off the runtime.** `materialize` becomes a free function over `&ZonePolicy`; `publish` runs `materialize` + `build_zone` in `tokio::task::spawn_blocking`, still under the publish mutex so snapshots stay ordered. New `ZoneOpError::Task` for a failed blocking task; mapped to `500 zone_error` in the API.
3. **Deletion ownership.** `Db::delete_user(user_id, base_domain)` deletes the user's TXT rows (`internal_domain(subdomain, base)`) in the same transaction as keys, domains and zones. `Db::remove_user_domain(user_id, domain, base_domain)` deletes that subdomain's TXT rows with the domain row. `Zones::delete_user` is removed; `Zones::publish` becomes public (rebuilds or unpublishes). A `web::delete_user_for(state, user_id)` helper (zones list → `Db::delete_user` → `publish` each zone) serves both `DELETE /api/profile` and `DELETE /admin/users/{id}`.
4. **Invariants in the transaction.** `Db::create_zone` reads all zones inside its transaction and returns `DbError::Conflict("zone overlap")` when another user's zone is an ancestor or descendant of the new name (string suffix check on the stored lowercase names). `Db::add_record` / `update_record` return `DbError::Conflict("cname")` when a CNAME would share a name with any other record, or any record would join a CNAME (`id <> ?` excludes the record being replaced). `ZoneOpError::from(DbError)` maps these to `NotAllowed("overlaps another user's zone")` and `Record(RecordError::CnameConflict)`. `records::validate` drops its `existing` / `replacing` parameters and in-memory conflict check; `Zones::checked` no longer loads records; `Zones::check_new_zone` keeps only the base-domain and static-record rules.
5. **Error mapping.** Both registration handlers map `DbError::Conflict(_)` to 409 and everything else to `ApiError::db` (500, logged).

Tests (RED first): DB tests for TXT cascade on `delete_user` and `remove_user_domain`, DB-level overlap and CNAME rejection, API test asserting admin user deletion clears TXT rows; `serving.rs` / `hosted.rs` / `api.rs` updated for the synchronous catalog and the moved deletion orchestration; `records.rs` unit test loses the conflict cases (now covered in `certo-db`).

## Risks

- Behavior change: `DELETE /admin/users/{id}` and `DELETE /api/domains/{domain}` / `/admin/domains/{domain}` now delete challenge TXT rows. Intended; documented in `docs/architecture.md`.
- `validate` signature change is internal to the workspace (`certo-dns` is `publish = false`).
- Catalog rebuild is O(zones) per publish; negligible at expected scale.

## Scope

`crates/certo-dns/src/{catalog,handler,zones,records}.rs`, `crates/certo-db/src/{store,zones}.rs`, `crates/certo-api/src/{web,admin,zones}.rs`, tests in all three crates, `docs/architecture.md`, `docs/changelog.md`.

## Alternatives

- `arc-swap` instead of `std::sync::RwLock<Arc<Catalog>>`: lock-free reads, but a new dependency for a lock that is held for a pointer clone.
- Generic check closure passed into `Db::create_zone` instead of string suffix logic in `certo-db`: more flexible, but the invariant is a storage rule and `add_user_domain` already follows the in-closure pattern.
- Keeping the in-memory CNAME check in `validate` as well as the DB check: duplicates one rule in two places; rejected.

## Annotations

User instruction "审核的问题全部处理" (handle all audit findings) taken as up-front approval of this scope. The optional in-memory `txt` copy discussed separately is not included.
