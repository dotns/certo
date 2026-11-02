# 20261102-1006-certo-db-source-of-truth Keep every DNS fact in certo.db and derive PowerDNS from it

- **status**: completed
- **approvedAt**: 2026-11-02 11:35
- **completedAt**: 2026-11-02 11:57
- **createdAt**: 2026-11-02 10:06
- **relatedTask**: 20261102-1006-certo-db-source-of-truth

## Context

Since the PowerDNS data plane (decision 2026-10-18) certo.db is the source of truth only
for users, API keys, challenge TXT (`txt`) and zone ownership/activation (`zones`, with a
JSON `snapshot` of rrsets taken when a zone is deactivated). Hosted-zone rrsets, DNSSEC
keys, TSIG secrets and the `ALLOW-AXFR-FROM` / `TSIG-ALLOW-AXFR` metadata live only in
`pdns.sqlite3`. Startup rebuilds just the base zone and the challenge TXT
(`ensure_base_zone`, `reconcile_txt`); nothing removes PowerDNS zones the database does
not know (the stale `s.dnsall.com` zone on the owner's server) and a lost PowerDNS file
loses every hosted zone. The schema was reset to version 1 on 2026-11-02 and the owner's
database is still to be migrated by hand, so tables can be added without a migration.

## Proposal

1. **Schema** (`certo-db`, still version 1): new tables `zone_rrsets(zone_id, name, rtype,
   ttl, records JSON, PRIMARY KEY (zone_id, name, rtype))`, `zone_dnssec(zone_id PRIMARY
   KEY, nsec3, algorithm, private_key)` (PowerDNS private key content in ISC format, so a
   re-import keeps the DNSKEY and DS), `zone_tsig_keys(zone_id, name, algorithm, secret,
   PRIMARY KEY (zone_id, name))`, column `zones.axfr_allow_from` (JSON list). The
   `zones.snapshot` column goes away (records stay in `zone_rrsets` while a zone is
   inactive). The base zone is a zone like any other: its static records come from
   `general.records` the first time the system row is created, then from the dashboard;
   challenge TXT rrsets are derived from `txt` at sync time and never stored twice.
   Deleting a zone or a user cascades to the new tables.
2. **Write path** (`Zones`): PowerDNS validates and applies first, certo.db records
   second — rrset patches are applied to `zone_rrsets` as the same REPLACE/DELETE
   changes; `set_dnssec` stores the key content fetched from `GET …/cryptokeys/{id}`
   (new client call) and the NSEC mode; TSIG creation stores the secret PowerDNS
   generated; the allow list is stored and pushed. Reads of rrsets, DNSSEC state (except
   the derived DNSKEY/DS strings), transfer settings and TSIG lists come from certo.db.
   The dashboard record table no longer shows the SOA row (PowerDNS maintains it).
3. **Sync** (`Zones::sync`, replaces `ensure_base_zone` + `reconcile_txt`): ensure the
   system row for `general.domain`; delete every PowerDNS zone without an active
   database zone; for every active zone ensure it exists, make its rrsets equal to
   `zone_rrsets` (+ apex NS for `general.nsname` when none is stored, + challenge TXT for
   the base zone) with one PATCH of differences (SOA untouched), import missing DNSSEC
   keys from the stored content and remove unknown ones, set NSEC3PARAM, create missing
   TSIG keys with their stored secrets, set both metadata kinds, flush the packet cache
   of every touched zone. Safety rule for the first run after this change: a database
   zone with no stored rrsets whose PowerDNS zone has records adopts them into certo.db
   instead of deleting them (logged at warn level) — this is what carries the owner's
   current base-zone records over. Returns a report (zones created/removed, rrsets
   changed, keys imported, records adopted).
4. **Data version between certo and PowerDNS**: `zones.version` (integer, starts at 1) is
   incremented in the same transaction as every change that affects what PowerDNS serves
   for that zone — rrsets, DNSSEC, TSIG keys, allow list, activation, and for the base
   zone every challenge TXT write. Each PowerDNS zone carries the version it was built
   from in the metadata kind `X-CERTO-VERSION`, written after every successful push and
   after a sync. `acmedns.dns_version` is the sum of all zone versions for a one-number
   overview. Sync compares per zone: equal versions skip the zone, a missing or different
   version converges it, a PowerDNS zone without metadata is the "first run" case for the
   adoption rule in item 3. `GET /api/info` (admin view) and `POST /admin/sync` report
   `{ db_version, pdns_version, zones: [{ name, db, pdns }] }` for the mismatched zones;
   the Server tab shows both numbers and marks drift.
5. **Triggers**: at startup (abort on failure as today); `POST /admin/sync` returning the
   report; a "Sync DNS" button with the report as a toast in the dashboard Server tab
   (en + zh-CN keys). Activation/deactivation use the same per-zone sync, so the
   snapshot logic disappears.
6. **Tests**: certo-db tests for the new tables and cascades; `certo-pdns` mock tests for
   the sync (orphan removed, rrsets converged, key imported with the same DS, adoption);
   the mock gains `GET cryptokeys/{id}` with content and `POST cryptokeys` with
   content; e2e restart test extended: wipe `pdns.sqlite3`, restart, a hosted zone
   comes back with its records, DNSSEC (same DS), TSIG key and allow list; e2e for
   `POST /admin/sync`.
7. **Docs**: `architecture.md` (database section, startup sequence, data flow),
   `deployment.md` (back up certo.db only; `pdns.sqlite3` is disposable; the manual
   migration section stays), `api.md` + `llms.txt` + `llms-api.txt` for `/admin/sync`,
   `configuration.md` (`general.records` seeds once), a decision record superseding the
   "keys live in PowerDNS' database" consequence of 2026-10-18, changelog.
8. **Release**: after the owner's database is migrated and verified against the new
   binary; the tag name is the owner's call (suggested `v1.0.0` for the new generation).

Out of scope: moving PowerDNS off SQLite, zone transfers *into* certo (secondary mode),
history/versioning of records.

## Steps

1. certo-db tables, cascades and accessors with tests → nextest green.
2. `Zones` write path through certo.db; reads from certo.db; client `cryptokey` fetch and
   create-with-content; mock support → certo-pdns tests green.
3. Zone versions and `X-CERTO-VERSION`, `Zones::sync` with adoption rule, startup
   wiring, `POST /admin/sync`, info fields, web button and drift display →
   clippy, nextest, e2e (incl. extended restart test), web gates green.
4. Docs, decision record, changelog → typos green; commit and push.
5. Migrate the owner's database by hand (schema 1, system rows removed), run the new
   binary against a copy, check `*.s.dnsall.com` TXT answers and the sync report; tag.

## Risks

- First sync on an existing server is destructive for PowerDNS zones the database does
  not know; the adoption rule covers zones the database knows but has no rrsets for.
  Backup of both files before the first start remains the documented step.
- DNSSEC private key content now lives in certo.db too: same file permissions (0600),
  same backup handling.
- Sync fetches the full rrset list only for zones whose version differs; every challenge
  write costs one extra metadata `PUT` on the base zone (local HTTP call).

## Alternatives

- Keep PowerDNS as the record store and add only orphan-zone cleanup: fixes the stale
  zone but leaves two files to back up and no way to rebuild hosted zones. Rejected by
  the owner.
- Store records in certo.db but read them from PowerDNS: avoids the SOA-row change in
  the dashboard, at the cost of two sources for the same screen. Not worth it.

## Revision (2026-11-02 11:57)

Implemented as proposed with three deviations: the versions are exposed by
`GET /admin/sync` (admin key) instead of an admin view of the public `GET /api/info`;
`dns_version` is computed as the sum of the active zones' versions instead of being stored
in `acmedns`; adoption covers rrsets, the active DNSSEC key and the allow list but not TSIG
keys (documented in deployment.md). Startup sync skips zones whose stamp matches, the
admin endpoint and dashboard button force every zone. Step 5 (owner's database) is still
open: the manual migration gained two `ALTER TABLE` lines.
