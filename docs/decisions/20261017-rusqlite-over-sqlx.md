# Use rusqlite instead of SQLx

- **date**: 2026-10-17
- **status**: accepted
- **relatedPlan**: 20261016-1840-authoritative-zones-ptr

## Context

The pma-rust baseline defaults to SQLx (alternatives: SeaORM, Diesel). certo targets SQLite only, ships as a single static binary, and must open existing databases created by the Go implementation, whose schema version lives in `acmedns.db_version` (currently 2).

## Decision

Use `rusqlite` with the `bundled` feature for all database access.

- Keep the existing `acmedns.db_version` upgrade scheme instead of a migration framework, so existing databases open without a bridge step.
- One connection behind a mutex, mirroring the Go implementation's global mutex; `busy_timeout` 5s; blocking calls run in `tokio::task::spawn_blocking`. Add WAL and a read pool only if DNS query load shows contention.
- No compile-time SQL checking; every query is covered by `certo-db` tests.

## Reasons

- SQLx's multi-database abstraction and async API add no value for a SQLite-only server; its SQLite driver is itself backed by a blocking worker thread.
- SQLx compile-time checks are weakest on SQLite (dynamic typing, nullability inference) and require a live DB or a committed `.sqlx/` cache.
- `sqlx migrate` would add `_sqlx_migrations` and require a legacy bridge for v1/v2 databases.
- rusqlite exposes the full SQLite API, including the online backup API used for backing up DNSSEC key material.

## Consequences

- Deviation from the pma-rust database default; no sunset date, revisit only if a non-SQLite backend is ever required.
- Query correctness relies on tests rather than macros.
