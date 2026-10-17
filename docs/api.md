# HTTP API reference

All responses are JSON unless noted. Paths are relative to the API base URL
(`http(s)://<api host>:<api port>`).

## Authentication schemes

| Scheme | How | Used by |
|---|---|---|
| None | — | `/health`, `/livez`, `/readyz`, `/llms.txt`, `/api/info`, `/api/register`, `/api/login`, `POST /register` |
| JWT | `Authorization: Bearer <token>` from register/login, valid 72 h | dashboard endpoints |
| API key (Bearer) | `Authorization: Bearer <api_key>` | dashboard endpoints; scoped keys are rejected on key/profile management |
| API key (Basic) | `Authorization: Basic base64(username:api_key)` | httpreq `/present`, `/cleanup`; acme-dns storage |
| API key (headers) | `X-Api-User: <username>`, `X-Api-Key: <api_key>` | acme-dns `POST /update` |
| Admin key | `X-Admin-Key: <admin_key>` | `/admin/*` |

The dashboard password is only for login and key recovery — it never authenticates protocol
traffic.

## Public

### `GET /health`, `GET /livez`, `GET /readyz`
`200` with an empty body. No auth. `/readyz` answers `503` while the database is
unreachable.

### `GET /llms.txt`
`200`, plain text — the machine-readable API summary embedded in the binary.

### `GET /api/info`
```json
{
  "provider": "ns-certo",
  "version": "v1.0.0",
  "base_domain": "s.example.com",
  "api_domain": "api.example.com",
  "allow_zone_creation": false,
  "capabilities": ["multi_key","scoped_key","domain_management",
                   "wildcard_scope","account_deletion","acmedns","zones"]
}
```
`api_domain` falls back to `base_domain` when `api.api_domain` is unset.
`allow_zone_creation` mirrors `api.allow_zone_creation`. Clients should key
feature detection off `capabilities`, not the version.

### `POST /api/register`
Body `{"username": "...", "password": "..."}` → `201 {"token","username"}`.
Creates the account plus a default global API key.
Errors: `400` (username empty or password < 6 chars), `409 username_taken`,
`403 registration_disabled` (when `api.allow_registration = false`).

### `POST /api/login`
Body `{"username","password"}` → `200 {"token","username"}`.
Errors: `401 invalid_credentials`.

## Account & keys

Require JWT or a **global** key; a scoped key gets `403 global_key_required`.

| Method | Path | Result |
|---|---|---|
| `GET` | `/api/profile` | `200 {"username"}` |
| `DELETE` | `/api/profile` | `204` — deletes the account, its keys, domains, TXT records and hosted zones |
| `GET` | `/api/keys` | `200 [{"id","name","key","scope","created_at"}]` |
| `POST` | `/api/keys` | `201` with the new key. Body `{"name","scope"?}`; `scope` defaults to `["*"]`. `400 name_required` if `name` is empty |
| `DELETE` | `/api/keys/:id` | `204`. `400 invalid_id` for a non-numeric id |

## Domains & records

Require JWT or any API key; scope is enforced per request.

| Method | Path | Result |
|---|---|---|
| `GET` | `/api/domains` | `200 [{"domain","subdomain","cname_target"}]`, filtered to the key's scope |
| `POST` | `/api/domains` | `201 {"domain","subdomain","cname_target"}`. Body `{"domain"}`. A scoped key may only add domains within its scope (`403 domain_not_in_scope` otherwise); it does not grant itself new scope. Errors: `400 domain_required`, `403 domain_not_in_scope`, `409 domain_already_exists` |
| `DELETE` | `/api/domains/:domain` | `204`. `403 domain_not_in_scope` for a scoped key outside its scope |
| `GET` | `/api/records` | `200 [{"domain","value","last_update"}]` for the user's subdomains, filtered to the key's scope |

`cname_target` is the value to point `_acme-challenge.<domain>` at.

## Hosted zones

Authoritative zones (forward or reverse) owned by one user. Require JWT or any API key; a
scoped key only sees and manages zones its scope covers (`403 domain_not_in_scope`), and
another user's zone answers `404 zone_not_found`. Zone names are given without the trailing
dot and are returned lower-cased. Validation errors carry a human-readable `message`.

| Method | Path | Result |
|---|---|---|
| `GET` | `/api/zones` | `200 [zone]` |
| `POST` | `/api/zones` | `201 zone`. Body `{"name"}`. Needs `api.allow_zone_creation` (`403 zone_creation_disabled`). The zone gets an apex NS for `general.nsname`. Errors: `400 invalid_name`, `403 zone_not_allowed` (at/under the base domain, covering a static record, or overlapping another user's zone), `409 zone_already_exists` |
| `GET` | `/api/zones/:zone` | `200 zone` with `records` |
| `DELETE` | `/api/zones/:zone` | `204` — removes the zone, its records and keys; it stops resolving immediately |
| `GET` | `/api/zones/:zone/records` | `200 [record]` |
| `POST` | `/api/zones/:zone/records` | `201 record`. Body `{"name","type","ttl"?,"data"}` |
| `PUT` | `/api/zones/:zone/records/:id` | `200 record`. Same body; replaces the record. `404 record_not_found` |
| `DELETE` | `/api/zones/:zone/records/:id` | `204`. `404 record_not_found` |
| `GET` | `/api/zones/:zone/dnssec` | `200 {"enabled","nsec_mode","dnskey"?,"ds"?}` |
| `PUT` | `/api/zones/:zone/dnssec` | `200` same shape. Body `{"enabled","nsec_mode"?}`; `nsec_mode` is `nsec` (default) or `nsec3` (`400 invalid_nsec_mode`). Enabling creates an ECDSAP256SHA256 key; disabling deletes it |
| `GET` | `/api/zones/:zone/axfr` | `200 {"allow_from":[cidr],"tsig_keys":[{"name","algorithm","created_at"}]}` |
| `PUT` | `/api/zones/:zone/axfr` | `200` same shape. Body `{"allow_from"}`: addresses or CIDRs allowed to transfer without TSIG; empty denies. `400 invalid_cidr` |
| `POST` | `/api/zones/:zone/tsig-keys` | `201 {"name","algorithm","created_at","secret"}`. Body `{"name","algorithm"?}`; `hmac-sha256` (default), `hmac-sha384`, `hmac-sha512` (`400 invalid_algorithm`). `secret` is base64 and returned **only here**. `400 tsig_key_exists` |
| `DELETE` | `/api/zones/:zone/tsig-keys/:name` | `204`. `404 tsig_key_not_found` |

```json
zone   {"name": "23.251.162.in-addr.arpa", "serial": 1792202451, "dnssec": false,
        "created_at": 1792202451, "records": [record]}
record {"id": 7, "name": "5.23.251.162.in-addr.arpa", "type": "PTR", "ttl": 3600,
        "data": "host.example.com."}
```

Records:

- `name` is relative (`www`, `@` for the apex) or absolute within the zone.
- `type`: `A`, `AAAA`, `CAA`, `CNAME`, `MX`, `NS`, `PTR`, `SRV`, `TXT`; `data` in zone-file
  presentation format (`10 mail.example.com`, `"v=spf1 -all"`, `0 issue "letsencrypt.org"`).
  Names in `data` are made absolute; unquoted TXT is one string, longer strings are split
  at 255 bytes.
- `ttl` defaults to 3600, maximum 2147483647.
- PTR only in `in-addr.arpa` / `ip6.arpa` zones; CNAME not at the apex and not next to
  other records at the same name.
- Every change bumps the SOA serial and is served immediately (and re-signed when DNSSEC is
  on).

## httpreq protocol

Basic Auth with username + API key. See [protocols.md](protocols.md#lego-httpreq).

### `POST /present`
Body `{"fqdn","value"}` → `200 {"internal_domain","cname_target"}`.
Stores a TXT record. When the domain is not registered yet it is created, provided the
key's scope covers it. `value` may be at most 255 bytes.
Errors: `400 invalid_json`, `400 missing_fqdn_or_value`, `400 value_too_long`,
`401 unauthorized`, `403 domain_not_in_scope`, `403 domain_not_authorized`, `500 db_error`.

### `POST /cleanup`
Body `{"fqdn","value"}` → `200`. Removes that exact value. Same error set.

## acme-dns protocol

Native paths, wire-compatible with [acme-dns](https://github.com/acme-dns/acme-dns#api).

### `POST /register`
No auth. Optional body `{"allowfrom": ["1.2.3.4/32", "2001:db8::/48"]}`.
```json
201 {
  "username":   "acme-<16 chars>",
  "password":   "<32-char api key>",
  "fulldomain": "<subdomain>.s.example.com",
  "subdomain":  "<10 chars>",
  "allowfrom":  ["1.2.3.4/32"]
}
```
Allocates a standalone account bound to a fresh random subdomain. `allowfrom`, when
non-empty, restricts which source IPs may call `/update` for it.
Errors: `403 registration_disabled` (when `api.allow_registration = false`), `500 db_error`.

> `GET /register` is **not** this endpoint — it serves the dashboard's signup page.

### `POST /update`
Headers `X-Api-User`, `X-Api-Key`. Body `{"subdomain","txt"}` → `200 {"txt"}`.
`txt` must be exactly 43 characters. Only the two most recent values per subdomain are
kept.
Errors: `400 invalid_json`, `400 bad_txt` (wrong length), `401 unauthorized`,
`403 forbidden` (unknown subdomain, or not owned by this user), `403 domain_not_in_scope`,
`403 ip_not_allowed` (source IP outside `allowfrom`), `500 db_error`.

### HTTP storage backend

Implements lego's `ACME_DNS_STORAGE_BASE_URL`. Basic Auth with certo username + API key —
lego sends credentials as URL userinfo. `:domain` is the certificate domain; a leading
`*.`, URL-escaping and a trailing dot are normalised away, so `*.example.com` and
`example.com` resolve to the same record.

| Method | Path | Result |
|---|---|---|
| `GET` | `/acmedns/:domain` | `200` account object; creates the domain if the key's scope covers it |
| `POST` | `/acmedns/:domain` | `200` — same get-or-create, for lego's `Put` |
| `GET` | `/acmedns` | `200 {"<domain>": {account}, …}` for the user's in-scope domains |

Account object:
```json
{
  "fulldomain": "<subdomain>.s.example.com",
  "subdomain":  "<subdomain>",
  "username":   "<certo username>",
  "password":   "<the api key used to authenticate>",
  "server_url": "https://api.example.com"
}
```
`server_url` is the **API base** (where `/update` lives), not the storage path.
Errors: `401 unauthorized`, `403 domain_not_in_scope`, `404 domain_not_found`,
`500 db_error`.

## Admin

All require `X-Admin-Key`. Disabled while `api.admin_key` is empty
(`403 admin_key_not_configured`); a wrong key gives `401 invalid_admin_key`.

| Method | Path | Result |
|---|---|---|
| `GET` | `/admin/users` | `200` all users |
| `POST` | `/admin/users` | `201` — body `{"username","password"}` (password ≥ 6 chars). `409 username_taken` |
| `DELETE` | `/admin/users/:id` | `204`; also removes the user's keys, domains and zones. `400 invalid_user_id` |
| `GET` | `/admin/domains` | `200` all domains with `owner` and `cname_target` |
| `POST` | `/admin/domains` | `201` — body `{"user_id","domain"}`. `404 user_not_found`, `409 domain_already_exists` |
| `DELETE` | `/admin/domains/:domain` | `204` — removes that domain from every user |
| `GET` | `/admin/records` | `200` all TXT records |
| `GET` | `/admin/zones` | `200` all zones, with `owner` |
| `POST` | `/admin/zones` | `201` — body `{"user_id","name"}`, regardless of `api.allow_zone_creation`. `400` when a field is missing, `404 user_not_found` |
| any | `/admin/zones/:zone/...` | Every per-zone route of [Hosted zones](#hosted-zones), on any user's zone |

## Error codes

Errors are `{"error": "<code>"}`, plus `"message"` with details for zone and record
validation errors.

| Code | Meaning |
|---|---|
| `unauthorized` | Missing or invalid credentials |
| `forbidden` | Subdomain unknown or not owned by the caller |
| `invalid_credentials` | Wrong username or password (login) |
| `username_taken` | Username already registered |
| `global_key_required` | A scoped key tried to manage keys or the profile |
| `domain_not_authorized` | Domain not registered for this user and not creatable |
| `domain_not_in_scope` | Outside the API key's scope |
| `domain_already_exists` | Already registered by this user |
| `domain_not_found` | No such storage account for this domain |
| `domain_required` | Empty domain in the request |
| `value_too_long` | httpreq `/present` value exceeds 255 bytes (a single DNS string) |
| `registration_disabled` | Open registration turned off (`api.allow_registration = false`) |
| `ip_not_allowed` | Client IP outside the registration's `allowfrom` |
| `bad_txt` | acme-dns `txt` is not exactly 43 characters |
| `missing_fqdn_or_value` | `/present` or `/cleanup` missing a field |
| `invalid_json` | Malformed request body |
| `invalid_id` / `invalid_user_id` | Non-numeric path id |
| `name_required` | Key creation without a name |
| `user_not_found` | Unknown `user_id` (admin) |
| `zone_creation_disabled` | Self-service zone creation is off (`api.allow_zone_creation = false`) |
| `zone_not_allowed` | Zone name at/under the base domain, covering a static record, or overlapping another user's zone |
| `zone_already_exists` | Zone name taken |
| `zone_not_found` / `record_not_found` / `tsig_key_not_found` | No such zone (or not yours), record or TSIG key |
| `invalid_name` / `name_outside_zone` / `name_reserved` | Bad zone or owner name; owner not inside the zone; owner inside the server's own domain |
| `unsupported_type` / `invalid_data` / `invalid_ttl` | Record type not supported; RDATA does not parse; TTL too large |
| `ptr_outside_reverse_zone` / `cname_at_apex` / `cname_conflict` | Record placement rules |
| `invalid_nsec_mode` / `invalid_cidr` / `invalid_algorithm` / `tsig_key_exists` | DNSSEC and transfer settings |
| `admin_key_not_configured` | Admin API disabled |
| `invalid_admin_key` | Wrong admin key |
| `db_error` / `internal_error` / `token_error` / `zone_error` | Server-side failure |
