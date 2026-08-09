# Architecture

## Startup flow (`main.go`)

1. Parse `-c` (default `./data/config.toml`) and read the TOML config via `certo.ReadConfig`.
2. `PORT` env var, if set, overrides `api.port`.
3. Set up logging (`certo.SetupLogging`).
4. `database.Init` — open SQLite, create/migrate the schema.
5. `api.Init` then `nameserver.InitAndStart` — DNS listeners start first, the API server
   runs in a goroutine.
6. Block on an error channel; any server error is fatal.

## Packages

| Package | Responsibility |
|---|---|
| `pkg/certo` | Shared foundation: `Config` + `ReadConfig`, the `DB`/`NS` interfaces, domain types and DTOs, logging setup, identifier generation (`cname.go`) |
| `pkg/database` | `certo.DB` over SQLite (`modernc.org/sqlite`, no CGO). One mutex serialises all access. Owns schema creation and versioned migrations |
| `pkg/api` | HTTP layer (`julienschmidt/httprouter` + `rs/cors`): challenge protocols, dashboard API, admin API, SPA serving, TLS |
| `pkg/nameserver` | Authoritative DNS server (`miekg/dns`): static records from config plus dynamic TXT from the database |
| `web` | React SPA; `web/embed.go` embeds `web/dist` into the binary |

## HTTP request routing

`api.Start` builds the router, then wraps it in `withSPA` (`pkg/api/api.go`). `withSPA`
decides per request whether it belongs to the API or the embedded SPA:

- Router paths: `/present`, `/cleanup`, `/update`, `/health`, `/llms.txt`, `/api/*`,
  `/admin/*`, `/acmedns*`.
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
| Write | `PresentTXT` — plain insert, unbounded | `UpdateACMEDNSTXT` — insert, then trim to the 2 newest rows |
| Delete | explicit `/cleanup` (`DELETE … WHERE Domain AND Value`) | none — the rolling cap is the cleanup |

The acme-dns rolling cap exists because acme-dns clients never call a cleanup endpoint,
and because a certificate covering both `example.com` and `*.example.com` produces two
challenges on the same name. Trimming uses `ORDER BY LastUpdate DESC, rowid DESC` —
`LastUpdate` is whole seconds, so `rowid` breaks ties within the same second.

### FQDN resolution (`pkg/api/present.go`)

`resolveSubdomain` accepts either form lego may send:

1. A name already under the base domain (CNAME-resolved, e.g. `a1b2c3d4.s.example.com`) —
   the subdomain is extracted and checked for ownership.
2. The original challenge name (e.g. `_acme-challenge.example.com`) — `_acme-challenge.`
   and the trailing dot are stripped, then the user's subdomain for that domain is looked up.

If the domain is not registered yet, `autoCreateSubdomain` provisions it — see
[on-demand creation](#on-demand-domain-creation). Case 1 is never auto-created: a name
already under the base domain has no real domain to create.

## DNS resolution (`pkg/nameserver`)

- `answer` rejects names it is not authoritative for with `NXDOMAIN`, serves static records
  from `general.records`, and for `TypeTXT` appends dynamic values.
- `answerTXT` looks up **all** stored values for the queried name and returns one TXT RR per
  value, TTL 1. Nothing else is needed for a new subdomain to resolve — writing a row makes
  it answerable.
- `_acme-challenge.<base domain>` is special-cased (`isOwnChallenge`) so certo can solve
  DNS-01 for its **own** API certificate when `api.tls = letsencrypt`.

## Authorization model

Two credentials exist, resolved by `pkg/api/auth.go` and `pkg/api/acmedns.go`:

- **JWT** — dashboard sessions, 72 h, HS256. The signing secret is `api.jwt_secret`, or a
  random per-process value when unset (see [configuration.md](configuration.md)).
- **API key** — 32-char token with a JSON `scope` array. Sent as a Bearer token (dashboard
  API), Basic Auth password (httpreq, acme-dns storage) or `X-Api-Key` (acme-dns update).

Scope matching (`certo.APIKey`):

| Scope | Meaning |
|---|---|
| `["*"]` | global — every domain, plus key/account management |
| `["*.example.com"]` | `example.com` and every subdomain of it |
| `["example.com"]` | that exact domain only |

`JWTOrGlobalKeyAuth` guards sensitive endpoints (profile, key management) — a scoped key
gets `403 global_key_required`. `JWTOrKeyAuth` accepts any key and leaves scope checks to
the handler.

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

SQLite, schema version **2**, tracked in `acmedns.db_version`.

```
acmedns(Name TEXT, Value TEXT)                        -- key/value metadata, incl. db_version
txt(Domain TEXT, Value TEXT, LastUpdate INT)          -- challenge TXT records
users(id, username UNIQUE, password_hash, created_at)
user_domains(user_id, domain, subdomain UNIQUE,
             allowfrom TEXT DEFAULT '[]',
             UNIQUE(user_id, domain))
api_keys(id, user_id, name, key_value UNIQUE,
         scope TEXT DEFAULT '["*"]', created_at)
```

Notes:

- `txt.Domain` holds the full internal name (`<subdomain>.<base domain>`), lower-cased with
  the trailing dot stripped.
- `user_domains` is also where anonymous acme-dns registrations live; for those,
  `domain == subdomain` because there is no real domain behind them.
- `allowfrom` is a JSON array of CIDR strings, only meaningful for acme-dns registrations.
- Deleting a user removes its `api_keys` and `user_domains`; `DELETE /api/profile` also
  clears the user's TXT rows first.

**Credential storage.** Account passwords are bcrypt-hashed. **API keys are stored in
plaintext** (`api_keys.key_value`) — this is required by the acme-dns HTTP storage backend,
which must return the key as the acme-dns `password`, and by httpreq/acme-dns Basic-Auth
echo. Treat the database as a secret: restrict file permissions and use disk encryption,
because a database compromise exposes usable API keys directly. `admin_key` and
`jwt_secret` live only in the config file, never in the database.

### Migrations

`Init` reads `db_version` and calls `checkDBUpgrades`:

- version > `DBVersion` → refuse to start ("regenerate the database").
- version < `DBVersion` → run the upgrade chain. `1 → 2` adds `user_domains.allowfrom`
  via `ALTER TABLE` and bumps `db_version`.

A fresh database is created at the current version directly.

## Identifier formats

| Identifier | Format | Source |
|---|---|---|
| Dashboard/httpreq subdomain | 8 hex chars, `sha256(lower(username) + ":" + lower(domain))[:8]` — deterministic, so a rebuilt database keeps existing CNAMEs valid. Collisions retry with a salt (≤ 10 attempts) | `certo.GenerateSubdomain` |
| acme-dns subdomain | 10 random chars from `[0-9a-z]` — deliberately longer than 8 so the two pools are distinguishable | `certo.GenerateNanoID` |
| acme-dns username | `acme-` + 16 random chars | `certo.GenerateACMEDNSUsername` |
| API key | 32 random chars from `[0-9a-z]` | `certo.GenerateAPIKey` |
| acme-dns `txt` value | exactly 43 chars (unpadded base64url of a 32-byte digest) — enforced on `/update` | `certo.ACMEDNSTxtLength` |

Anonymous acme-dns users have no usable password hash and cannot log in to the dashboard;
they authenticate by API key only.
