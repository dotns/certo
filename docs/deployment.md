# Deployment

certo is a single binary that serves the DNS zone, the HTTP API and the dashboard SPA. It
needs port 53 (UDP+TCP) for DNS and one HTTP(S) port for the API.

## Docker Compose

```yaml
services:
  certo:
    image: dotns/certo:latest
    container_name: certo
    restart: unless-stopped
    ports:
      - "53:53"
      - "53:53/udp"
      - "3000:3000"
    volumes:
      - ./data:/app/data
```

The image's entrypoint is `/app/certo`, which reads `./data/config.toml` by default, and
`/app/data` is declared as a volume. So create `data/config.toml` (see
[configuration.md](configuration.md)) and:

```bash
docker compose up -d
```

Images are published for `linux/amd64` and `linux/arm64` on tag pushes (`v*`) by
`.github/workflows/docker.yml`, which also attaches a release tarball containing
`config.toml` and `docker-compose.yml`.

## Binary

```bash
cd web && npm ci && npx vite build && cd ..   # build the SPA (embedded)
CGO_ENABLED=0 go build -o certo .
./certo -c data/config.toml
```

`CGO_ENABLED=0` is intentional — the SQLite driver is pure Go, so the result is static.
Binding port 53 needs root or `CAP_NET_BIND_SERVICE`:

```bash
sudo setcap 'cap_net_bind_service=+ep' ./certo
```

`web/dist` must exist at build time (it is embedded via `web/embed.go`). If an on-disk
`web/dist` is present at runtime it takes priority over the embedded copy, which is handy
for front-end development.

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

## TLS for the API

Set `api.tls` (see [configuration.md](configuration.md)):

| Value | Behaviour |
|---|---|
| `none` | Plain HTTP. Use this behind a reverse proxy that terminates TLS |
| `cert` | HTTPS with `tls_cert_fullchain` + `tls_cert_privkey` |
| `letsencrypt` | HTTPS with certificates obtained automatically via certmagic, solving DNS-01 **against certo's own DNS server** for `general.domain`. Certificates are cached in `acme_cache_dir` |
| `letsencryptstaging` | Same against Let's Encrypt staging — use while testing |

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
  `/api/*`, `/admin/*`, `/health`, `/llms.txt`, plus `/` for the dashboard.
- DNS traffic cannot go through an HTTP proxy — expose port 53 directly.

## Operations

- **Persistence** — everything lives under the path configured in `database.connection`
  (plus `acme_cache_dir` and any log file). Back up that directory; `data/` is
  git-ignored by design.
- **Sessions** — set `api.jwt_secret` explicitly, otherwise every restart invalidates all
  dashboard sessions.
- **Admin API** — leave `api.admin_key` empty to keep `/admin/*` disabled.
- **Health check** — `GET /health` returns `200`; use it as the container probe.
- **Upgrades** — schema migrations run automatically at startup. A database created by a
  *newer* certo makes an older binary refuse to start, so roll forward, not back.
