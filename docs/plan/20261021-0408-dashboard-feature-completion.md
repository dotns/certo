# 20261021-0408-dashboard-feature-completion Complete the dashboard feature set

- **status**: completed
- **completedAt**: 2026-10-22 02:02
- **approvedAt**: 2026-10-22 01:45
- **createdAt**: 2026-10-21 04:08
- **relatedTask**: 20261021-0408-dashboard-feature-completion

## Context

`web/` (React 19, TanStack Router/Query, shadcn base-nova, 41 Vitest tests) covers the
API endpoint by endpoint: domains + TXT list, keys, zones (records, DNSSEC, AXFR/TSIG),
client snippets, admin (zones, users, domains, records). Missing against what an
operator and a tenant need day to day:

- `/` redirects straight to `/domains`; there is no overview of the server
  (`base_zone`, `base_domain`, `ns_target`, version) or of the account.
- No account page: `DELETE /api/profile` has no UI, passwords cannot be changed
  (no endpoint either), and `GET /api/info` does not say whether registration is open.
- Domains: no search, no way to see whether the `_acme-challenge` CNAME is in place,
  no per-domain view of its tokens.
- Keys: the created key is only visible in the table, abbreviated; the Clients page uses
  a `<your-api-key>` placeholder and knows only lego and Traefik.
- Zones: records cannot be filtered, imported (zone-file paste) or exported; the detail
  page has no delete action and no metadata (created, verified, owner).
- Admin: users carry no counts, no password reset (no endpoint), tables cannot be
  filtered, nothing shows the server configuration.
- No success feedback after mutations; the top navigation overflows on narrow screens.

## Proposal

Backend (`certo-api`, `certo-db`; all small, tested in `tests/api.rs` + e2e):

1. `PUT /api/profile/password` body `{"current_password","new_password"}` (JWT only;
   `401 invalid_credentials` on a wrong current password, `400 invalid_password` < 6).
   `PUT /admin/users/:id/password` body `{"password"}`. New `Db::set_password_hash`.
2. `GET /api/domains/:domain/check` → `{"cname": <observed target or null>, "ok": bool}`
   using the configured resolver (`DnsDelegationChecker`, new `cname` lookup); one check
   per 10 s per domain (`429 check_rate_limited`), `503 check_failed` on resolver error.
3. `GET /api/info` adds `"registration": true|false`; `capabilities` gains
   `password_change`, `cname_check`. (The `acme_domain` alias was dropped: `base_domain`
   already carries it.)

Web:

4. **Overview** (`/overview`, new home after sign-in): server card (version, base zone,
   challenge domain, NS target, API domain, registration open/closed), account counters
   (domains, published tokens, zones by status, keys), and a three-step quick start
   (add domain → set CNAME → configure client) linking to the pages.
5. **Account** (`/account`): username, change password form, delete account (type the
   username to confirm, then sign out). Header gets a user menu (account, sign out).
6. **Domains**: search box; row action "Check CNAME" showing observed vs expected
   target; a row expands to the tokens currently published for that domain; the Add
   dialog shows the resulting CNAME line (zone-file form) to copy right after creation.
7. **API keys**: after creation a dialog reveals the full key once with copy and a
   direct link to Clients pre-selecting it. Clients page: key selector (real keys, or
   placeholder), tabs httpreq / acme-dns, and snippet sets for lego, Traefik, Caddy
   (`caddy-dns/acmedns`), acme.sh (`dns_acmedns`) and cert-manager (`ACMEDNS` solver).
8. **Zones**: record table gets a text filter (name/type/content) and a type chip
   filter; "Export" downloads `<zone>.zone` built from the rrsets (SOA included
   read-only); "Import" pastes zone-file lines (`name [ttl] [IN] TYPE rdata`, `$ORIGIN`
   and `$TTL` honoured, `@` and relative names) → preview table → one `PATCH` with
   REPLACE changes grouped by name/type (existing sets of the same name/type are
   replaced; nothing is deleted). Detail page: metadata row (created, verified, last
   check, owner), "Delete zone" action for non-system zones.
9. **Admin**: Users tab shows per-user domain and zone counts (from the existing admin
   lists) and a "Reset password" action; Zones/Domains/Users/Records tables get a
   filter box; a "Server" tab shows the `GET /api/info` values and the base zone link.
10. **Cross-cutting**: toast notifications (shadcn `sonner`) on successful mutations;
    header collapses to a menu below `md`; `ErrorAlert` descriptions for the new codes;
    tests for every new hook, parser (zone file import/export) and snippet set;
    Playwright smoke script extended to the new screens.
11. Docs: `docs/api.md`, `llms.txt`, README screenshots list / feature bullets,
    `docs/README.md`; changelog; release `v0.2.0`.

Out of scope (say so if wanted): i18n (zh-CN UI), key scope editing, moving a zone to
another owner, TOTP/2FA, audit log — each needs its own backend design.

## Steps

1. Backend endpoints + `Db::set_password_hash` + `cname` lookup, RED → GREEN in
   `crates/certo-api/tests/api.rs` and `tests/e2e` → `just rs-ci` + e2e green.
2. Shared: toasts, user menu, mobile nav, Overview and Account pages → web gates green.
3. Domains + Keys + Clients → web gates green, snippet tests.
4. Zones: filter, export, import parser (`zonefile.ts`, unit-tested) and dialogs.
5. Admin: counts, filters, reset password, Server tab.
6. Smoke run in the sibling (Playwright screenshots), docs, changelog, tag `v0.2.0`.

## Risks

- The CNAME check depends on the server's resolver; in offline deployments it reports
  `check_failed` — the UI shows that as "could not check", never as "wrong".
- Zone-file import covers the record types the dashboard manages (A, AAAA, CNAME, MX,
  TXT, NS, PTR, SRV, CAA); other lines are listed as skipped in the preview.
- `sonner` is a new dependency (shadcn's toast primitive; latest stable verified at
  install time).

## Alternatives

- Only add missing API coverage (account deletion, info fields) without the new screens:
  smaller, but leaves the "no overview / no feedback / placeholder key" gaps that make
  the dashboard feel incomplete.
- Full i18n now: doubles the UI surface to maintain; deferred until the feature set
  settles.
