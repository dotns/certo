# HTTP API reference

All responses are JSON unless noted. Paths are relative to the API base URL
(`http(s)://<api host>:<api port>`).

## Authentication schemes

| Scheme | How | Used by |
|---|---|---|
| None | — | `/health`, `/llms.txt`, `/api/info`, `/api/register`, `/api/login`, `POST /register` |
| JWT | `Authorization: Bearer <token>` from register/login, valid 72 h | dashboard endpoints |
| API key (Bearer) | `Authorization: Bearer <api_key>` | dashboard endpoints; scoped keys are rejected on key/profile management |
| API key (Basic) | `Authorization: Basic base64(username:api_key)` | httpreq `/present`, `/cleanup`; acme-dns storage |
| API key (headers) | `X-Api-User: <username>`, `X-Api-Key: <api_key>` | acme-dns `POST /update` |
| Admin key | `X-Admin-Key: <admin_key>` | `/admin/*` |

The dashboard password is only for login and key recovery — it never authenticates protocol
traffic.

## Public

### `GET /health`
`200` with an empty body. No auth.

### `GET /llms.txt`
`200`, plain text — the machine-readable API summary embedded in the binary.

### `GET /api/info`
```json
{
  "provider": "ns-certo",
  "version": "v1.0.0",
  "base_domain": "s.example.com",
  "api_domain": "api.example.com",
  "capabilities": ["multi_key","scoped_key","domain_management",
                   "wildcard_scope","account_deletion","acmedns"]
}
```
`api_domain` falls back to `base_domain` when `api.api_domain` is unset. Clients should key
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
| `DELETE` | `/api/profile` | `204` — deletes the account, its keys, domains and TXT records |
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
| `DELETE` | `/admin/users/:id` | `204`; also removes the user's keys and domains. `400 invalid_user_id` |
| `GET` | `/admin/domains` | `200` all domains with `owner` and `cname_target` |
| `POST` | `/admin/domains` | `201` — body `{"user_id","domain"}`. `404 user_not_found`, `409 domain_already_exists` |
| `DELETE` | `/admin/domains/:domain` | `204` — removes that domain from every user |
| `GET` | `/admin/records` | `200` all TXT records |

## Error codes

Errors are `{"error": "<code>"}`.

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
| `admin_key_not_configured` | Admin API disabled |
| `invalid_admin_key` | Wrong admin key |
| `db_error` / `internal_error` / `token_error` | Server-side failure |
