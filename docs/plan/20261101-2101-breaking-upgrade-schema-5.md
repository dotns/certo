# 20261101-2101-breaking-upgrade-schema-5 Breaking upgrade: schema 5 with a single upgrade path and a config-derived base zone

- **status**: completed
- **approvedAt**: 2026-11-02 00:05
- **completedAt**: 2026-11-02 00:40
- **createdAt**: 2026-11-01 21:01
- **relatedTask**: 20261101-2055-breaking-upgrade-schema-5

## Context

`certo-db` is at schema version 4 and still carries the whole upgrade chain: `1 -> 2`
(Go era), `2 -> 3` (hickory zone tables), `3 -> 4` plus the one-time import of version 3
zones into PowerDNS (`Zones::import_legacy`, `Db::legacy_zones`, `Db::complete_import`,
the `go-v2.db` fixture test). The base zone is a `zones` row with `user_id IS NULL`
created by `ensure_base_zone`; nothing removes such a row when `general.domain` changes,
and `Zones::delete` refuses system rows. On the owner's server the v0.1.1 move from
`domain = s.dnsall.com` to `domain = dnsall.com` + `acme_domain = s.dnsall.com` left the
old `s.dnsall.com` zone in both stores. PowerDNS picks the most specific zone, so
`65bc0c2f.s.dnsall.com` answered NXDOMAIN from the stale zone while certo wrote the TXT
into `dnsall.com` (verified against ns1.dnsall.com, SOA serial 2026102102 for
`s.dnsall.com`). The owner wants a breaking upgrade rather than compatibility code.

## Revision (2026-11-02)

The owner replaced items 1, 2 and 4 of the proposal: the schema is reset to version **1**
of a new generation with no upgrade chain (any other version, and a Go-era version 1, is
refused); earlier databases are migrated by hand with the SQL documented in
`docs/deployment.md`; the startup reconciliation of PowerDNS zones is not implemented;
all GitHub releases and tags are deleted and `main` is force-pushed instead of tagging
v0.4.0. Item 3 (docs) stands with the new wording. The text below is the original
proposal, kept for the record.

## Proposal

1. **Schema 5, one upgrade path** (`crates/certo-db`): `DB_VERSION = 5`. A fresh database
   is created at 5. Version 4 upgrades in one transaction: `DELETE FROM zones WHERE
   user_id IS NULL` (the base zone row is re-created from the config at every start) and
   `db_version = 5`. Versions 1-3 are refused at open with
   `DbError::ObsoleteVersion { found, oldest_supported: 4 }` ("upgrade with certo v0.3.x
   first"); versions above 5 keep `UnsupportedVersion`. Removed: `ZONE_TABLES_V3`,
   `upgrade_1_to_2`, `upgrade_2_to_3`, `upgrade_3_to_4`, `has_legacy_zones`,
   `LegacyZone`, `legacy_zones`, `complete_import`, the v1 and `go-v2.db` fixture tests
   and the v3 import tests; new tests: 4 -> 5 drops system rows and keeps user zones,
   3 is refused, 6 is refused.
2. **PowerDNS is derived state** (`crates/certo-pdns`): `import_legacy` is removed (and
   its call in `crates/certo/src/main.rs`). `ensure_base_zone` becomes
   `reconcile_base_zone`: (a) ensure the system row and the PowerDNS zone for
   `general.domain` as today; (b) delete every PowerDNS zone that has no `zones` row
   (warn log per zone) — this drops the stale `s.dnsall.com` zone, zones of users deleted
   while PowerDNS was unreachable, and zones created by hand; (c) when
   `acme_domain != domain`, delete an NS rrset at `acme_domain` inside the base zone (warn
   log): a delegation there turns every challenge name into a referral, which is the
   owner's current workaround. Tests on the mock client: orphan zone removed, user zone
   kept, NS at the challenge domain removed, NS at other names kept.
3. **Docs**: README upgrade note, `docs/architecture.md` Migrations, `docs/deployment.md`
   Upgrades (back up `certo.db` and `pdns.sqlite3`; roll forward only; Go and schema-3
   databases must go through v0.3.x), `docs/configuration.md` `domain` row (changing it
   discards the old base zone), decision record
   `docs/decisions/20261101-schema-5-single-upgrade-path.md`, changelog.
4. **Release**: `v0.4.0`, together with the pending session-redirect fix
   (task 20261022-1946). The owner's database is then checked against the release binary
   on a copy before the server is switched; nothing in it needs hand editing (the
   upgrade removes the system row, startup removes the orphan zone and the NS record).

Out of scope: moving hosted-zone records back into certo's database, changing the
`txt` / `user_domains` tables, removing the system row concept from the API.

## Steps

1. certo-db schema 5 + refusal of 1-3, tests (RED -> GREEN) -> `cargo nextest` green.
2. certo-pdns reconcile (orphans, NS at acme_domain), remove import, tests -> nextest,
   clippy, e2e green (restart test still rebuilds base zone + TXT).
3. Docs, decision record, changelog -> `typos`, links checked.
4. Full gate set, commit, push, tag v0.4.0, watch CI/release; verify the owner's database
   copy with the release binary.

## Risks

- Destructive by design: PowerDNS zones without a row are deleted at startup; operators
  must back up `data/pdns/pdns.sqlite3` and `data/db/certo.db` before upgrading.
- A Go-era or schema-3 database no longer upgrades directly; the error names the release
  to go through first.
- `lode` auto-update installs v0.4.0 unattended; the changelog and release notes carry
  the backup warning.

## Alternatives

- Keep the chain and add a "retire old system zone" step: rejected by the owner (no
  compatibility code).
- Fix the owner's database by hand only: does not stop the next `domain` change from
  recreating the problem.
