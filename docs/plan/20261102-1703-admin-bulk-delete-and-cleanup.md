# 20261102-1703-admin-bulk-delete-and-cleanup Bulk deletion of users and domains and cleanup of unused acme-dns registrations

- **status**: completed
- **completedAt**: 2026-11-02 17:45
- **approvedAt**: 2026-11-02 17:30
- **createdAt**: 2026-11-02 17:03
- **relatedTask**: 20261102-1703-admin-bulk-delete-and-cleanup

## Context

`DELETE /admin/users/{id}` and `DELETE /admin/domains/{domain}` act on one row; the
dashboard tables (`admin-users.tsx`, `admin-domains.tsx`) have one trash button per row.
Anonymous acme-dns registrations are users with `password_hash = '!'` and the
`acme-` username prefix, each with one `user_domains` row (`domain == subdomain`) and one
key. In the owner's database (2026-11-02): 551 users of which 547 anonymous, 546 of those
without any `txt` row; no orphan rows (every domain has a user). "Domains without any
user" therefore means these never-used registrations.

## Proposal

1. **Bulk endpoints** (`certo-api/admin.rs`): `POST /admin/users/delete {"ids": [..]}`
   and `POST /admin/domains/delete {"domains": [..]}`, both answering
   `200 {"deleted": n}`; each item goes through the existing single-row path
   (`delete_user_for`, `remove_user_domain` + TXT sync), unknown ids/domains are skipped,
   empty lists are `400`.
2. **Cleanup**: `certo-db` gains `stale_acmedns_accounts(before)` (anonymous accounts
   created before `before` whose subdomains have no `txt` row). `GET /admin/cleanup?older_than_days=N`
   (default 7) answers `{"accounts": n, "older_than_days": N}`; `POST /admin/cleanup
   {"older_than_days": N}` deletes them through `delete_user_for` and answers
   `{"deleted": n}`. Only anonymous accounts qualify; dashboard accounts are never touched.
3. **Dashboard**: a checkbox column in the Users and Domains tables (header checkbox
   selects the visible/filtered rows), a "Delete selected (n)" button with a confirm
   dialog, and on the Users tab a "Clean up" button opening a dialog with the days input
   and the live preview count. shadcn `checkbox` component added (base-nova). en + zh-CN
   keys.
4. **Tests**: certo-db test for the stale query (anonymous vs. real, with/without TXT,
   age); API tests for the four endpoints; web hook tests; e2e admin cases.
5. **Docs**: api.md, llms.txt, served llms-api.txt, changelog.

6. **Challenge TXT retention** (owner, 2026-11-02): ACME challenge values are validated
   within minutes and useless afterwards, but acme-dns clients never remove them. The
   hourly loop in `main.rs` gains a sweep: `txt` rows with `LastUpdate` older than
   `CHALLENGE_RETENTION` (2 h, owner's choice over 24 h) are deleted and the affected names re-synced
   (`Db::expire_txt(before) -> Vec<String>`); also run once at startup. A row with
   `LastUpdate` NULL (Go era) counts as expired. Same sweep deletes stale acme-dns
   accounts (item 2 with the default age), so the cleanup becomes automatic too.
7. **Amplification hardening**: the bundled PowerDNS is authoritative only (no recursion,
   so it cannot act as an open resolver) and already answers `ANY` with TC=1
   (`any-to-tcp=yes`) and truncates UDP answers above 1232 bytes
   (`udp-truncation-threshold`). Both are written explicitly into the generated
   `pdns.conf` so a future default change cannot regress them. EDNS cookies are not
   possible: the bundled static `pdns_server` is built without libsodium and exits when
   `edns-cookie-secret` is set (found while implementing). PowerDNS
   Authoritative has no response rate limiting; `deployment.md` documents an nftables
   per-source rate limit for UDP/53 and names dnsdist as the option for real RRL.
8. **Wording** (owner: `zones.description` is off): en "Authoritative forward and reverse
   zones. Delegate a zone's NS to {{ns}}; it is served once the next delegation check
   passes."; zh-CN "权威正向与反向解析区域。把区域的 NS 委派到 {{ns}}，通过下一次委派检查后即对外解析。";
   `admin.zones.hint` zh-CN "始终服务" → "始终对外解析".

Out of scope: deleting hosted zones in bulk; response rate limiting inside certo (needs
dnsdist in front).

## Steps

1. certo-db `stale_acmedns_accounts` with test → nextest green.
2. API endpoints with tests → clippy, nextest green.
3. Dashboard selection, bulk delete, cleanup dialog, hooks + tests, i18n → web gates.
4. TXT retention sweep + stale-account sweep in the hourly loop with tests; pdns.conf
   hardening settings with config test → nextest green.
5. e2e admin cases, docs (api.md, llms.txt, deployment.md hardening section), wording,
   changelog → e2e green; commit and push.

## Risks

- Bulk deletion of many users runs one PowerDNS TXT sync per subdomain; 500 deletions
  take a few seconds, acceptable for an admin action.
- A registration created moments ago has no TXT yet; the age threshold (default 7 days)
  keeps it.

## Alternatives

- Cleanup by "no TXT" only, without age: would delete registrations still being set up.
- A single `DELETE` with a query string instead of `POST …/delete`: long id lists do not
  fit URLs well.
