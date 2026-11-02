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
3. `Db::open` — open SQLite, create the schema or check its version.
4. `Supervisor::start` (`certo-pdns`) — resolve the `pdns_server` binary (`[pdns] binary`,
   default next to the certo executable), create `[pdns] dir` (0700), initialize
   `pdns.sqlite3` from the embedded PowerDNS schema when missing, write `pdns.conf` (0600)
   from the certo config, spawn `pdns_server` and wait up to 10 s for its API. The
   supervisor restarts PowerDNS with exponential backoff (max 30 s) when it exits; after 5
   exits within a minute certo itself exits non-zero so the outer supervisor restarts the
   whole tree.
5. Build the `Zones` service, create the base zone row on first start (seeded from
   `general.records`) and run `Zones::sync` (see [Sync](#sync)): PowerDNS zones the
   database does not serve are removed, active zones are created or converged from the
   zone tables. A sync failure aborts the start.
6. Start the HTTP server (plain, PEM files, or ACME via certo's own DNS) and the hourly
   delegation-check loop.
7. Run until the HTTP server fails, PowerDNS crashes too often, or SIGINT/SIGTERM arrives;
   shutdown drains HTTP for up to 10 s, then sends PowerDNS SIGTERM and SIGKILL after 10 s.

## Crates

| Crate | Responsibility |
|---|---|
| `certo-core` | Config (`config.rs`), domain types, identifier generation (`ids.rs`), scope matching |
| `certo-db` | SQLite via `rusqlite` (bundled). One connection behind a mutex, calls run on `spawn_blocking`. Owns schema creation and the version check |
| `certo-pdns` | PowerDNS supervisor and generated config, HTTP API client, rrset validation, delegation checker, `Zones` service (base zone, challenge TXT sync, activation cycle, DNSSEC, AXFR/TSIG). Feature `mock` provides an in-memory PowerDNS API and a scripted checker for tests |
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

### Source of truth and sync

certo's database holds every DNS fact: zone ownership and activation (`zones`), the
rrsets of every zone (`zone_rrsets`), the DNSSEC signing key as PowerDNS exports it
(`zone_dnssec`, private key in ISC format so a re-import keeps the DNSKEY and DS), TSIG
keys with their secrets (`zone_tsig_keys`), the AXFR allow list (`zones.axfr_allow_from`)
and the challenge TXT values (`txt`). PowerDNS is derived from it: a lost `pdns.sqlite3`
is rebuilt completely at the next start.

Every write goes to PowerDNS first (it validates the data) and is then stored in the same
shape; reads of rrsets, DNSSEC state and transfer settings come from the database (the
rrset list therefore has no SOA row — PowerDNS maintains the SOA). The DNSKEY and DS
strings are the only values read back from PowerDNS.

Each zone has a data `version` that moves in the same transaction as any change to what
PowerDNS serves for it: rrsets, DNSSEC, TSIG keys, allow list, activation, and for the base
zone every challenge TXT write. After every successful push certo stamps the PowerDNS zone
with that version in the metadata kind `X-CERTO-VERSION`. `Zones::status` compares the two
sides without changing anything (`GET /admin/sync`: sum of the versions of all active
zones vs. sum of the stamps, plus the list of zones that differ).

`Zones::sync` (startup; `POST /admin/sync` and the dashboard's "Sync DNS" button) brings
PowerDNS in line: it deletes every PowerDNS zone that is not an active database zone,
then for each active zone creates it when missing or converges it — one `PATCH` with the
rrsets that differ (SOA and certo's own `_acme-challenge` TXT are left alone), the stored
key imported when no key with the same content exists and other keys removed, NSEC3PARAM
set, missing TSIG keys created with their stored secrets, both metadata kinds set, the
packet cache flushed, the version stamped. A zone whose stamp equals its database version
is skipped at startup; the admin endpoint forces every zone. Server-wide TSIG keys that no
active zone references are deleted. A PowerDNS zone that exists but was never stamped is
**adopted** first: its rrsets (minus SOA and TTL-1 challenge TXT), active key and allow
list are taken into the database before converging — this is what carries a server from
the previous generation over without losing records. TSIG keys are not adopted.

### Base zone

`general.domain` is a zone like any other, with `user_id = NULL` in `zones`;
`ensure_base_zone` creates the row at first start with the apex NS (`general.nsname`) and
the parsed `general.records` lines that fall inside it (others are logged as errors), so
`general.records` is a first-start seed; later changes are made through the admin zone API.
Only admin routes see the base zone (`system: true`), it is always exempt and cannot be
deleted (`403 zone_not_allowed`).

The challenge domain `general.acme_domain` (default: the zone itself) is a name inside that
zone: challenge TXT rrsets `<subdomain>.<acme_domain>` are rrset writes on the base zone,
derived from the `txt` table at sync time (never stored twice), as is
`_acme-challenge.<domain>` for certo's own certificate. A typical layout serves
`example.com` as the base zone with its ordinary A/MX records and keeps challenges under
`s.example.com`, so the registrar delegation points at `example.com` only.
`ZonePolicy { base_domain, base_zone, nsname }` carries the two names (`base_domain` is the
challenge domain); user zones may not be at or under `base_zone`.

### Hosted zones

- A zone belongs to one user. Names need at least two labels and may not be at or under
  `general.domain`, be a public suffix (`in-addr.arpa`, `co.uk`, `github.io`; checked
  against the Public Suffix List compiled into the binary), or overlap another user's
  zone (`403 zone_not_allowed`). Pending zones count for the overlap check, so without the
  suffix rule one user could block every reverse zone.
- Records are managed as rrsets (`name`, `type`, `ttl`, `records[]`) through
  `PATCH …/rrsets` with `REPLACE`/`DELETE` changes. certo checks the owner name is inside
  the zone, the type is in the allowed list (A, AAAA, CAA, CNAME, MX, NS, PTR, SRV, TXT;
  PTR only in `in-addr.arpa` / `ip6.arpa` zones), CNAME is not at the apex, and quotes plain
  TXT values; PowerDNS validates the rest and its message is surfaced as
  `400 pdns_rejected`. Accepted changes are stored in `zone_rrsets`. The SOA is
  maintained by PowerDNS (`soa-edit-api`) and not part of the rrset list.
- DNSSEC (opt-in): one `ecdsa256` CSK created through `cryptokeys`; its private key is
  fetched (`GET …/cryptokeys/{id}`) and stored in `zone_dnssec` together with the NSEC
  mode; NSEC3 is switched on with `NSEC3PARAM` metadata (RFC 9276 parameters).
  `GET …/dnssec` returns the DNSKEY and the DS records PowerDNS computes (one per digest
  type).
- AXFR: `ALLOW-AXFR-FROM` metadata holds the allow list (CIDRs, normalized), `tsigkeys` +
  `TSIG-ALLOW-AXFR` metadata hold the TSIG keys; both are stored (`zones.axfr_allow_from`,
  `zone_tsig_keys` with the secret PowerDNS generated). A transfer is accepted when the
  source address is allowed or the request is signed with one of the zone's keys. NOTIFY
  is not sent.
- After creating a zone certo flushes the PowerDNS packet cache for that name, because the
  cache would otherwise keep answering `REFUSED` for up to `cache-ttl` seconds.

### Zone activation

Anyone may create a zone, but it is served only when the parent delegates it to us:

1. `POST /api/zones` stores the ownership row as `pending` and runs a delegation check
   immediately. Nothing is served and rrsets cannot be changed (`409 zone_not_active`).
2. The check (`DnsDelegationChecker`) resolves the parent's nameservers through
   `[zones] resolver` (default: system resolver, fallback `1.1.1.1:53`), asks one of them
   non-recursively for `NS <zone>` and requires `general.nsname` in the referral.
3. On success the zone becomes `active` and is built in PowerDNS from the stored data
   (the apex NS stored at creation, plus whatever was managed before a deactivation).
4. Pending zones are re-checked hourly and on `POST …/verify` (one check per minute per
   zone, `429 check_rate_limited`; an inconclusive check answers `503 check_failed`). A
   pending zone nobody activates within 30 days is deleted.
5. Active zones are re-checked daily. Only a definite "not delegated to us" answer counts
   as a failure; after 7 consecutive failures the zone is deactivated: the PowerDNS zone
   (and its TSIG keys) are deleted while the stored data stays, so a later activation
   restores the zone as it was. This is what frees a name when a domain changes hands.
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

certo's SQLite database, schema version **1**, tracked in `acmedns.db_version`:

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
      failed_checks, version, axfr_allow_from TEXT DEFAULT '[]',
      created_at)                                     -- ownership, activation, data version
zone_rrsets(zone_id, name, rtype, ttl, records TEXT,  -- records JSON list, name with trailing dot
            PRIMARY KEY(zone_id, name, rtype))
zone_dnssec(zone_id PRIMARY KEY, nsec3, algorithm, private_key)
zone_tsig_keys(zone_id, name, algorithm, secret, PRIMARY KEY(zone_id, name))
```

PowerDNS' own SQLite database, `[pdns] dir/pdns.sqlite3`, is derived from these tables
(see [Source of truth and sync](#source-of-truth-and-sync)) and holds only what PowerDNS
needs at runtime (SOA serials, signatures, caches).
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
- Deleting a zone removes its `zone_rrsets`, `zone_dnssec` and `zone_tsig_keys` rows in the
  same transaction; an inactive zone keeps them for re-activation.
- `zones.version` starts at 1 and is incremented with every change to what PowerDNS serves
  for the zone; challenge TXT writes increment the base zone's version.

**Credential storage.** Account passwords are bcrypt-hashed. **API keys are stored in
plaintext** (`api_keys.key_value`) — this is required by the acme-dns HTTP storage backend,
which must return the key as the acme-dns `password`, and by httpreq/acme-dns Basic-Auth
echo. Treat the database as a secret: restrict file permissions and use disk encryption,
because a compromise exposes usable API keys, DNSSEC private keys and TSIG secrets
directly (`pdns.sqlite3` holds copies of the latter two). `admin_key` and `jwt_secret` live only in the config file. A
TSIG secret is returned by the API only once, at creation.

### Migrations

There is no upgrade chain. `Db::open` (`crates/certo-db/src/schema.rs`) creates every
table of a new database and stores `db_version = 1`. An existing database is accepted only
when `db_version` is `1` **and** the `zones` table has the current columns (the Go
releases also wrote version 1 with a different layout); anything else fails with
`unsupported database schema N … migrate the database by hand`. The manual steps for a
database of the previous generation are in [deployment.md](deployment.md#upgrades).

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
