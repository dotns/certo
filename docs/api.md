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
  "base_zone": "example.com",
  "api_domain": "api.example.com",
  "ns_target": "s.example.com",
  "capabilities": ["multi_key","scoped_key","domain_management","wildcard_scope",
                   "account_deletion","acmedns","zones","password_change","cname_check"],
  "registration": true
}
```
`api_domain` falls back to `base_domain` when `api.api_domain` is unset. `registration`
tells whether open registration (`POST /api/register`) is enabled.
`ns_target` is the nameserver (`general.nsname`) a hosted zone must be delegated to
before it is served. `base_zone` is the zone certo serves (`general.domain`); `base_domain`
(`general.acme_domain`) is the challenge domain inside it.

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
| `PUT` | `/api/profile/password` | `204`. Body `{"current_password","new_password"}`. Dashboard sessions only (`403 session_required` for an API key); `401 invalid_credentials` when the current password is wrong, `400 invalid_password` when the new one is shorter than 6 characters |
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
| `GET` | `/api/domains/:domain/check` | `200 {"name","expected","observed","ok"}` — looks up `_acme-challenge.<domain>` CNAME through the server's resolver; `observed` is the target found (or `null`) and `ok` whether it equals `expected` (the `cname_target`). `404 domain_not_found`, `429 check_rate_limited` (one check per 10 s per domain), `503 check_failed` (resolver error, `message` says why) |
| `GET` | `/api/records` | `200 [{"domain","value","last_update"}]` for the user's subdomains, filtered to the key's scope |

`cname_target` is the value to point `_acme-challenge.<domain>` at.

## Hosted zones

Authoritative zones (forward or reverse) owned by one user and served by the bundled
PowerDNS. Require JWT or any API key; a scoped key only sees and manages zones its scope
covers (`403 domain_not_in_scope`), and another user's zone (or the base zone) answers
`404 zone_not_found`. Zone names are given without the trailing dot and are returned
lower-cased. Validation errors carry a human-readable `message`.

A new zone is `pending`: nothing is served and rrsets cannot be changed
(`409 zone_not_active`). It becomes `active` once certo finds `ns_target` (from
`GET /api/info`, i.e. `general.nsname`) among the nameservers the parent publishes for it —
checked at creation, on `verify`, and hourly; active zones are re-checked daily and
deactivated after 7 consecutive failures (records are kept and restored on re-activation).
Administrators can exempt a zone from the check (see [Admin](#admin)).

| Method | Path | Result |
|---|---|---|
| `GET` | `/api/zones` | `200 [zone]` |
| `POST` | `/api/zones` | `201 zone`. Body `{"name"}`. Runs the delegation check immediately, so a correctly delegated zone is `active` in the response. Errors: `400 invalid_name`, `403 zone_not_allowed` (at/under the base domain, or overlapping another user's zone), `409 zone_already_exists` |
| `GET` | `/api/zones/:zone` | `200 zone` with `rrsets` when active |
| `DELETE` | `/api/zones/:zone` | `204` — removes the zone from PowerDNS with its records and keys; it stops resolving immediately |
| `POST` | `/api/zones/:zone/verify` | `200 zone` after a fresh delegation check. `429 check_rate_limited` (one check per minute per zone), `503 check_failed` when the check could not be completed (`message` says why) |
| `GET` | `/api/zones/:zone/rrsets` | `200 [rrset]`. `409 zone_not_active` |
| `PATCH` | `/api/zones/:zone/rrsets` | `200 [rrset]` (the zone after the change). Body `{"rrsets": [change]}` |
| `GET` | `/api/zones/:zone/dnssec` | `200 {"enabled","nsec_mode","dnskey"?,"ds"?:[string]}` |
| `PUT` | `/api/zones/:zone/dnssec` | `200` same shape. Body `{"enabled","nsec_mode"?}`; `nsec_mode` is `nsec` (default) or `nsec3` (`400 invalid_nsec_mode`). Enabling creates an ECDSAP256SHA256 key; disabling deletes it |
| `GET` | `/api/zones/:zone/axfr` | `200 {"allow_from":[cidr],"tsig_keys":[{"name","algorithm"}]}` |
| `PUT` | `/api/zones/:zone/axfr` | `200` same shape. Body `{"allow_from"}`: addresses or CIDRs allowed to transfer without TSIG; empty denies. `400 invalid_cidr` |
| `POST` | `/api/zones/:zone/tsig-keys` | `201 {"name","algorithm","secret"}`. Body `{"name","algorithm"?}`; `hmac-sha256` (default), `hmac-sha384`, `hmac-sha512` (`400 invalid_algorithm`). `secret` is base64 and returned **only here**. `400 tsig_key_exists` |
| `DELETE` | `/api/zones/:zone/tsig-keys/:name` | `204`. `404 tsig_key_not_found` |

```json
zone   {"name": "23.251.162.in-addr.arpa", "status": "active", "exempt": false, "system": false,
        "verified_at": 1792202451, "last_check": 1792202451, "last_error": null,
        "failed_checks": 0, "created_at": 1792202400, "ns_target": "s.example.com",
        "rrsets": [rrset]}
rrset  {"name": "5.23.251.162.in-addr.arpa.", "type": "PTR", "ttl": 3600,
        "records": ["host.example.com."]}
change {"name": "5", "type": "PTR", "ttl": 3600, "changetype": "REPLACE",
        "records": ["host.example.com."]}
```

Rrsets (the PowerDNS model: one entry per owner name and type):

- `name` in a change is relative (`www`, `@` for the apex) or absolute within the zone
  (`400 name_outside_zone`); rrsets are returned with absolute names.
- `type`: `A`, `AAAA`, `CAA`, `CNAME`, `MX`, `NS`, `PTR`, `SRV`, `TXT`; `records` in
  zone-file presentation format (`10 mail.example.com.`, `"v=spf1 -all"`,
  `0 issue "letsencrypt.org"`). Plain TXT values are quoted for you.
- `changetype` is `REPLACE` (default; `records` replaces every record of that name and
  type) or `DELETE` (removes the rrset; `records` ignored). `ttl` defaults to 3600 and
  applies to the whole rrset. A body without changes is `400 empty_change`.
- PTR only in `in-addr.arpa` / `ip6.arpa` zones; CNAME not at the apex and alone at its
  name; a `REPLACE` with no records (`400 invalid_data`). Anything PowerDNS refuses is
  `400 pdns_rejected` with its message.
- The SOA is maintained by PowerDNS; every change bumps its serial and is served
  immediately (and signed when DNSSEC is on).

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
| `PUT` | `/admin/users/:id/password` | `204` — body `{"password"}` (≥ 6 chars, else `400 invalid_password`). `404 user_not_found` |
| `GET` | `/admin/domains` | `200` all domains with `owner` and `cname_target` |
| `POST` | `/admin/domains` | `201` — body `{"user_id","domain"}`. `404 user_not_found`, `409 domain_already_exists` |
| `DELETE` | `/admin/domains/:domain` | `204` — removes that domain from every user |
| `GET` | `/admin/records` | `200` all TXT records |
| `GET` | `/admin/zones` | `200` all zones, with `owner`; includes the base zone (`"system": true`, no owner) |
| `POST` | `/admin/zones` | `201` — body `{"user_id","name"}`; the zone starts the same activation cycle as a user-created one. `400` when a field is missing, `404 user_not_found` |
| `PUT` | `/admin/zones/:zone/activation` | `200 zone` — body `{"exempt": true}` serves the zone without a delegation check and skips re-checks; `false` returns it to the normal cycle. The base zone cannot be de-exempted (`403 zone_not_allowed`) |
| any | `/admin/zones/:zone/...` | Every per-zone route of [Hosted zones](#hosted-zones), on any user's zone and on the base zone (which cannot be deleted) |

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
| `session_required` | Password change attempted with an API key instead of a dashboard session |
| `invalid_password` | New password shorter than 6 characters |
| `domain_not_found` | CNAME check for a domain the caller has not added |
| `zone_not_allowed` | Zone name at/under the base domain or overlapping another user's zone; base zone deletion or de-exemption |
| `zone_already_exists` | Zone name taken |
| `zone_not_found` / `tsig_key_not_found` | No such zone (or not yours) or TSIG key |
| `zone_not_active` | The zone is pending delegation; rrsets cannot be changed yet |
| `check_rate_limited` / `check_failed` | `verify` called within a minute of the last check (or a domain CNAME check within 10 s); the check could not be completed |
| `invalid_name` / `name_outside_zone` | Bad zone or owner name; owner not inside the zone |
| `invalid_data` / `empty_change` / `pdns_rejected` | Record type or placement rule violated (PTR, CNAME, empty REPLACE); no changes in the body; PowerDNS refused the change (`message` has its text) |
| `invalid_nsec_mode` / `invalid_cidr` / `invalid_algorithm` / `tsig_key_exists` | DNSSEC and transfer settings |
| `admin_key_not_configured` | Admin API disabled |
| `invalid_admin_key` | Wrong admin key |
| `db_error` / `internal_error` / `token_error` / `zone_error` | Server-side failure (`zone_error`: certo's database or PowerDNS) |
