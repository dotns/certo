# Configuration

certo reads a single TOML file, `-c <path>` (default `./data/config.toml`). A commented
sample lives at [`config.toml`](../config.toml) in the repository root.

Defaults below are the ones **applied by the code** (`certo.ReadConfig`, plus the library
behaviour when a value is empty) — not merely the values that happen to be in the sample
file. Keys marked *required* have no default; leaving them empty either aborts startup or
produces a useless binding.

## `[general]`

| Key | Type | Default / when empty | Notes |
|---|---|---|---|
| `listen` | string | `:53` (all interfaces, port 53) | DNS listen address, e.g. `127.0.0.1:53`. An empty value falls back to the DNS library default |
| `protocol` | string | **required** | `both`, `both4`, `both6`, `udp`, `udp4`, `udp6`, `tcp`, `tcp4`, `tcp6`. Empty aborts startup with `bad network` |
| `domain` | string | **required in practice** | Base domain certo is authoritative for; all records live at `<subdomain>.<domain>` |
| `nsname` | string | **required in practice** | Name used in the zone's SOA/NS answers |
| `nsadmin` | string | — | SOA admin address; `@` is written as `.` (e.g. `admin.example.com`) |
| `records` | []string | empty | Static records served alongside the dynamic TXT, in zone-file syntax |
| `debug` | bool | `false` | Verbose CORS/debug logging |

`domain` and `nsname` are not validated at load time, but nothing resolves correctly
without them.

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
| `acme_cache_dir` | string | `data/certs` | certmagic storage for `letsencrypt*` |
| `notification_email` | string | empty | ACME account email for `letsencrypt*` |
| `corsorigins` | []string | empty | Allowed CORS origins; wildcards permitted |
| `use_header` | bool | `false` | Take the client IP from a header instead of the socket. Only affects acme-dns `allowfrom` enforcement |
| `header_name` | string | — | Header to read when `use_header` is on, e.g. `X-Forwarded-For` (first value is used) |
| `jwt_secret` | string | random per process | HS256 signing key for dashboard JWTs. **When empty, a new random secret is generated at each start, so all existing sessions become invalid on restart and replicas cannot share sessions** — set it for stable/scaled deployments. certo logs a warning at startup when it is empty |
| `admin_key` | string | empty ⇒ admin API disabled | Value expected in `X-Admin-Key`. While empty, every `/admin/*` request returns `403 admin_key_not_configured`. Compared in constant time |
| `allow_registration` | bool | `true` | When `false`, `POST /api/register` and the anonymous acme-dns `POST /register` return `403 registration_disabled`. Existing accounts, login, and all protocol traffic keep working — use it to lock a private deployment to pre-created accounts |

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
protocol = "both"
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
