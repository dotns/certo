# Changelog

## 2026-10-17 00:31 [decision]

Plan 20261016-1840-authoritative-zones-ptr revised: zone/PTR support moves from a Go extension on `miekg/dns` to a Rust rewrite on `hickory-server` + `axum`, keeping all HTTP/DNS contracts and the SQLite data compatible. The Go-only approach is retained as Alternative A.

## 2026-10-17 00:34 [decision]

Plan 20261016-1840-authoritative-zones-ptr: multi-user zone ownership; pma-rust baseline with SQLx; Bun e2e suite kept as parity oracle; DNSSEC and AXFR (TSIG / IP allow list) included in the first release. Added a step-1 spike to validate hickory runtime catalog updates, DNSSEC signing and TSIG AXFR.

## 2026-10-17 00:38 [decision]

Plan 20261016-1840-authoritative-zones-ptr: database layer switched from SQLx to rusqlite, keeping the existing `acmedns.db_version` upgrade scheme. Recorded in `docs/decisions/20261017-rusqlite-over-sqlx.md`.

## 2026-10-17 01:00 [progress]

Step 1 of plan 20261016-1840-authoritative-zones-ptr: Rust workspace (`crates/certo-{core,db,dns,api}`, `crates/certo`) with pma-rust lint policy, nextest/deny/shear/typos/MSRV (1.88) gates, CI `rust` and `rust-msrv` jobs, `just rs-ci`. `certo-dns` provides `DynamicCatalog` and `build_zone` (in-memory zone + DNSSEC signing + TSIG/IP AXFR access); 7 socket-level tests.

## 2026-10-17 01:00 [pitfall]

hickory-server 0.26 `InMemoryZoneHandler` ignores `AxfrPolicy::AllowSigned` (refuses everything except `AllowAll`); TSIG must be verified by a wrapping `ZoneHandler`. AXFR also strips DNSSEC records unless the request has the DO bit.

## 2026-10-17 01:07 [progress]

Step 2 of plan 20261016-1840-authoritative-zones-ptr: `certo-core` (nanoid/API key/acme-dns username generators, deterministic subdomain matching the Go SHA-256 derivation, API key scope matching, domain types) and `certo-db` (rusqlite port of every Go storage method, v1 -> v2 upgrade, newer-version rejection). Compatibility verified both ways: `tests/fixtures/go-v2.db` written by the Go code is read and extended by Rust, and a Rust-written database is read back by the Go code. Simplified from the plan: single mutex-guarded connection instead of WAL + read pool; zone tables (v3) move to step 5.

## 2026-10-17 01:07 [pitfall]

`clippy::redundant_pub_crate` (nursery) conflicts with rustc `unreachable_pub`; allowed workspace-wide.

## 2026-10-17 01:15 [progress]

Step 3 of plan 20261016-1840-authoritative-zones-ptr: `certo-dns` `ChallengeResponder` reproduces the Go nameserver (static records lowercased with default TTL 3600, single base-domain SOA, DB TXT with TTL 1 for any name, own ACME token, NXDOMAIN+SOA only when authoritative, EDNS 512 / BADVERS); `DnsHandler` routes names inside user zones to the zone catalog and everything else to the responder. `certo-core` config reads the Go TOML layout (plus `PORT` and `CERTO_<SECTION>__<KEY>` overrides). `certo` binary: `-c`, logging (text/json, stdout/file), umask 077, core dumps off, panic hook, graceful shutdown. Differential check against the Go binary (same config and DB, 25 names x UDP/EDNS+DO/TCP/EDNS1) produced identical dig output.

## 2026-10-17 01:15 [pitfall]

hickory-proto decodes extended rcode 16 as `BADSIG` (shared with `BADVERS`); compare wire values in tests. The Go server answers NXDOMAIN for non-TXT queries on names that only have challenge TXT values; kept for parity.

## 2026-10-17 01:45 [progress]

Step 4 of plan 20261016-1840-authoritative-zones-ptr: `certo-api` (axum 0.8 + axum-server) ports every Go route: httpreq `/present` `/cleanup`, acme-dns `/register` `/update` and `/acmedns` storage, `/api/*`, `/admin/*`, `/health`, `/llms.txt`, SPA serving (`web/dist` override, then embedded files), rs/cors-equivalent CORS, 64 KiB body cap, 5s header read timeout, plus `/livez` and `/readyz`. TLS modes: `none`, `cert` (PEM files) and `letsencrypt`/`letsencrypt_staging` via instant-acme DNS-01 answered by certo's own DNS, renewing at 1/3 remaining lifetime and reading/writing certmagic's cache layout so certificates issued by the Go server are reused. New optional config keys `api.acme_directory` and `api.acme_root_ca` (private ACME CAs; used to test against Pebble). Verified: unchanged `tests/e2e` (32 cases) passes against the Rust binary; differential run of 77 HTTP requests against the Go binary shows no differences beyond random identifiers and the version string; Pebble issued a certificate through real DNS-01 validation, served over TLS 1.3/h2 and reused from cache after restart.

## 2026-10-17 01:45 [pitfall]

`MockConnectInfo` is only visible through the `ConnectInfo` extractor, not as a `ConnectInfo` request extension. hickory-server logs every request at info level; the logging filter caps `hickory_server` at warn unless `loglevel = "debug"`.

## 2026-10-17 02:20 [progress]

Step 5 of plan 20261016-1840-authoritative-zones-ptr: hosted zones. Schema v3 adds `zones`, `records`, `zone_keys`, `tsig_keys` (fresh DBs and v1/v2 upgrades, one transaction each); after the upgrade the Go binary can no longer open the database. `certo-dns` `Zones` service keeps the database as the source of truth and rebuilds a zone into the catalog on every change (serialized, so a stale snapshot never wins), loads all zones at startup, and enforces: zone names need two labels, may not be at or under `general.domain`, may not cover static config records, and may not overlap another user's zone. Records: A, AAAA, CAA, CNAME, MX, NS, PTR, SRV, TXT; owners relative (`www`, `@`) or absolute; RDATA names always absolute; PTR only in reverse zones; CNAME not at the apex and exclusive at its name; TTL default 3600, max 2^31-1; TXT stored quoted/escaped and long strings split at 255 bytes. New zones get an apex NS for `general.nsname`; the SOA is derived from `nsname`/`nsadmin` with a unix-time serial bumped on every change. API: `/api/zones[/{zone}[/records[/{id}]]]` (owner, scope-checked, other users' zones answer 404) and the same under `/admin/zones`; self-service creation behind `api.allow_zone_creation` (default false); errors carry an optional `message`. Account and admin user deletion stop serving the user's zones. Verified: 70 Rust tests; e2e grows to 36 cases (dig -x PTR, NXDOMAIN/NODATA with SOA, apex SOA/NS, CNAME chains, update/delete visible in DNS, ownership/scope/collision rejection, admin assignment).

## 2026-10-17 02:20 [pitfall]

hickory-proto's TXT `Display` concatenates the character-strings without quotes, so it does not round-trip; certo renders TXT itself. hickory lowercases names when parsing RDATA.

## 2026-10-17 03:00 [progress]

Step 6 of plan 20261016-1840-authoritative-zones-ptr: DNSSEC and AXFR. Per-zone DNSSEC opt-in (`PUT /api/zones/{zone}/dnssec {enabled, nsec_mode}`, `nsec` or `nsec3` with RFC 9276 parameters): one ECDSAP256SHA256 key generated on enable (changing `nsec_mode` keeps it, disabling deletes it), online signing on every publish, all signed zones re-signed every 24h with a serial bump (RRSIG validity 14 days); `GET` returns the DNSKEY and the SHA-256 DS for the parent. AXFR is refused unless the source address is in `allow_from` (`PUT /api/zones/{zone}/axfr`, CIDRs or single addresses) or the request is signed with one of the zone's TSIG keys (`POST /api/zones/{zone}/tsig-keys {name, algorithm}`, hmac-sha256/384/512; the base64 secret is returned only in that response; `DELETE .../tsig-keys/{name}`). All per-zone routes are served under both `/api/zones` and `/admin/zones` by one handler set. Verified: 74 Rust tests; e2e grows to 38 cases (`dig +dnssec` returns RRSIG/NSEC, DNSKEY at the apex, a TSIG-signed AXFR with DO passes `ldns-verify-zone`, AXFR refused by default, allowed with TSIG and from an allow-listed address, rejected with a wrong secret). CI e2e job installs `ldnsutils`.

## 2026-10-17 03:20 [progress]

Step 7 of plan 20261016-1840-authoritative-zones-ptr: dashboard Zones tab (`web/src/pages/Zones.tsx`): zone list with serial and DNSSEC state, create (shown only when `/api/info` reports `allow_zone_creation`, new field) and delete; record editor (add/edit/delete, type-specific placeholders, PTR preselected in reverse zones); DNSSEC enable/disable and NSEC/NSEC3 switch with copyable DS and DNSKEY; AXFR allow list and TSIG keys (secret and a ready `dig ... AXFR -y` line shown once after creation). API errors now surface the server's `message` when present. `CopyBtn`/`Modal` moved to `web/src/ui.tsx`. Verified: `vite build` and eslint clean; Playwright run against the Rust binary covering register, reverse-zone creation, PTR add/edit, DNSSEC enable, TSIG key creation, allow list save and the error banner, with the edited PTR answered and signed over DNS.

## 2026-10-17 03:50 [progress]

Step 8 of plan 20261016-1840-authoritative-zones-ptr: Go implementation removed (`main.go`, `umask_unix.go`, `pkg/`, `web/embed.go`, `go.mod`, `go.sum`). Dockerfile builds a static musl binary with `cargo-zigbuild` for amd64 and arm64 on the build platform, runtime image unchanged (Alpine + bind-tools + ca-certificates; `docs/decisions/20261017-container-base-image.md`). `CERTO_VERSION` at build time sets the reported version (Docker passes the git tag). justfile, CI (Go job dropped, e2e builds with cargo), e2e harness (`cargo build -p certo`), README/README_CN, `docs/*`, `llms.txt` and the served `llms-api.txt` describe the Rust build and the zone API; sample `config.toml` gains `allow_zone_creation`. Verified: all Rust gates green (74 tests), e2e 38/38 with the harness building via cargo, amd64 image serves SPA/API/DNS with a static 12 MB binary, arm64 build produces a static AArch64 executable (runtime stage not run locally: no arm64 emulation on this host). Not done from the plan: utoipa OpenAPI generation; the API stays documented in `docs/api.md` and `llms.txt`.

## 2026-10-17 03:50 [pitfall]

cargo-zigbuild 0.20 cannot link `aarch64-unknown-linux-musl` with current rustc (`unsupported linker arg: --fix-cortex-a53-843419`); 0.23.4 works. `docker buildx --target backend --output type=local` exports the whole build-stage filesystem, not just the binary.

## 2026-10-17 04:10 [progress]

Plan 20261017-0350-audit-remediation: fixes for the five findings of the repository audit. `DynamicCatalog` now keeps an immutable hickory `Catalog` snapshot behind a `std::sync::RwLock` and rebuilds it on every change, so DNS requests never await while holding a lock and a publish never stalls queries; all catalog methods are synchronous. `Zones::publish` runs record parsing and DNSSEC signing in `spawn_blocking` (still serialized by the publish mutex) and is public. User deletion is orchestrated by `certo-api` (`delete_user_for`, used by `DELETE /api/profile` and `/admin/users/{id}`): `Db::delete_user` and `Db::remove_user_domain` take the base domain and delete the challenge TXT rows of the affected subdomains in the same transaction; `Zones::delete_user` is gone. Zone overlap with another user and CNAME exclusivity are enforced inside the `certo-db` transactions (`DbError::Conflict("zone overlap")`, `Conflict("cname")`), replacing the check-then-write in `Zones`; `records::validate` lost its `existing`/`replacing` parameters. Registration maps only `DbError::Conflict` to `409 username_taken`; other storage errors are logged `500 db_error`. Verified: fmt, clippy, 77 Rust tests, doc tests, cargo deny, shear, typos, MSRV check, e2e 38/38 against the debug binary.

## 2026-10-17 04:35 [progress]

Plan 20261017-0403-lode-self-update: certo is updated through the lode supervisor instead of a certo-published image. `.github/workflows/release.yml` (replacing `docker.yml`) builds static musl binaries for amd64 and arm64 from the new `binary` stage of the `Dockerfile` and publishes `certo-linux-{amd64,arm64}.tar.gz` plus `certo-<tag>.tar.gz` (sample `config.toml`, `lode.toml`, `docker-compose.yml`) as a GitHub release; no Docker image is pushed any more and the binaries are not signed (owner decision: GitHub's sha256 digests are the integrity check, `require_signature = "auto"`). New `lode.toml` runs certo under `bkhq/lode:v0.1.0` with `workdir = "/srv/lode"` so the sample config's relative paths land in the persistent `lode/data/`; operators choose `policy = off | check | auto`. `docker-compose.yml`, both READMEs, `docs/deployment.md` and `docs/development.md` describe the layout, policies, `lode-cli status/update/rollback` and the release process. No Rust change. Verified locally: both binaries exported through `--target binary` (static, stripped, `certo --version` reports the tag), certo under `bkhq/lode:v0.1.0` against a sibling HTTP manifest served DNS and `/api/info`, kept its database under `lode/data/db`, auto-installed 0.1.1 within one check interval (an acme-dns account registered before the update still worked), and refused a 0.1.2 entry with a wrong digest while staying on 0.1.1.

## 2026-10-17 04:35 [pitfall]

The lode image tags are `v0.1.0` (`bkhq/lode:0.1.0` does not exist). `docker run busybox` on this host pulls the arm64 variant and fails with `exec format error`; pass `--platform linux/amd64`.

## 2026-10-17 05:25 [progress]

Task 20261017-0515-ci-spa-build: the CI `e2e` job now builds the dashboard (Node 24, `npm ci && vite build`) before `cargo build`, so the tested binary embeds the SPA like a release and `GET /register` serves the signup page instead of 405 (the one failure of run 39003491987). Both workflows run on `ubuntu-26.04`, use `dtolnay/rust-toolchain@v1`, and the toolchain moves to Rust 1.99.1 (`rust-toolchain.toml`, CI); the Dockerfile's frontend stage moves to `node:24-alpine`. Verified locally: dashboard build under `node:24-alpine`; fmt, clippy, 77 tests and the MSRV check on Rust 1.99.1.

## 2026-10-17 12:40 [pitfall]

A multi-platform `docker buildx build` runs the amd64 and arm64 `backend` stages in parallel; with plain `--mount=type=cache` both cargo processes share `/usr/local/cargo/registry` and `/build/target` and race (`failed to unpack package ... File exists`). `sharing=locked` on both mounts serializes them (first release run of v0.0.1 failed this way).

## 2026-10-18 04:45 [decision]

Direction change (owner): PowerDNS Authoritative becomes the DNS data plane; certo starts and configures a bundled static `pdns_server` and keeps only the management layer (users, keys, challenge protocols, zone ownership, dashboard, TLS). Task 20261018-0429-system-zone closed as superseded; plan 20261018-0445-pdns-control-plane drafted.

## 2026-10-18 04:45 [progress]

Spike for plan 20261018-0445-pdns-control-plane: fully static `pdns_server` 5.0.7 (gsqlite3 built in, no dynamic modules, 12.4 MB) builds on Alpine 3.22/musl in ~1 min (32 cores). Smoke test against the API: zone create, TXT rrset REPLACE with TTL 1 and two values, DELETE, CSK cryptokey with DS, TSIG key, `ALLOW-AXFR-FROM`/`TSIG-ALLOW-AXFR` metadata, clean SIGTERM. Build files kept at `.tmp/pdns-static/` (`Dockerfile.ok`, `smoke.sh`) until step 1 moves them into the repository.

## 2026-10-18 04:45 [pitfall]

Static PowerDNS: libtool drops `-static` from `LDFLAGS`; configure with `LDFLAGS=-static` (so its link tests pass) and then run `make LDFLAGS="-all-static -static-libgcc -static-libstdc++"`. Alpine needs `boost1.84-static`, `openssl-libs-static`, `sqlite-static`, `zlib-static`; `luajit-dev` already ships the static archive. LUA records pull in libcurl, so `--disable-lua-records`; `--without-libsodium --without-libdecaf --disable-systemd --disable-unit-tests --disable-ixfrdist`. `soa_edit_api` must be `DEFAULT`, not `INCEPTION-INCREMENT` (that is a SOA-EDIT value; pdns logs it as unknown).

## 2026-10-18 05:05 [decision]

Plan 20261018-0445-pdns-control-plane revised (owner): production is rebuilt once from the old database, so the schema 3 → 4 import is a one-shot maintenance step and the DNSSEC DS change is accepted. `api.allow_zone_creation` is replaced by zone activation: anyone may create a zone, it is served only after certo verifies the parent delegation points at `general.nsname` (re-checked daily, deactivated after 7 failed days), and admins can exempt a zone from the check.

## 2026-10-18 05:20 [decision]

Plan 20261018-0445-pdns-control-plane approved (owner): rrset-shaped record API accepted; arm64 assets suspended until the static pdns build is verified on a native arm runner; the dashboard is rewritten on the `/pma-web` baseline (TanStack Router/Query, shadcn/ui base-nova + Base UI, Tailwind, Zustand, Vitest, bun, nsl) with an admin area.

## 2026-10-18 07:10 [progress]

Plan 20261018-0445-pdns-control-plane steps 2-4: `crates/certo-pdns` (supervisor with restart backoff and SIGTERM/SIGKILL shutdown, generated `pdns.conf` with a per-start API key, API client, rrset validation, delegation checker that asks the parent's authoritative servers non-recursively, `Zones` service with base zone, challenge TXT sync/reconcile, activation cycle and legacy import; `mock` feature with an in-memory pdns API and a scripted checker for dependent crates). Schema 4 (`zones` with `active/exempt/verified_at/last_check/last_error/failed_checks/snapshot`; v3 record tables dropped after the one-shot import). `certo-api`: `/rrsets` GET/PATCH, `/verify`, `/admin/zones/{zone}/activation`, zone views with `status`/`exempt`/`system`/`ns_target`, challenge writes synced to pdns, `readyz` pings pdns, `api.allow_zone_creation` removed. `main.rs` starts pdns before the HTTP server, runs the hourly check loop and exits when pdns keeps crashing. `crates/certo-dns` and `hickory-server` removed. `just rs-ci` green (72 tests). Step order: e2e adaptation (6) runs before the dashboard rewrite (5) so the API is validated against the real pdns first.

## 2026-10-18 07:10 [pitfall]

PowerDNS creates a SOA for every new zone and rejects TXT content that is not quoted; certo quotes plain TXT values on `PATCH /rrsets` and returns them as pdns presents them (quoted). A test that binds an "unused" port and drops the listener races with other tests' ephemeral ports under nextest; keep the listener and close accepted connections instead.

## 2026-10-18 07:40 [progress]

Plan 20261018-0445-pdns-control-plane step 6: e2e suite on the real static `pdns_server` (42 cases, 11 files, 7 s). Helpers start certo with `[pdns] dir/api_port` and `[zones] resolver` pointed at pdns itself (unknown names are refused, so every delegation check is inconclusive and zones stay pending until the admin exempts them); the server object gained `dir` and `restart()`. New cases: pending zone refuses rrsets (`409 zone_not_active`), `verify` reports `503 check_failed`, admin exemption serves the zone and the base zone cannot be de-exempted or deleted, names outside every zone answer `REFUSED`, NSEC3 switch, TSIG key deletion revokes transfers, and a restart test that wipes `pdns.sqlite3` and checks the base zone and challenge TXT are rebuilt from certo's database. Generated `pdns.conf` now sets `allow-axfr-ips=` (pdns allows AXFR from localhost by default), `default-api-rectify=yes` and `security-poll-suffix=`.

## 2026-10-18 07:40 [pitfall]

The PowerDNS packet cache keeps serving `REFUSED` for a zone created after a negative query (`cache-ttl`, 20 s); zone creation through the API does not purge it. certo calls `PUT /cache/flush?domain=<zone>` after creating a zone. The API returns one DS per digest type (SHA-256 and SHA-384), so `ds` is a list. The sibling Rust image now ships `bun` so the e2e suite can run there (the station glibc is too old for binaries built in the image).

## 2026-10-18 06:40 [progress]

Plan 20261018-0445-pdns-control-plane steps 5 and 7, task completed. Dashboard rewritten on the pma-web baseline (`web/`: Vite 8, React 19, TanStack Router file routes + Query, Zustand session store, shadcn/ui `base-nova` on `@base-ui/react`, Tailwind 4, Vitest, antfu ESLint, bun): pages for domains, API keys, client snippets, zones (list with status badges, create, detail with rrset table, DNSSEC card with DS/DNSKEY, AXFR allow list and TSIG keys with one-time secret) and an admin area at `/manage` (admin key kept in `sessionStorage`; users, every zone incl. the base zone with the exemption switch, domains, TXT records). Gates: `bun run lint`, `typecheck`, `test:coverage` (41 cases, 99 % lines on `features/**` and `shared/lib/**`), `build`; every page exercised with Playwright against the debug binary (register → domain → key → zone pending → admin exemption → records → DNSSEC → TSIG → dark mode, no console errors). `Dockerfile` frontend stage, CI (new `web` job, e2e job) and `justfile` moved from npm to bun. Docs: `architecture.md`, `deployment.md`, `development.md`, `api.md`, `configuration.md`, both READMEs, `llms.txt`, sample `config.toml`, decision record `docs/decisions/20261018-powerdns-data-plane.md`. e2e 42/42 in the sibling image. Release `v0.1.0` is the owner's tag after commit.

## 2026-10-18 06:40 [pitfall]

The dashboard's admin area cannot live at `/admin`: `certo_api::app` routes every `/admin/*` request to the API router, so the SPA page answered a JSON 404 — it is `/manage` now. TypeScript 7.0 is not supported by typescript-eslint yet (antfu config fails to load) and drops `baseUrl`; `web/` pins `typescript` 6.0.3 until typescript-eslint supports TS 7 (recorded deviation from latest-stable). `react-refresh/only-export-components` is switched off for `src/app/routes/**` because TanStack route files must export `Route` next to their component (`allowExportNames` does not cover the local component). The e2e DNSSEC assertion compared the answer section in order; PowerDNS may return RRSIG before A, so it sorts now.
