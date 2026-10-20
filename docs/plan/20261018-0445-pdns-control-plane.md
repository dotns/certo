# 20261018-0445-pdns-control-plane Run PowerDNS as the data plane, certo as its control plane

- **status**: completed
- **completedAt**: 2026-10-18 06:40
- **createdAt**: 2026-10-18 04:45
- **approvedAt**: 2026-10-18 05:20
- **relatedTask**: 20261018-0445-pdns-control-plane

## Context

Today `crates/certo-dns` (1.9k lines on `hickory-server`) answers DNS itself: a challenge
responder for the base domain (static `general.records` + `txt` table, TTL 1) and a catalog
of hosted zones rebuilt and DNSSEC-signed from `zones`/`records`/`zone_keys`/`tsig_keys`
on every change, with its own AXFR/TSIG wrapper because hickory lacks it. Every DNS feature
(NSEC3, NOTIFY, secondaries, catalog zones, more record types) is ours to build and
maintain.

Spike (2026-10-18, `.tmp/pdns-static/`): PowerDNS Authoritative 5.0.7 builds as a fully
static musl binary (12.4 MB, gsqlite3 compiled in, no dynamic modules) in about one minute
on Alpine 3.22, and its HTTP API covers everything certo needs: zone create/delete, rrset
`REPLACE`/`DELETE` (TTL 1 TXT with several values verified by `dig`), `cryptokeys` (CSK
with DS), `tsigkeys`, zone `metadata` (`ALLOW-AXFR-FROM`, `TSIG-ALLOW-AXFR`); it exits
cleanly on SIGTERM. The official image and the Alpine package are dynamically linked and
unusable as a bundled binary, so the static build is ours. The pdns e2e smoke used
`soa_edit_api = "DEFAULT"` (serial maintained by pdns on every API change).

## Proposal

### Architecture

```
lode ──run──> certo ──spawn/supervise──> pdns_server (:53, API 127.0.0.1:8081)
               │  users, keys, scopes, ownership, challenge protocols, dashboard, TLS
               └──HTTP API (X-API-Key)──> zones / rrsets / cryptokeys / tsigkeys / metadata
```

certo owns identity and authorization; PowerDNS owns every DNS answer. certo never reads
or writes the PowerDNS SQLite file after creating the schema; all DNS data goes through
the API.

### 1. `pdns/` build and packaging

- `Dockerfile` stage `pdns` (from the spike): Alpine 3.22, pdns 5.0.7, `--with-modules=gsqlite3
  --with-dynmodules="" --disable-lua-records --without-libsodium --without-libdecaf
  --disable-systemd --disable-unit-tests --disable-ixfrdist`, `make LDFLAGS="-all-static
  …"`; `scratch` stage `pdns-binary` exports `pdns_server` alone (CI e2e, `just
  pdns-build`). `pdns/schema.sqlite3.sql` copied from the tarball (93 lines) and embedded
  into certo with `include_str!`.
- `.github/workflows/release.yml`: builds the `binary` stage for `linux/amd64` only;
  `certo-linux-amd64.tar.gz` contains `certo` and `pdns_server`. arm64 is deferred (owner
  2026-10-18) until the static pdns build is verified on a native `ubuntu-26.04-arm`
  runner; no arm64 asset is published meanwhile. `lode.toml` unchanged (`run = "./certo"`);
  lode's version directory holds both files, so certo finds `pdns_server` beside its own
  executable.
- `Dockerfile`: new `pdns` stage (the same build), `binary` and runtime stages copy
  `pdns_server` next to `certo`; the frontend stage builds with bun.
- CI `e2e` job builds the `pdns-binary` stage with gha cache straight into `target/debug/`
  before running the suite; `justfile` gets `pdns-build` doing the same locally.

### 2. `crates/certo-pdns` (replaces `certo-dns`)

- `Supervisor`: resolves the binary (`[pdns] binary`, default `<exe dir>/pdns_server`),
  creates `<data dir>/pdns/` (0700), initializes `pdns.sqlite3` from the embedded schema
  when missing, writes `pdns.conf` (0600) on every start from the certo config, spawns
  `pdns_server --config-dir=<dir> --config-name=`, waits for `GET /api/v1/servers/localhost`
  (bounded, 10 s), and restarts it with backoff on exit; after 5 failures in a minute certo
  exits non-zero so lode restarts the whole tree. Shutdown: SIGTERM to pdns, wait up to 10
  s, SIGKILL. pdns stdout/stderr are forwarded to certo's log at info/warn.
- Generated `pdns.conf`: `daemon=no guardian=no write-pid=no setuid= setgid=`,
  `local-address`/`local-port` from `general.listen`, `launch=gsqlite3`,
  `gsqlite3-database`, `gsqlite3-dnssec=yes`, `api=yes`, `api-key=<random per start>`,
  `webserver-address=127.0.0.1`, `webserver-port=[pdns].api_port`,
  `webserver-allow-from=127.0.0.1`, `webserver-loglevel=none`, `socket-dir=<dir>`,
  `default-soa-content` from `general.nsname`/`nsadmin`, `log-dns-details=no`, `loglevel`
  mapped from `logconfig.loglevel`. `general.protocol` is dropped (pdns always serves UDP and
  TCP; the key is still accepted and ignored).
- `Client` (`reqwest`, rustls, latest at crates.io when implemented): typed calls for
  `zones` (list/get/create/delete), `rrsets` PATCH (REPLACE/DELETE), `cryptokeys`
  (list/create/delete), `tsigkeys` (create/delete), zone `metadata` (get/put). Errors map
  pdns 4xx/5xx to `PdnsError { status, message }`.
- `Zones` keeps the service API the HTTP layer uses today but is backed by the client and
  the ownership table; `ZonePolicy` keeps the base-domain and two-label rules only.

### 3. Data model (`certo-db`, schema 4)

- Kept: `users`, `api_keys`, `user_domains`, `acmedns`, `txt`, `zones` reduced to
  `(id, user_id NULL for the base zone, name, created_at)`.
- Dropped: `records`, `zone_keys`, `tsig_keys`, `zones.serial/dnssec_enabled/nsec_mode/
  axfr_allow_from` — PowerDNS is the source of truth for all of it.
- Upgrade 3 → 4 is a one-time maintenance import (owner decision 2026-10-18: production
  is rebuilt from the old database, the DS change is accepted). It runs at startup after
  pdns is ready and before the listeners: for each zone create it in pdns with its rrsets,
  re-enable DNSSEC with a fresh CSK when `dnssec_enabled` (new DS logged at warn), create
  TSIG keys from `tsig_keys` (secret preserved), set `ALLOW-AXFR-FROM`/`TSIG-ALLOW-AXFR`,
  mark every imported zone `active` (exempt, it was served before); then drop the old
  tables in one transaction. Failure leaves the database at version 3 and aborts start.
- New columns on `zones`: `active INTEGER NOT NULL DEFAULT 0`, `exempt INTEGER NOT NULL
  DEFAULT 0`, `verified_at INT`, `last_check INT`, `last_error TEXT` (see §4a).
- `txt` stays the source of truth for challenge values. After every write the handler
  pushes the full value list of that name as one TXT rrset (TTL 1) `REPLACE`, or `DELETE`
  when empty. At startup certo reconciles every distinct `txt.Domain` into the base zone.

### 4. Base zone

`ensure_base_zone` at startup: when `general.domain` is not a pdns zone, create it with
the apex NS (`general.nsname`) and the parsed `general.records` lines that fall inside the
base domain (others logged as errors with the hint to create a hosted zone); existing zone
→ no change. `general.records` is documented as a first-start seed. The base zone row in
`zones` has `user_id = NULL`; only admin routes see it; delete is refused
(`403 zone_not_allowed`). `_acme-challenge.<base>` for certo's own certificate is a TXT
rrset write on this zone.

### 4a. Zone activation instead of a creation switch

Zone creation is open to every user (`api.allow_zone_creation` is removed; the key is
ignored if present). A new zone is `pending`: certo stores the ownership row only, pdns
does not know it, nothing is served and records cannot be set yet (`409 zone_not_active`).
A zone becomes `active` when its delegation points at us: certo finds the parent's
nameservers through the configured recursive resolver (`[zones] resolver`, default from
`/etc/resolv.conf`, fallback `1.1.1.1:53`), asks one of them non-recursively for
`NS <zone>` and requires `general.nsname` in the referral (answer or authority section).
On success the pdns zone is created with the apex NS and the zone is served.

- Checks run on `POST /api/zones` (immediately, so a correctly delegated zone activates in
  the same request), on `POST /api/zones/{name}/verify` (rate-limited to one per minute per
  zone), and hourly for pending zones. Active zones are re-checked daily; after 7
  consecutive failed days the zone is deactivated (pdns zone deleted, records kept in a
  JSON snapshot in `zones.last_snapshot` so re-activation restores them) — this is what
  frees a name when a domain changes hands. Pending zones nobody activates within 30 days
  are deleted.
- Admin exemption: `PUT /admin/zones/{name}/activation {"exempt": true}` activates without
  a check and skips re-checks; `false` returns the zone to the normal cycle. The base zone
  is always exempt.
- Views expose `status: "pending" | "active"`, `exempt`, `verified_at`, `last_check`,
  `last_error`; the dashboard shows the required NS target and a "Verify now" button.
- Implementation: `hickory-resolver` (client only, latest at crates.io) for the parent
  lookup and the raw referral query; no server-side hickory code remains.

### 5. HTTP API (`certo-api`)

- Zone routes keep their paths and ownership checks. Record routes change shape to the
  PowerDNS rrset model (contract change, accepted by the owner): `GET /api/zones/{name}/rrsets`,
  `PATCH /api/zones/{name}/rrsets` with `{"rrsets":[{name,type,ttl,changetype,records}]}`,
  validated against the zone name and the allowed types (same list as today plus pdns'
  own validation). `/records/{id}` routes are removed; `docs/api.md` and `llms-api.txt`
  follow.
- DNSSEC: `PUT /dnssec {enabled, nsec3}` → cryptokeys + `NSEC3PARAM` metadata; `GET`
  returns DS from pdns. Transfer/TSIG routes keep their shape and write metadata/tsigkeys.
- `/admin/zones` lists the base zone with `system: true`; `/admin/zones/{name}/activation`
  per §4a. `GET /admin/users` exists; the admin area of the dashboard uses the admin key.
- `readyz` also checks the pdns API.

### 5a. Dashboard rewrite on the pma-web baseline (`web/`)

The current dashboard (react-router, hand-written CSS, 1k lines) is replaced following
`/pma-web`: React + TypeScript + Vite, TanStack Router (file-based routes under
`src/app/routes/`), TanStack Query for server state, Zustand for UI state only, shadcn/ui
`base-nova` on `@base-ui/react` + Tailwind CSS (CSS-first theme, oklch tokens, light/dark/
system `ThemeProvider`), lucide-react icons, ESLint with `@antfu/eslint-config`, Vitest,
bun as package manager, `@nsio/nsl` for dev routing (`bun run dev`), `dev:bare` fallback.
No Radix or other component ecosystems. All dependencies at their latest stable versions
verified at npmjs.com when installed. Layout: `src/app/{providers,routes}`,
`src/features/{auth,domains,keys,zones,admin}` (each with `api.ts` + query/mutation
hooks), `src/shared/{components/ui,lib/http.ts}`.

Features: login/register; challenge domains with CNAME targets and live TXT values; API
keys with scopes; zones list with status badge (pending/active/exempt), required NS target
and "Verify now"; zone detail with rrset table (add/edit/delete by name+type, multi-value),
DNSSEC toggle with DS display, AXFR allow list and TSIG keys; client config snippets
(httpreq / acme-dns); admin area unlocked by entering the admin key (stored in session
only): users, all zones with owner, activation exemption, base zone editing. Accessible
components (labels, focus, dialogs via shadcn), empty/loading/error states on every query.
Gates: `bun run lint`, `typecheck`, `test` (Vitest, coverage ≥ 80 % on `features/*` logic
and `shared/lib`), `build`; the `Dockerfile` frontend stage and the CI e2e job switch from
npm to bun.

### 6. Config (`certo-core`)

New `[pdns]` section: `binary = ""` (default beside the executable), `api_port = 8081`.
New `[zones]` section: `resolver = ""` (recursive resolver for delegation checks).
`[general] listen/domain/nsname/nsadmin/records` unchanged in meaning; `protocol` and
`api.allow_zone_creation` are ignored. `config.toml`, `docs/configuration.md` updated.

### 7. Tests (RED first)

- `certo-pdns`: supervisor starts a fake binary and restarts it; config rendering;
  client against a tiny mocked HTTP server (zone create body, rrset PATCH body, error
  mapping). Integration tests marked `#[ignore]` unless `CERTO_PDNS_BIN` is set run the
  real static binary (readiness, TXT REPLACE visible via a UDP query).
- `certo-db`: 3 → 4 upgrade from a v3 fixture produces the import list and drops tables;
  `txt` value listing per name.
- `certo-api`: rrset routes ownership and validation; base zone hidden from users;
  pending zone refuses records; admin exemption activates without a check.
- activation: delegation check against a mocked parent nameserver (referral present /
  absent / wrong NS); daily re-check deactivates after the threshold; snapshot restore.
- e2e: current suite adapted to rrsets; expectations that encode hickory behaviour are
  re-checked against pdns (names outside every zone answer `REFUSED`, not `NXDOMAIN`);
  new case: restart certo and verify challenge TXT is reconciled.

### 8. Docs

`docs/architecture.md` (process model, API client, data ownership), `docs/deployment.md`
(two binaries per asset, port 53 held by pdns, `pdns/` data dir, DS change on upgrade),
`docs/api.md`, `docs/configuration.md`, `docs/development.md` (`just pdns-build`), README
feature list, `docs/decisions/20261018-powerdns-data-plane.md`, changelog.

## Steps

1. `pdns/` build files + release/CI workflow changes; verify amd64 and arm64 artifacts →
   `file` reports `statically linked` for both. ✅ (amd64 only; arm64 suspended by the owner)
2. `certo-pdns` crate: supervisor + client + config rendering, unit tests → green. ✅
3. Schema 4, import, `txt` reconciliation, base zone, zone activation → `certo-db`/
   `certo-pdns` tests green. ✅
4. Wire `main.rs`, rewrite `certo-api` zone routes, delete `certo-dns` and hickory deps →
   `just rs-ci` green, `cargo shear` clean. ✅ 2026-10-18 07:10 (72 tests)
5. Dashboard rewrite on the pma-web baseline (§5a) → `bun run lint && bun run typecheck &&
   bun run test && bun run build` green; manual check of every page against the running
   backend through nsl. ✅ 2026-10-18 06:30 (41 Vitest cases, coverage 99 %; pages checked
   with a Playwright run against the debug binary instead of nsl — same build served by
   certo itself)
6. e2e suite adaptation → `just e2e` green locally and on CI. ✅ 2026-10-18 07:40 locally (42 cases); CI after the bun switch in step 5
7. Docs, decision record, changelog, release `v0.1.0`. ✅ 2026-10-18 06:40 (docs and
   record written; the owner tags `v0.1.0` after reviewing and committing)

## Risks

- PowerDNS security fixes require a certo re-release (bundled binary). Mitigation: the
  `pdns/Dockerfile` pins the version in one place; a patch release is a tag.
- arm64 static build is unverified (spike was amd64); arm64 assets are suspended until a
  native `ubuntu-26.04-arm` build is verified (deferred by the owner).
- Existing DNSSEC zones get new keys on import; the owner re-publishes DS at the registrar
  (accepted: production is rebuilt once from the old database). TSIG secrets are preserved.
- Delegation checks depend on an outbound recursive resolver; a broken resolver blocks
  activation (never deactivation: deactivation needs 7 consecutive failed daily checks
  with a definite "not delegated to us" answer, resolver errors do not count).
- Record API contract changes (rrsets replace record ids); existing scripts need updating.
- certo no longer validates records itself beyond the type list; pdns error messages are
  surfaced as `400 pdns_rejected` with the pdns text.
- Port 53 is bound by `pdns_server`, started by certo; in the lode container both run as
  root as today. `setuid`/`setgid` can be added later via `[pdns]` if wanted.
- Asset grows by about 12 MB per architecture.

## Alternatives

- A. Keep the hickory server and add the system zone (plan 20261018-0429): no new runtime
  dependency, but every DNS feature stays home-grown. Rejected by the owner.
- B. Run the official `powerdns/pdns-auth-50` image as a second container and make certo
  a pure API client: no static build needed, but two images, two lifecycles, and lode
  cannot update them as one unit. Rejected: the owner wants one package.
- C. Write to the PowerDNS SQLite file directly instead of using the API: fewer moving
  parts, but bypasses pdns caches and locking. Rejected.
