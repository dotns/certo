# Architecture

certo is a control plane in front of a bundled [PowerDNS Authoritative](https://doc.powerdns.com/authoritative/)
server. certo owns users, API keys and scopes, the ACME challenge protocols, zone
ownership, the dashboard and its own TLS; PowerDNS owns every DNS answer.

```
lode ──run──> certo ──spawn/supervise──> pdns_server (general.listen, API on 127.0.0.1:[pdns].api_port)
               │  users, keys, scopes, ownership, challenge protocols, dashboard, TLS
               └──HTTP API (X-API-Key, random per start)──> zones / rrsets / cryptokeys / tsigkeys / metadata
```

certo never reads or writes the PowerDNS SQLite file after creating its schema; all DNS
data goes through the API.

## Startup flow (`crates/certo/src/main.rs`)

1. Harden the process (umask 077, core dumps off, panic hook), parse `-c` (default
   `./data/config.toml`) and load the config (file, then `PORT` and `CERTO_<SECTION>__<KEY>`
   env overrides).
2. Set up logging, install the rustls `aws-lc-rs` provider.
3. `Db::open` — open SQLite, create or upgrade the schema.
4. `Supervisor::start` (`certo-pdns`) — resolve the `pdns_server` binary (`[pdns] binary`,
   default next to the certo executable), create `[pdns] dir` (0700), initialize
   `pdns.sqlite3` from the embedded PowerDNS schema when missing, write `pdns.conf` (0600)
   from the certo config, spawn `pdns_server` and wait up to 10 s for its API. The
   supervisor restarts PowerDNS with exponential backoff (max 30 s) when it exits; after 5
   exits within a minute certo itself exits non-zero so the outer supervisor restarts the
   whole tree.
5. Build the `Zones` service: import a schema-3 database once (see
   [Migrations](#migrations)), create the base zone when PowerDNS does not have it, and
   reconcile every challenge TXT name from the `txt` table into it.
6. Start the HTTP server (plain, PEM files, or ACME via certo's own DNS) and the hourly
   delegation-check loop.
7. Run until the HTTP server fails, PowerDNS crashes too often, or SIGINT/SIGTERM arrives;
   shutdown drains HTTP for up to 10 s, then sends PowerDNS SIGTERM and SIGKILL after 10 s.

## Crates

| Crate | Responsibility |
|---|---|
| `certo-core` | Config (`config.rs`), domain types, identifier generation (`ids.rs`), scope matching |
| `certo-db` | SQLite via `rusqlite` (bundled). One connection behind a mutex, calls run on `spawn_blocking`. Owns schema creation and versioned upgrades |
| `certo-pdns` | PowerDNS supervisor and generated config, HTTP API client, rrset validation, delegation checker, `Zones` service (base zone, challenge TXT sync, activation cycle, DNSSEC, AXFR/TSIG, legacy import). Feature `mock` provides an in-memory PowerDNS API and a scripted checker for tests |
| `certo-api` | `axum` HTTP layer: challenge protocols, dashboard API, zone API, admin API, CORS, SPA serving, TLS and ACME |
| `certo` | Binary: wiring, logging, shutdown |
| `web` | React SPA; `certo-api` embeds `web/dist` at build time (`rust-embed`) |

## HTTP request routing

`certo_api::app` (`crates/certo-api/src/lib.rs`) decides per request whether it belongs to
the API router or the SPA:

- Router paths: `/present`, `/cleanup`, `/update`, `/health`, `/livez`, `/readyz`,
  `/llms.txt`, `/api/*`, `/admin/*`, `/acmedns*`.
- `POST /register` → router (acme-dns registration). Any other method on `/register` →
  SPA, because the dashboard has a client-side `/register` signup page. This method-aware
  split is deliberate; changing it breaks either the signup page or acme-dns clients.
- Everything else → static file from `web/dist` if it exists, else `index.html` (SPA
  client-side routing). Priority: on-disk `web/dist` (dev override) > embedded copy.
- The dashboard's admin area therefore lives at `/manage`, not `/admin`.

## Challenge write paths

Both protocols end at the same `txt` table, keyed by `<subdomain>.<base domain>`:

| | httpreq | acme-dns |
|---|---|---|
| Endpoint | `POST /present` / `POST /cleanup` | `POST /update` |
| Auth | HTTP Basic (username + API key) | `X-Api-User` / `X-Api-Key` headers |
| Addressing | FQDN (`_acme-challenge.example.com.`, or the CNAME-resolved name) | `subdomain` |
| Write | `present_txt` — plain insert, unbounded | `update_acmedns_txt` — insert, then trim to the 2 newest rows |
| Delete | explicit `/cleanup` (`DELETE … WHERE Domain AND Value`) | none — the rolling cap is the cleanup |

After every write the handler calls `Zones::sync_txt(name)`, which publishes the full value
list of that name as one TXT rrset (TTL 1) in the base zone, or deletes the rrset when no
value is left. The `txt` table stays the source of truth: at startup every distinct name is
reconciled into PowerDNS, so a lost or wiped `pdns.sqlite3` is rebuilt from certo's
database.

The acme-dns rolling cap exists because acme-dns clients never call a cleanup endpoint,
and because a certificate covering both `example.com` and `*.example.com` produces two
challenges on the same name. Trimming uses `ORDER BY LastUpdate DESC, rowid DESC` —
`LastUpdate` is whole seconds, so `rowid` breaks ties within the same second.

### FQDN resolution (`crates/certo-api/src/httpreq.rs`)

`resolve_subdomain` accepts either form lego may send:

1. A name already under the base domain (CNAME-resolved, e.g. `a1b2c3d4.s.example.com`) —
   the subdomain is extracted and checked for ownership.
2. The original challenge name (e.g. `_acme-challenge.example.com`) — `_acme-challenge.`
   and the trailing dot are stripped, then the user's subdomain for that domain is looked up.

If the domain is not registered yet, `auto_create` provisions it — see
[on-demand creation](#on-demand-domain-creation). Case 1 is never auto-created: a name
already under the base domain has no real domain to create.

## DNS data plane (`crates/certo-pdns`)

PowerDNS runs with the `gsqlite3` backend (`gsqlite3-dnssec=yes`), `api=yes` on
`127.0.0.1:[pdns].api_port` with a per-start random key, `default-api-rectify=yes`,
`allow-axfr-ips=` (empty: transfers only per zone metadata), `security-poll-suffix=`
(no outbound security poll), `default-soa-content` built from `general.nsname`/`nsadmin`,
and `loglevel` mapped from `logconfig.loglevel`. PowerDNS output is forwarded to certo's
log. Names outside every zone answer `REFUSED` (PowerDNS behaviour; the former server said
`NXDOMAIN`).

### Base zone

`general.domain` is a PowerDNS zone like any other. `ensure_base_zone` creates it at first
start with the apex NS (`general.nsname`) and the parsed `general.records` lines that fall
inside the base domain (others are logged as errors); an existing zone is left alone, so
`general.records` is a first-start seed. The base zone row in `zones` has `user_id = NULL`;
only admin routes see it (`system: true`), it is always exempt and cannot be deleted
(`403 zone_not_allowed`). `_acme-challenge.<base>` for certo's own certificate is a TXT
rrset write on this zone.

### Hosted zones

- A zone belongs to one user. Names need at least two labels and may not be at or under
  `general.domain` or overlap another user's zone (`403 zone_not_allowed`).
- Records are managed as rrsets (`name`, `type`, `ttl`, `records[]`) through
  `PATCH …/rrsets` with `REPLACE`/`DELETE` changes. certo checks the owner name is inside
  the zone, the type is in the allowed list (A, AAAA, CAA, CNAME, MX, NS, PTR, SRV, TXT;
  PTR only in `in-addr.arpa` / `ip6.arpa` zones), CNAME is not at the apex, and quotes plain
  TXT values; PowerDNS validates the rest and its message is surfaced as
  `400 pdns_rejected`. The SOA is maintained by PowerDNS (`soa-edit-api`).
- DNSSEC (opt-in): one `ecdsa256` CSK created through `cryptokeys`; NSEC3 is switched on
  with `NSEC3PARAM` metadata (RFC 9276 parameters). `GET …/dnssec` returns the DNSKEY and
  the DS records PowerDNS computes (one per digest type).
- AXFR: `ALLOW-AXFR-FROM` metadata holds the allow list (CIDRs, normalized), `tsigkeys` +
  `TSIG-ALLOW-AXFR` metadata hold the TSIG keys. A transfer is accepted when the source
  address is allowed or the request is signed with one of the zone's keys. NOTIFY is not
  sent.
- After creating a zone certo flushes the PowerDNS packet cache for that name, because the
  cache would otherwise keep answering `REFUSED` for up to `cache-ttl` seconds.

### Zone activation

Anyone may create a zone, but it is served only when the parent delegates it to us:

1. `POST /api/zones` stores the ownership row as `pending` and runs a delegation check
   immediately. Nothing is served and rrsets cannot be changed (`409 zone_not_active`).
2. The check (`DnsDelegationChecker`) resolves the parent's nameservers through
   `[zones] resolver` (default: system resolver, fallback `1.1.1.1:53`), asks one of them
   non-recursively for `NS <zone>` and requires `general.nsname` in the referral.
3. On success the zone is created in PowerDNS (restoring the rrset snapshot of a previous
   activation, if any) with the apex NS and becomes `active`.
4. Pending zones are re-checked hourly and on `POST …/verify` (one check per minute per
   zone, `429 check_rate_limited`; an inconclusive check answers `503 check_failed`). A
   pending zone nobody activates within 30 days is deleted.
5. Active zones are re-checked daily. Only a definite "not delegated to us" answer counts
   as a failure; after 7 consecutive failures the zone is deactivated: the PowerDNS zone is
   deleted and its rrsets are kept in `zones.snapshot` so a later activation restores them.
   This is what frees a name when a domain changes hands.
6. Admin exemption (`PUT /admin/zones/{zone}/activation {"exempt": true}`) activates a zone
   without a check and skips re-checks; `false` returns it to the normal cycle.

Zone views expose `status` (`pending` / `active`), `exempt`, `system`, `verified_at`,
`last_check`, `last_error`, `failed_checks` and the `ns_target` to delegate to.

## Authorization model

Two credentials exist, resolved by `crates/certo-api/src/auth.rs` and `acmedns.rs`:

- **JWT** — dashboard sessions, 72 h, HS256. The signing secret is `api.jwt_secret`, or a
  random per-process value when unset (see [configuration.md](configuration.md)).
- **API key** — 32-char token with a JSON `scope` array. Sent as a Bearer token (dashboard
  API), Basic Auth password (httpreq, acme-dns storage) or `X-Api-Key` (acme-dns update).

Scope matching (`ApiKey` in `certo-core`):

| Scope | Meaning |
|---|---|
| `["*"]` | global — every domain, plus key/account management |
| `["*.example.com"]` | `example.com` and every subdomain of it |
| `["example.com"]` | that exact domain only |

The `GlobalBearer` extractor guards sensitive endpoints (profile, key management) — a
scoped key gets `403 global_key_required`. `Bearer` accepts any key and leaves scope checks
to the handler.

Hosted zones belong to one user. A user's session or global key manages all their zones; a
scoped key only zones its scope covers (`403 domain_not_in_scope`). Other users' zones and
the base zone answer `404`. Every user may create zones; serving them is gated by
[activation](#zone-activation). The admin API (`X-Admin-Key`) creates zones for any user,
sees every zone including the base zone, and sets the activation exemption.

### On-demand domain creation

When a request targets a domain the user has not registered, certo creates it instead of
failing, provided the **key's scope already covers that domain**. This applies to httpreq
`/present` and acme-dns storage fetch. Out-of-scope domains are rejected (`403`), so a
narrowly scoped key bounds what an automation can create. There is no config switch for
this — scope is the control.

### acme-dns `allowfrom`

`POST /register` accepts `{"allowfrom": ["1.2.3.4/32", …]}`. The CIDRs are stored with the
allocated subdomain and enforced on every `/update`: the client IP must fall inside one of
them, else `403 ip_not_allowed`. An empty list means no restriction. The client IP comes
from `RemoteAddr`, or from `api.header_name` when `api.use_header` is enabled (first value
of a comma-separated list).

## Database

certo's SQLite database, schema version **4**, tracked in `acmedns.db_version`:

```
acmedns(Name TEXT, Value TEXT)                        -- key/value metadata, incl. db_version
txt(Domain TEXT, Value TEXT, LastUpdate INT)          -- challenge TXT records (source of truth)
users(id, username UNIQUE, password_hash, created_at)
user_domains(user_id, domain, subdomain UNIQUE,
             allowfrom TEXT DEFAULT '[]',
             UNIQUE(user_id, domain))
api_keys(id, user_id, name, key_value UNIQUE,
         scope TEXT DEFAULT '["*"]', created_at)
zones(id, user_id NULL for the base zone, name UNIQUE,
      active, exempt, verified_at, last_check, last_error,
      failed_checks, snapshot TEXT, created_at)       -- ownership and activation state only
```

Everything DNS — rrsets, SOA serials, DNSSEC keys, TSIG keys, zone metadata — lives in
PowerDNS' own SQLite database, `[pdns] dir/pdns.sqlite3`.

Notes:

- `txt.Domain` holds the full internal name (`<subdomain>.<base domain>`), lower-cased with
  the trailing dot stripped.
- `user_domains` is also where anonymous acme-dns registrations live; for those,
  `domain == subdomain` because there is no real domain behind them.
- `allowfrom` is a JSON array of CIDR strings, only meaningful for acme-dns registrations.
- Deleting a user (`DELETE /api/profile` or `/admin/users/{id}`) first removes the user's
  zones from PowerDNS, then deletes its `api_keys`, `user_domains`, the challenge TXT rows
  of those subdomains and its zone rows in one transaction, and finally unpublishes the
  affected TXT names; removing a domain does the same for that subdomain.
- `zones.snapshot` is the JSON rrset list of a deactivated zone, restored on re-activation.

**Credential storage.** Account passwords are bcrypt-hashed. **API keys are stored in
plaintext** (`api_keys.key_value`) — this is required by the acme-dns HTTP storage backend,
which must return the key as the acme-dns `password`, and by httpreq/acme-dns Basic-Auth
echo. Treat both databases as secrets: restrict file permissions and use disk encryption,
because a compromise exposes usable API keys (certo) and DNSSEC private keys and TSIG
secrets (PowerDNS) directly. `admin_key` and `jwt_secret` live only in the config file. A
TSIG secret is returned by the API only once, at creation.

### Migrations

`Db::open` reads `db_version` (`crates/certo-db/src/schema.rs`):

- version > `DB_VERSION` → refuse to start ("regenerate the database").
- version < `DB_VERSION` → run the upgrade chain, each step in one transaction: `1 → 2`
  adds `user_domains.allowfrom`, `2 → 3` adds the zone tables, `3 → 4` adds the activation
  columns and schedules the one-time import.

The `3 → 4` import runs at startup once PowerDNS is ready: every zone of the old `records`,
`zone_keys` and `tsig_keys` tables is created in PowerDNS with its rrsets, DNSSEC is
re-enabled with a fresh key where it was on (the new DS is logged at warn level), TSIG keys
are created with their old secrets, transfer settings are copied, and every imported zone is
marked active and exempt (it was served before). The old tables are then dropped. A failed
import leaves the database at version 3 and aborts the start.

A fresh database is created at the current version directly. Version 3 and 4 databases
cannot be opened by the Go releases of certo.

## Identifier formats

| Identifier | Format | Source |
|---|---|---|
| Dashboard/httpreq subdomain | 8 hex chars, `sha256(lower(username) + ":" + lower(domain))[:8]` — deterministic, so a rebuilt database keeps existing CNAMEs valid. Collisions retry with a salt (≤ 10 attempts) | `ids::subdomain` |
| acme-dns subdomain | 10 random chars from `[0-9a-z]` — deliberately longer than 8 so the two pools are distinguishable | `ids::nanoid` |
| acme-dns username | `acme-` + 16 random chars | `ids::acmedns_username` |
| API key | 32 random chars from `[0-9a-z]` | `ids::api_key` |
| acme-dns `txt` value | exactly 43 chars (unpadded base64url of a 32-byte digest) — enforced on `/update` | `ACMEDNS_TXT_LENGTH` |

Anonymous acme-dns users have no usable password hash and cannot log in to the dashboard;
they authenticate by API key only.
