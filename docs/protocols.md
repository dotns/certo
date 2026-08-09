# Client protocols

certo answers DNS-01 challenges for two client protocols. **Both write the same record**,
so a domain can be driven by either, and switching protocols needs no DNS change.

Common prerequisites:

1. An account and an API key — register in the dashboard, or purely over the API:
   ```bash
   TOKEN=$(curl -s -X POST https://api.example.com/api/register \
     -H 'Content-Type: application/json' \
     -d '{"username":"myuser","password":"password123"}' | jq -r .token)
   API_KEY=$(curl -s https://api.example.com/api/keys \
     -H "Authorization: Bearer $TOKEN" | jq -r '.[0].key')
   ```
2. One CNAME per certificate domain, pointing at the subdomain certo allocated:
   ```
   _acme-challenge.example.com.  CNAME  <subdomain>.s.example.com.
   ```
   `<subdomain>.s.example.com` is the `cname_target` / `fulldomain` shown in the dashboard,
   `GET /api/domains`, or the acme-dns storage response. It is stable for a given
   (user, domain), so it survives a database rebuild.

Domains do not have to be pre-created: the first `/present` or storage fetch creates them,
as long as the API key's scope covers the domain.

## lego httpreq

Set the endpoint plus your username and API key.

```bash
LEGO_DISABLE_CNAME_SUPPORT=true \
HTTPREQ_ENDPOINT=https://api.example.com \
HTTPREQ_USERNAME=myuser \
HTTPREQ_PASSWORD=<api_key> \
lego --dns httpreq \
  --dns.propagation-disable-ans \
  --domains example.com \
  --domains "*.example.com" \
  --email admin@example.com \
  --accept-tos run
```

- `LEGO_DISABLE_CNAME_SUPPORT=true` — stops lego rewriting the FQDN, so certo receives the
  original `_acme-challenge.<domain>` name.
- `--dns.propagation-disable-ans` — the TXT lives on certo's nameserver, not on the
  domain's authoritative NS, so lego's authoritative check must be skipped.

### Traefik

```yaml
# traefik.yml
certificatesResolvers:
  letsencrypt:
    acme:
      email: admin@example.com
      storage: /data/ssl/acme.json
      dnsChallenge:
        provider: httpreq
        propagation:
          disableChecks: true
```

```yaml
# environment
LEGO_DISABLE_CNAME_SUPPORT: "true"
HTTPREQ_ENDPOINT: "https://api.example.com"
HTTPREQ_USERNAME: "myuser"
HTTPREQ_PASSWORD: "<api_key>"
```

## acme-dns

The acme-dns API is unchanged from upstream — `/register` and `/update` sit on the root
path, so any acme-dns client works. What differs is that certo *also* implements lego's
**HTTP storage backend**, which stores and serves the account so you do not need to seed a
local file.

`ACME_DNS_API_BASE` always points at the API root. Then pick **one** storage mode
(lego rejects both at once).

### Mode A — HTTP storage (recommended, nothing to seed)

```bash
ACME_DNS_API_BASE=https://api.example.com \
ACME_DNS_STORAGE_BASE_URL=https://myuser:<api_key>@api.example.com/acmedns \
lego --dns acme-dns \
  --domains example.com \
  --domains "*.example.com" \
  --email admin@example.com \
  --accept-tos run
```

lego fetches the account for the certificate domain from `/acmedns/<domain>`, then sends
TXT updates to `POST /update`. The credentials must live in the URL userinfo — lego's HTTP
storage has no other auth mechanism. The domain is created on first fetch (subject to key
scope), and `/register` is never called.

### Mode B — local file storage (stock acme-dns)

```bash
ACME_DNS_API_BASE=https://api.example.com \
ACME_DNS_STORAGE_PATH=/etc/lego/acmedns.json \
lego --dns acme-dns --domains example.com --email admin@example.com --accept-tos run
```

With an empty storage file, lego calls `POST /register`. certo allocates a standalone
`acme-<nanoid>` account bound to a fresh random subdomain, lego saves it, and stops with
the one-time CNAME instruction. Create that CNAME, re-run, and issuance proceeds. This is
exactly the upstream acme-dns flow and needs no certo dashboard account.

To restrict who may update that registration, register with an allow list:

```bash
curl -s -X POST https://api.example.com/register \
  -H 'Content-Type: application/json' \
  -d '{"allowfrom":["203.0.113.10/32"]}'
```

Later `POST /update` calls from other source IPs are refused with `403 ip_not_allowed`. An
empty or omitted `allowfrom` means no restriction. Behind a reverse proxy, set
`api.use_header` and `api.header_name` so the real client IP is used.

### Using an existing certo account with acme-dns

An existing account already *is* a valid acme-dns account:

| acme-dns field | certo value |
|---|---|
| `username` | your certo username |
| `password` | an API key whose scope covers the domain |
| `subdomain` | the domain's existing (deterministic) subdomain |
| `fulldomain` | `<subdomain>.<base domain>` — the same CNAME target httpreq uses |
| `server_url` | the API base URL |

Either use Mode A, which serves exactly that object, or pre-seed Mode B's file so lego
skips `/register`:

```json
{
  "example.com": {
    "username": "myuser",
    "password": "<api_key>",
    "fulldomain": "a1b2c3d4.s.example.com",
    "subdomain": "a1b2c3d4",
    "server_url": "https://api.example.com"
  }
}
```

The ready-to-paste object is what the storage endpoint returns:

```bash
curl -s -u myuser:<api_key> https://api.example.com/acmedns/example.com
```

Because the subdomain is the same one httpreq uses, an existing CNAME keeps working — no
re-delegation.

### Raw protocol calls

```bash
# publish a challenge value (must be exactly 43 characters)
curl -s -X POST https://api.example.com/update \
  -H "X-Api-User: myuser" -H "X-Api-Key: <api_key>" \
  -H 'Content-Type: application/json' \
  -d '{"subdomain":"a1b2c3d4","txt":"LHDhK3oGRvkiefQnx7OOczTY5Tic_xZ6HcMOc_gmtoM"}'

# verify it resolves
dig +short a1b2c3d4.s.example.com TXT
```

## Choosing between them

| | httpreq | acme-dns |
|---|---|---|
| Client config | endpoint + username + password | API base + storage (URL or file) |
| Client support | lego, Traefik | lego, acme.sh, cert-manager, certbot plugins, … |
| Record cleanup | explicit `/cleanup` | none — certo keeps the 2 newest values |
| Needs a certo account | yes | no (Mode B can self-register) |
| TXT value constraint | any | exactly 43 characters |
