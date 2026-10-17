# Architecture

## Startup flow (`crates/certo/src/main.rs`)

1. Harden the process (umask 077, core dumps off, panic hook), parse `-c` (default
   `./data/config.toml`) and load the config (file, then `PORT` and `CERTO_<SECTION>__<KEY>`
   env overrides).
2. Set up logging, install the rustls `aws-lc-rs` provider.
3. `Db::open` — open SQLite, create or upgrade the schema.
4. Build the challenge responder and the hosted-zone service; `Zones::load_all` signs and
   publishes every zone into the catalog. A background task re-signs DNSSEC zones daily.
5. Start the DNS listeners, then the HTTP server (plain, PEM files, or ACME via its own DNS).
6. Run until a server fails or SIGINT/SIGTERM arrives; shutdown drains HTTP for up to 10 s.

## Crates

| Crate | Responsibility |
|---|---|
| `certo-core` | Config (`config.rs`), domain types, identifier generation (`ids.rs`), scope matching |
| `certo-db` | SQLite via `rusqlite` (bundled). One connection behind a mutex, calls run on `spawn_blocking`. Owns schema creation and versioned upgrades |
| `certo-dns` | `hickory-server`: challenge responder (static records + DB TXT), hosted zones (validation, materialization, DNSSEC, AXFR/TSIG), listeners |
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

## Challenge write paths

Both protocols end at the same `txt` table, keyed by `<subdomain>.<base domain>`:

| | httpreq | acme-dns |
|---|---|---|
| Endpoint | `POST /present` / `POST /cleanup` | `POST /update` |
| Auth | HTTP Basic (username + API key) | `X-Api-User` / `X-Api-Key` headers |
| Addressing | FQDN (`_acme-challenge.example.com.`, or the CNAME-resolved name) | `subdomain` |
| Write | `present_txt` — plain insert, unbounded | `update_acmedns_txt` — insert, then trim to the 2 newest rows |
| Delete | explicit `/cleanup` (`DELETE … WHERE Domain AND Value`) | none — the rolling cap is the cleanup |

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

## DNS resolution (`crates/certo-dns`)

`DnsHandler` sends a query to the hosted-zone catalog when the name is inside a user zone,
and to the challenge responder otherwise.

Challenge responder (`challenge.rs`), answering exactly like the former Go server:

- Names it is not authoritative for get `NXDOMAIN`; static records come from
  `general.records`, and TXT queries append dynamic values.
- TXT lookups return **all** stored values for the queried name, one TXT RR per value,
  TTL 1. Writing a row makes a new subdomain answerable.
- `_acme-challenge.<base domain>` is special-cased so certo can solve DNS-01 for its
  **own** API certificate when `api.tls = letsencrypt`.

Hosted zones (`zones.rs`, `zone.rs`, `records.rs`):

- The database is the source of truth. Every change rebuilds the zone (SOA from
  `general.nsname`/`nsadmin`, serial `max(old + 1, unix time)`) into a hickory in-memory
  zone on the blocking pool and swaps it into the catalog; publishes are serialized so a
  stale snapshot never wins. The catalog is an immutable snapshot replaced on every
  publish, so queries never wait for a publish.
- Uniqueness, overlap with another user's zone, and CNAME exclusivity at a name are
  enforced inside the `certo-db` write transactions.
- Zone names need at least two labels and may not be at or under `general.domain`, cover a
  static config record, or overlap another user's zone.
- Record types: A, AAAA, CAA, CNAME, MX, NS, PTR, SRV, TXT. PTR only in `in-addr.arpa` /
  `ip6.arpa` zones; CNAME not at the apex and exclusive at its name.
- DNSSEC (opt-in): one ECDSAP256SHA256 key per zone, online signing with NSEC or NSEC3
  (RFC 9276 parameters), RRSIG validity 14 days, re-signed every 24 h.
- AXFR is refused unless the source address is in the zone's allow list or the request is
  signed with one of its TSIG keys (`CertoZone` wraps the in-memory zone because hickory's
  in-memory handler does not verify TSIG). DNSSEC records are included only when the
  request sets DO.

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
scoped key only zones its scope covers (`403 domain_not_in_scope`). Other users' zones
answer `404`. Self-service creation needs `api.allow_zone_creation`; the admin API can
always create a zone and assign it to a user.

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

SQLite, schema version **3**, tracked in `acmedns.db_version`.

```
acmedns(Name TEXT, Value TEXT)                        -- key/value metadata, incl. db_version
txt(Domain TEXT, Value TEXT, LastUpdate INT)          -- challenge TXT records
users(id, username UNIQUE, password_hash, created_at)
user_domains(user_id, domain, subdomain UNIQUE,
             allowfrom TEXT DEFAULT '[]',
             UNIQUE(user_id, domain))
api_keys(id, user_id, name, key_value UNIQUE,
         scope TEXT DEFAULT '["*"]', created_at)
zones(id, user_id, name UNIQUE, serial, dnssec_enabled,
      nsec_mode DEFAULT 'nsec', axfr_allow_from TEXT DEFAULT '[]', created_at)
records(id, zone_id, name, rtype, ttl, rdata, created_at)
zone_keys(id, zone_id, algorithm, private_key BLOB, created_at)   -- DNSSEC, PKCS#8
tsig_keys(id, zone_id, name, algorithm, secret BLOB, created_at,
          UNIQUE(zone_id, name))
```

Notes:

- `txt.Domain` holds the full internal name (`<subdomain>.<base domain>`), lower-cased with
  the trailing dot stripped.
- `user_domains` is also where anonymous acme-dns registrations live; for those,
  `domain == subdomain` because there is no real domain behind them.
- `allowfrom` is a JSON array of CIDR strings, only meaningful for acme-dns registrations.
- Deleting a user (`DELETE /api/profile` or `/admin/users/{id}`) removes its `api_keys`,
  `user_domains`, the challenge TXT rows of those subdomains, and its zones (with records
  and keys) in one transaction; removing a domain also removes that subdomain's TXT rows.
- `records` store absolute owner names without the trailing dot and RDATA in presentation
  format (names absolute, TXT quoted).

**Credential storage.** Account passwords are bcrypt-hashed. **API keys are stored in
plaintext** (`api_keys.key_value`) — this is required by the acme-dns HTTP storage backend,
which must return the key as the acme-dns `password`, and by httpreq/acme-dns Basic-Auth
echo. Treat the database as a secret: restrict file permissions and use disk encryption,
because a database compromise exposes usable API keys directly. `admin_key` and
`jwt_secret` live only in the config file, never in the database. DNSSEC private keys and
TSIG secrets are stored in the database too and are never returned by the API (a TSIG
secret only once, at creation).

### Migrations

`Db::open` reads `db_version` (`crates/certo-db/src/schema.rs`):

- version > `DB_VERSION` → refuse to start ("regenerate the database").
- version < `DB_VERSION` → run the upgrade chain, each step in one transaction: `1 → 2`
  adds `user_domains.allowfrom`, `2 → 3` adds the zone tables.

A fresh database is created at the current version directly. Version 3 databases cannot be
opened by the Go releases of certo.

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
