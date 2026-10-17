# Deployment

certo is a single binary that serves the DNS zone, the HTTP API and the dashboard SPA. It
needs port 53 (UDP+TCP) for DNS and one HTTP(S) port for the API.

## Docker Compose (lode)

Releases are published on GitHub as static binaries, `certo-linux-amd64.tar.gz` and
`certo-linux-arm64.tar.gz`. certo publishes no container image of its own; the recommended
deployment runs the release under [lode](https://github.com/bkhq/lode), a small supervisor
image (`bkhq/lode`) that downloads the asset named in `lode.toml`, checks it against the
sha256 digest GitHub publishes for it, launches certo and, when allowed, installs newer
releases by itself.

Layout on the host — the release tarball `certo-<tag>.tar.gz` ships it ready to edit:

```
certo/
├── docker-compose.yml
└── lode/
    ├── lode.toml          # how lode fetches and runs certo
    └── data/
        └── config.toml    # certo's configuration (see configuration.md)
```

```yaml
services:
  certo:
    image: bkhq/lode:v0.1.0
    container_name: certo
    restart: unless-stopped
    ports:
      - "53:53"
      - "53:53/udp"
      - "3000:3000"
    volumes:
      - ./lode:/srv/lode
```

`./lode` is mounted at `/srv/lode`: lode keeps `lode.toml`, its state and the installed
releases (`versions/`) there and runs certo with `/srv/lode` as working directory, so the
relative paths of the sample config (`./data/config.toml`, `data/db/certo.db`,
`data/certs`, `data/logs`) resolve to `lode/data/...` on the host.

The repository's [`lode.toml`](../lode.toml) is the operator's file. Edit two things:

| Key | Meaning |
|---|---|
| `[update].asset` | The release file for this CPU: `certo-linux-amd64.tar.gz` or `certo-linux-arm64.tar.gz` |
| `[update].policy` | `off` never updates, `check` only logs newer releases, `auto` installs them and restarts certo |

Releases carry no publisher signature yet, so `[trust].require_signature = "auto"` installs
them after the digest check and lode logs that the release is unsigned. When signing is
introduced, pin the key in `[trust].trusted_keys` and set `require_signature = "enforce"`.

`channel = "stable"` follows the latest GitHub release; `pin = "<version>"` stays on one
release. Checks run every `check_interval` seconds (GitHub allows 60 unauthenticated API
calls per hour and IP). Then:

```bash
docker compose up -d
docker compose exec certo lode-cli status     # installed release, last check, errors
docker compose exec certo lode-cli update     # check and install now, whatever the policy
docker compose exec certo lode-cli rollback   # back to the previous release
```

An update downloads and verifies the new release, stops certo (SIGTERM, which drains HTTP
for up to 10 s) and starts the new binary; a release that exits within `health_grace`
seconds is rolled back to the previous one automatically. A failure later than that is not:
use `lode-cli rollback` or `pin`. The running version is reported by `GET /api/info`.

Moving an existing deployment: stop the old container, move its `data/` directory to
`lode/data/`, add `lode.toml` next to it and switch to the compose file above. The database
and certificates are used as they are.

Without lode, the repository `Dockerfile` still builds a plain image (`docker build -t
certo .`, entrypoint `/app/certo`, volume `/app/data`), and the release binaries run on any
Linux host as described next.

## Binary

```bash
cd web && npm ci && npx vite build && cd ..   # build the SPA (embedded)
cargo build --locked --profile dist -p certo
./target/dist/certo -c data/config.toml
```

For a portable static binary build for musl, e.g.
`cargo zigbuild --locked --profile dist -p certo --target x86_64-unknown-linux-musl`.
Binding port 53 needs root or `CAP_NET_BIND_SERVICE`:

```bash
sudo setcap 'cap_net_bind_service=+ep' ./certo
```

`web/dist` is embedded at build time when it exists. If an on-disk `web/dist` is present
at runtime it takes priority over the embedded copy, which is handy for front-end
development.

## DNS delegation

certo must be reachable as the authoritative nameserver for its base domain. In the parent
zone (`example.com`), publish glue and a delegation for the base domain:

```
s.example.com.     NS  s.example.com.
s.example.com.     A   198.51.100.1        ; the host running certo
```

Mirror the same records in `general.records` so certo answers them itself:

```toml
records = [
    "s.example.com. A 198.51.100.1",
    "s.example.com. NS s.example.com.",
]
```

Verify before issuing certificates:

```bash
dig @198.51.100.1 s.example.com NS
dig @8.8.8.8 s.example.com NS        # delegation visible publicly
```

Users then delegate each challenge name once:

```
_acme-challenge.example.com.  CNAME  <subdomain>.s.example.com.
```

### Hosted zones

To host a full zone (forward or reverse) on certo, the parent must delegate it to certo's
nameserver (`general.nsname`, which every new zone gets as its apex NS):

```
example.net.               NS  s.example.com.               ; at the example.net registrar
23.251.162.in-addr.arpa.   NS  s.example.com.               ; requested from the IP block owner (RIR/ISP)
```

With DNSSEC enabled for a zone, publish the DS record shown in the dashboard (or returned by
`GET /api/zones/{zone}/dnssec`) at the parent; before disabling DNSSEC, remove the DS first
and wait out its TTL, or validating resolvers will treat the zone as bogus.

Secondaries transfer zones with AXFR over TCP; allow them by source address or give them a
TSIG key, and have them request with the DO bit to receive DNSSEC records. NOTIFY is not
sent; secondaries poll on the SOA refresh interval (3600 s).

## TLS for the API

Set `api.tls` (see [configuration.md](configuration.md)):

| Value | Behaviour |
|---|---|
| `none` | Plain HTTP. Use this behind a reverse proxy that terminates TLS |
| `cert` | HTTPS with `tls_cert_fullchain` + `tls_cert_privkey` |
| `letsencrypt` | HTTPS with certificates obtained automatically over ACME, solving DNS-01 **against certo's own DNS server** for `general.domain`. Certificates are cached in `acme_cache_dir` (certmagic layout, so caches from the Go release are reused) and renewed when a third of their lifetime remains |
| `letsencryptstaging` | Same against Let's Encrypt staging — use while testing |

`api.acme_directory` (and `api.acme_root_ca` for a private CA's root certificate) point the
ACME client at another CA.

Minimum TLS version is 1.2. For `letsencrypt*`, DNS delegation must already be working,
otherwise the self-issued challenge cannot be validated.

## Behind a reverse proxy

- Terminate TLS at the proxy and run certo with `api.tls = "none"`.
- Forward the real client IP and enable it, so acme-dns `allowfrom` checks are correct:
  ```toml
  [api]
  use_header = true
  header_name = "X-Forwarded-For"
  ```
- Proxy the API paths: `/present`, `/cleanup`, `/register` (POST), `/update`, `/acmedns*`,
  `/api/*`, `/admin/*`, `/health`, `/livez`, `/readyz`, `/llms.txt`, plus `/` for the
  dashboard.
- DNS traffic cannot go through an HTTP proxy — expose port 53 directly.

## Operations

- **Persistence** — everything lives under the path configured in `database.connection`
  (plus `acme_cache_dir` and any log file). Back up that directory; `data/` is
  git-ignored by design.
- **Sessions** — set `api.jwt_secret` explicitly, otherwise every restart invalidates all
  dashboard sessions.
- **Admin API** — leave `api.admin_key` empty to keep `/admin/*` disabled.
- **Health check** — `GET /livez` (process up) and `GET /readyz` (database reachable)
  return `200`; `GET /health` is kept for existing probes.
- **Upgrades** — under lode, `policy = "auto"` installs new releases by itself and
  `lode-cli update` does it on demand; otherwise replace the binary. Schema migrations run
  automatically at startup. A database created by a *newer* certo makes an older binary
  refuse to start, so roll forward, not back (a lode rollback to a release with an older
  schema fails the same way). The first Rust release upgrades the schema to version 3,
  which the Go releases cannot read: back up the database before switching.
- **Keys** — DNSSEC private keys and TSIG secrets live in the database; losing it means
  re-keying signed zones (new DS at the parent).
