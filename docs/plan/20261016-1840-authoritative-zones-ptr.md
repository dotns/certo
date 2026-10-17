# 20261016-1840-authoritative-zones-ptr Authoritative zones with PTR and general records

- **status**: completed
- **createdAt**: 2026-10-16 18:40
- **approvedAt**: 2026-10-17 00:51
- **relatedTask**: 20261016-1840-authoritative-zones-ptr

## Context

Current Go implementation (~4.9k lines incl. tests):

- `pkg/nameserver`: `miekg/dns` handler; answers static config records plus dynamic challenge TXT from the DB. Only one SOA (`general.domain`); no zone model.
- `pkg/database`: SQLite (`modernc.org/sqlite`), tables `acmedns` (db_version), `txt`, `users`, `user_domains`, `api_keys`; schema version 2.
- `pkg/api`: httprouter. Endpoints: httpreq `/present` `/cleanup`; acme-dns `/register` `/update`; lego HTTP storage `/acmedns[/:domain]`; `/health`, `/llms.txt`; dashboard `/api/*` (JWT HS256 or API key); admin `/admin/*` (`X-Admin-Key`). Passwords are bcrypt (cost 10). Auto TLS via certmagic, solving DNS-01 against its own nameserver.
- `web/`: React SPA embedded in the binary.
- `tests/e2e`: Bun + TypeScript black-box suite that builds the binary, drives every HTTP API and checks DNS with `dig`.

Decision (2026-10-17): rewrite the server in Rust and build general zone support on `hickory-server`.

Rust building blocks (versions checked on crates.io 2026-10-17):

- `hickory-server` 0.26.3: `Catalog` routes queries by longest zone suffix to `ZoneHandler` implementations; ships `InMemoryZoneHandler`, `FileZoneHandler`, `SqliteZoneHandler` (RFC 2136 dynamic update with SQLite journal, SIG(0)/TSIG), DNSSEC online signing, AXFR policy. Custom `ZoneHandler` can be implemented for certo's challenge zone.
- `domain` 0.12.3 (NLnet Labs): zonetree is still marked experimental; not chosen.
- HTTP: `axum` 0.8.9; ACME client for auto TLS: `rustls-acme` 0.15.4 or `instant-acme` 0.8.5; SQLite: `rusqlite` 0.40.2 (bundled), `rusqlite_migration` 2.6.0 available but not needed (see decision record).

## Proposal

Replace the Go server with a Rust workspace that follows the pma-rust baseline (edition 2024, `[workspace.lints]` deny-warnings, `#![forbid(unsafe_code)]`, rustls + aws-lc-rs, no unwrap/expect/panic in runtime paths, thiserror per crate, tracing, rusqlite, figment, clap, secrecy, nextest/deny/shear/typos/MSRV gates, `/livez` + `/readyz`, graceful shutdown, panic hook, core dumps off, musl static release). Keep `web/`, `config.toml`, the SQLite file and every HTTP/DNS contract compatible so existing users and lego/acme-dns clients notice nothing.

Decisions (2026-10-17): multi-user zone ownership; pma-rust baseline with rusqlite instead of SQLx (`docs/decisions/20261017-rusqlite-over-sqlx.md`); Bun e2e suite stays as the parity oracle; DNSSEC and AXFR ship in the first release.

1. **Workspace**:
   - `crates/certo-core`: config (same TOML keys, figment: defaults -> file -> env -> CLI), domain types, scope matching, nanoid generation.
   - `crates/certo-db`: `rusqlite` (`bundled`). Keeps the existing schema versioning in `acmedns.db_version`: same `CREATE TABLE IF NOT EXISTS` set and v1 -> v2 upgrade as the Go code, plus v2 -> v3 adding the zone tables (step 5), each upgrade in one transaction. One connection behind a mutex (same serialization as the Go global mutex), `busy_timeout` 5s, default journal mode; all calls run via `spawn_blocking`. Every query is covered by tests (no compile-time SQL checking).
   - `crates/certo-dns`: hickory-server (features `dnssec-aws-lc-rs`, no `sqlite`: not needed, and its `rusqlite ^0.39` pulls `libsqlite3-sys ^0.37`, which conflicts with `rusqlite` 0.40's `^0.38` on the `links = "sqlite3"` key).
   - `crates/certo-api`: axum 0.8, same paths, bodies, status codes and auth (bcrypt verify/hash cost 10, HS256 JWT, Basic Auth, `X-Admin-Key`), utoipa OpenAPI.
   - `crates/certo` bin: wiring, `web/dist` embedded (`rust-embed`), auto TLS via ACME DNS-01 against its own challenge handler (`instant-acme` or `rustls-acme`, chosen in step 4 after a spike).
2. **DNS serving model**:
   - Challenge zone (`general.domain`): custom `ZoneHandler` that serves static config records and reads challenge TXT from the DB on every query (TTL 1), byte-identical to today. Unsigned, same as today.
   - User zones: the DB is the source of truth; each zone is materialized into a hickory `InMemoryZoneHandler` and upserted into the `Catalog` on change (load all at start). A thin `RequestHandler` wrapper holds the catalog behind an `RwLock` and applies per-zone AXFR source-IP allow lists.
3. **Zones and records**: tables `zones(id, user_id, name, serial, soa fields, dnssec_enabled, nsec_mode, axfr_policy, axfr_allow_from, created_at)`, `records(id, zone_id, name, type, ttl, rdata, created_at)`, `zone_keys(id, zone_id, algorithm, private_key, created_at)`, `tsig_keys(id, zone_id, name, algorithm, secret, created_at)`. Record types A, AAAA, CNAME, TXT, MX, NS, PTR, SRV, CAA, parsed/validated by hickory-proto; CNAME exclusivity; owner inside zone; PTR only in `in-addr.arpa`/`ip6.arpa` zones. Serial bumps on each change. Zones equal to or under `general.domain` are rejected.
4. **DNSSEC**: per-zone opt-in; on enable, generate an ECDSAP256SHA256 key, online-sign via `add_zone_signing_key` + `secure_zone`, re-sign on every change; NSEC by default, NSEC3 selectable. API returns the DS record for the user to publish at the parent. Private keys stored in the DB, wrapped in `secrecy`, never logged or returned.
5. **AXFR**: per-zone policy Deny (default) / TSIG-signed (`AllowSigned`) / source-IP allow list; TSIG keys managed per zone via API. NOTIFY to secondaries is out of scope for v1.
6. **API (new)**: user `/api/zones`, `/api/zones/{zone}`, `/api/zones/{zone}/records[/{id}]`, `/api/zones/{zone}/dnssec`, `/api/zones/{zone}/tsig-keys`; admin mirrors under `/admin/zones`. Ownership: a user manages only their zones; API keys need a scope covering the zone name (reuse wildcard/exact matching); a config flag `allow_zone_creation` (default false) gates self-service creation, admin can always create and assign.
7. **Dashboard**: Zones list, record editor, DNSSEC toggle with DS display, AXFR/TSIG settings.
8. **Removal**: Go sources, `go.mod`, `go.sum` and Go build steps are deleted only after the Rust binary passes the full e2e suite.

Phasing, each with its own verify step:

1. Workspace skeleton, lint/CI gates, justfile, spike: hickory catalog upsert at runtime + DNSSEC signing + AXFR with TSIG on `InMemoryZoneHandler` -> verify: gates green; spike test answers signed records and a TSIG AXFR.
2. `certo-db` with baseline + legacy bridge on copies of real v1/v2 DBs -> verify: tests read/write existing data; upgrades v1 -> v3 and v2 -> v3 idempotent.
3. Challenge DNS parity -> verify: e2e DNS assertions pass against the Rust binary.
4. HTTP API parity incl. auto TLS -> verify: full `tests/e2e` suite passes with only its build step switched to cargo.
5. Zones + records (DNS + API) -> verify: new e2e cases (`dig -x` PTR, NXDOMAIN/NODATA with SOA, apex SOA/NS, CNAME, ownership/scope denial, collision rejection).
6. DNSSEC + AXFR -> verify: `dig +dnssec` returns RRSIG, `delv`/`ldns-verify-zone` validates with the DS anchor; AXFR denied by default, allowed with TSIG and from allow-listed IP.
7. Dashboard Zones page -> verify: `vite build` + browser check.
8. Delete Go code; update Dockerfile (multi-arch musl), justfile, README, `docs/*` -> verify: Docker build for amd64/arm64, e2e green.

Build and tests run in a sibling Rust container (no cargo in this environment).

## Risks

- Rewrite regressions in auth/challenge flows; mitigated by keeping `tests/e2e` unchanged as the parity oracle before any Go code is removed.
- Data compatibility: bcrypt hashes, JWT secret, nanoid subdomains, `scope`/`allowfrom` JSON columns must be read identically.
- Auto TLS: certmagic's behavior (storage layout in `acme_cache_dir`) is not reproducible; certificates will be re-issued once after the switch.
- hickory-server is pre-1.0 (0.26); API breaks between minors are likely. Pin and wrap behind `certo-dns`.
- Opening general hosting invites abuse; zone creation off by default.
- DNSSEC mistakes (bad re-sign, key loss) make a zone bogus for validating resolvers; DNSSEC is opt-in per zone and keys are backed up with the DB.
- hickory runtime catalog mutation, TSIG for in-memory zones and signing cost on large zones are unverified; step 1 spike decides before any parity work.
- Reverse zones still need delegation from the IP block owner.

## Scope

Full rewrite of `pkg/*` and `main.go` (~3k lines of Go runtime code) into 5 crates, plus zones, DNSSEC, AXFR/TSIG, Docker/justfile/CI changes, docs. `web/` and `tests/e2e` mostly unchanged (e2e build step switched to cargo; zone cases added).

## Alternatives

- **A. Stay on Go, add zones on `miekg/dns`** (previous draft of this plan): smallest change, no rewrite risk; DNSSEC/AXFR/RFC 2136 would have to be hand-written.
- **B. Rust rewrite on hickory-server (proposed)**: zones, DNSSEC, AXFR, dynamic update available from the library; one-time rewrite cost.
- **C. Rust, DNS only**: keep Go API, run a Rust hickory DNS process sharing the SQLite file; two binaries, cross-process DB locking.

## Annotations

Answered 2026-10-17: multi-user zones; pma-rust standards; rusqlite chosen over SQLx after comparison; keep Bun e2e; DNSSEC + AXFR in v1.

Step 1 spike results (2026-10-17):
- Runtime zone replace/remove works through `DynamicCatalog` (`tokio::sync::RwLock<Catalog>` behind a `RequestHandler`).
- `InMemoryZoneHandler` only honors `AxfrPolicy::AllowAll`; TSIG is implemented only in hickory's `SqliteZoneHandler`. certo wraps the in-memory zone (`CertoZone`) and does TSIG verification (unknown key, bad MAC, BADTIME) and source-IP allow lists itself; signed responses are produced by the catalog from the returned `TSigResponseContext`.
- DNSSEC online signing with ECDSAP256SHA256, NSEC and NSEC3 (RFC 9276 parameters) validated end to end: AXFR of the signed zone passes `ldns-verify-zone`; a tampered PTR is reported as bogus. `delv` 9.20 only accepts root anchors, so step 6 uses `ldns-verify-zone`.
- AXFR omits DNSSEC records unless the request sets the DO bit; secondaries must request with DO.
- `secure_zone_mut` increments the SOA serial on every signing pass.

Step 4 results (2026-10-17):
- ACME client: `instant-acme` (DNS-01 driven by certo itself; `rustls-acme` only does TLS-ALPN-01). Certificates are stored in certmagic's layout (`certificates/<issuer>/<domain>/<domain>.{crt,key}`), so a Go deployment's cache is picked up without re-issuing; the ACME account is stored separately (`acme/<issuer>/certo-account.json`) because certmagic's account format differs.
- Added optional `api.acme_directory` / `api.acme_root_ca` for private ACME CAs; verified against Pebble with real DNS-01 validation.
- The HTTP API lives in `certo-api` (not the bin crate) so it is testable with `tower::oneshot`; the bin only wires DNS, API and TLS.

Step 6 results (2026-10-17):
- Validation uses `ldns-verify-zone` on a TSIG-signed AXFR requested with DO (see step 1: `delv` needs a root anchor).
- The DNSSEC key is deleted when DNSSEC is disabled; re-enabling generates a new key and DS.
- The AXFR allow list also accepts single addresses (stored as /32 or /128).

Step 8 results (2026-10-17):
- Go sources removed; Docker image cross-builds static musl binaries with cargo-zigbuild (see `docs/decisions/20261017-container-base-image.md`).
- Deviation: utoipa OpenAPI was not added; `docs/api.md` and `llms.txt` remain the API reference.
