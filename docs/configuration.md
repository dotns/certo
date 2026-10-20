# Configuration

certo reads a single TOML file, `-c <path>` (default `./data/config.toml`). A commented
sample lives at [`config.toml`](../config.toml) in the repository root.

Defaults below are the ones **applied by the code** (`Config::load` in `certo-core`, plus the library
behaviour when a value is empty) — not merely the values that happen to be in the sample
file. Keys marked *required* have no default; leaving them empty either aborts startup or
produces a useless binding.

## `[general]`

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `listen` | string | **required** | DNS listen address handed to `pdns_server`, e.g. `127.0.0.1:53`; `:53` binds all interfaces. Empty aborts startup |
| `protocol` | string | — | Ignored since PowerDNS always serves UDP and TCP; accepted so older config files keep loading |
| `domain` | string | **required in practice** | Base domain (the base zone); challenge records live at `<subdomain>.<domain>` |
| `nsname` | string | **required in practice** | Nameserver name: SOA MNAME and apex NS of the base zone and of every hosted zone, and the target a hosted zone must be delegated to (`ns_target` in `GET /api/info`) |
| `nsadmin` | string | — | SOA admin address; `@` is written as `.` (e.g. `admin.example.com`) |
| `records` | []string | empty | Records seeded into the base zone **when it is first created** (zone-file syntax, inside the base domain). Later changes are made through the admin zone API, not by editing this list |
| `debug` | bool | `false` | Accepted for compatibility; use `logconfig.loglevel = "debug"` for verbose logs |

`domain` and `nsname` are not validated at load time, but nothing resolves correctly
without them.

## `[pdns]`

certo starts and supervises the bundled PowerDNS Authoritative server; these keys say where
to find it and where its state lives.

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `binary` | string | `pdns_server` next to the certo executable | Path of the `pdns_server` executable |
| `dir` | string | `data/pdns` | Directory for the generated `pdns.conf`, the PowerDNS SQLite database (`pdns.sqlite3`) and its control socket. Created with mode `0700` |
| `api_port` | u16 | `8081` | Port of the PowerDNS HTTP API, bound to `127.0.0.1` only. The API key is random per start |

## `[zones]`

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `resolver` | string | system resolver (`/etc/resolv.conf`), fallback `1.1.1.1:53` | Recursive resolver (`host:port`) used to find a zone's parent nameservers for the delegation check |

## `[database]`

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `engine` | string | `sqlite` | Only `sqlite` is accepted; any other value aborts startup |
| `connection` | string | **required** | Local path (`data/db/certo.db`) or a `file:` URL. Empty aborts startup. Parent directories are created automatically |

`:memory:` works and is what the test suites use.

## `[api]`

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `ip` | string | **required** | Listen address for HTTP(S), e.g. `0.0.0.0` |
| `port` | string | **required** | Listen port. Empty binds an arbitrary free port. Overridden by the `PORT` env var |
| `api_domain` | string | falls back to `general.domain` | Display only — reported by `GET /api/info` and used to build the dashboard's client snippets |
| `tls` | string | `none` | `none`, `cert`, `letsencrypt`, `letsencryptstaging`. Any other value aborts startup |
| `tls_cert_fullchain` | string | — | Required when `tls = "cert"` |
| `tls_cert_privkey` | string | — | Required when `tls = "cert"` |
| `acme_cache_dir` | string | `data/certs` | Certificate and ACME account storage for `letsencrypt*` (certmagic's layout, so a Go release's cache is reused) |
| `acme_directory` | string | Let's Encrypt (production or staging, per `tls`) | ACME directory URL, for other or private CAs |
| `acme_root_ca` | string | empty | PEM root certificate trusted for `acme_directory` (private CAs) |
| `notification_email` | string | empty | ACME account email for `letsencrypt*` |
| `corsorigins` | []string | empty | Allowed CORS origins; wildcards permitted |
| `use_header` | bool | `false` | Take the client IP from a header instead of the socket. Only affects acme-dns `allowfrom` enforcement |
| `header_name` | string | — | Header to read when `use_header` is on, e.g. `X-Forwarded-For` (first value is used) |
| `jwt_secret` | string | random per process | HS256 signing key for dashboard JWTs. **When empty, a new random secret is generated at each start, so all existing sessions become invalid on restart and replicas cannot share sessions** — set it for stable/scaled deployments. certo logs a warning at startup when it is empty |
| `admin_key` | string | empty ⇒ admin API disabled | Value expected in `X-Admin-Key`. While empty, every `/admin/*` request returns `403 admin_key_not_configured`. Compared in constant time |
| `allow_registration` | bool | `true` | When `false`, `POST /api/register` and the anonymous acme-dns `POST /register` return `403 registration_disabled`. Existing accounts, login, and all protocol traffic keep working — use it to lock a private deployment to pre-created accounts |
| `allow_zone_creation` | bool | — | Ignored (every user may create zones; a zone is served only once its delegation is verified or an admin exempts it, see [architecture.md](architecture.md#zone-activation)). Accepted so older config files keep loading |

## `[logconfig]`

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `loglevel` | string | `info` | `debug`, `info`, `warn`, `error` (also `dpanic`, `panic`, `fatal`). An unparseable value aborts startup |
| `logtype` | string | `stdout` | `file` writes to `logfile`; any other value logs to stdout/stderr |
| `logfile` | string | — | Path used when `logtype = "file"`; parent directories are created |
| `logformat` | string | `console` | `json` selects JSON encoding; **any other value (including empty) yields console/plain text** |

## Environment variables

| Variable | Effect |
|---|---|
| `PORT` | Overrides `api.port` (convenient for PaaS hosts) |
| `CERTO_<SECTION>__<KEY>` | Overrides any config key, e.g. `CERTO_API__JWT_SECRET`, `CERTO_GENERAL__DOMAIN` |

certo does **not** read the `ACME_DNS_*` or `HTTPREQ_*` variables — those are consumed by
the ACME clients that talk to it. See [protocols.md](protocols.md).

## acme-dns protocol

There is no configuration for the acme-dns endpoints: `POST /register`, `POST /update` and
the `/acmedns` HTTP storage backend are always served. Who may register domains is
controlled by API-key scope, and who may update a registration by its `allowfrom` list —
see [architecture.md](architecture.md#authorization-model).

## Minimal example

```toml
[general]
listen = "0.0.0.0:53"
domain = "s.example.com"
nsname = "s.example.com"
nsadmin = "admin.example.com"
records = [
    "s.example.com. A 198.51.100.1",
    "s.example.com. NS s.example.com.",
]

[database]
engine = "sqlite"
connection = "data/db/certo.db"

[pdns]
dir = "data/pdns"

[api]
ip = "0.0.0.0"
port = "3000"
tls = "none"
jwt_secret = "change-me-to-a-random-string"
admin_key = "change-me-to-a-secret-key"

[logconfig]
loglevel = "info"
logtype = "stdout"
logformat = "json"
```
