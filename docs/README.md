# certo documentation

DNS TXT record management server for ACME DNS-01 challenges. Speaks two client
protocols — lego [httpreq](https://go-acme.github.io/lego/dns/httpreq/) and native
[acme-dns](https://github.com/acme-dns/acme-dns#api) — over the same records, and hosts
authoritative forward and reverse zones per user, with optional DNSSEC and AXFR.

| Document | Contents |
|---|---|
| [architecture.md](architecture.md) | Crates, request flows, DNS resolution and hosted zones, database schema, identifier formats |
| [configuration.md](configuration.md) | Every config key, its actual code default, and env overrides |
| [api.md](api.md) | Full HTTP API reference: auth, endpoints, status codes, error codes |
| [protocols.md](protocols.md) | Client setup for httpreq and acme-dns (incl. using an existing account) |
| [deployment.md](deployment.md) | Docker/binary deployment, DNS and zone delegation, DNSSEC, TLS options |
| [development.md](development.md) | Build, test, e2e suite, release |
| [decisions/](decisions/) | Architecture decision records |
| [changelog.md](changelog.md) | Progress, decisions and pitfalls log |

Start here: [README](../README.md) ([中文](../README_CN.md)) for a quick overview and
quick-start.

## Concepts in one page

- **Base domain** (`general.domain`) — the zone certo is authoritative for, e.g.
  `s.example.com`. Every managed record lives at `<subdomain>.<base domain>`.
- **Subdomain** — the label a challenge TXT is stored under. Dashboard/httpreq domains get
  a deterministic 8-char subdomain; anonymous acme-dns registrations get a random 10-char
  one. See [architecture.md](architecture.md#identifier-formats).
- **CNAME delegation** — you point `_acme-challenge.<your domain>` at the subdomain once,
  so the CA follows the CNAME into certo and your primary DNS zone stays untouched.
- **Account vs API key** — the username/password is only for the dashboard. All protocol
  traffic authenticates with an **API key**; keys carry a domain **scope**.
- **One record, two protocols** — httpreq `/present` and acme-dns `/update` write the same
  `txt` rows for the same subdomain, so either client can drive a domain.
- **Hosted zone** — a whole zone (e.g. `example.net` or `23.251.162.in-addr.arpa`) owned by
  one user and served authoritatively once the parent delegates it to certo's nameserver.
  Separate from the base domain, which only carries challenge records.
