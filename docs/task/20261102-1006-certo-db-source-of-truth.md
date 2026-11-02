# 20261102-1006-certo-db-source-of-truth Keep every DNS fact in certo.db and derive PowerDNS from it

- **status**: completed
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-11-02 10:06

## Description

Hosted-zone records, DNSSEC keys, TSIG secrets and AXFR allow lists exist only in
PowerDNS' SQLite file; certo.db holds users, keys, challenge TXT and zone ownership. Two
files must be backed up and kept consistent, configuration changes (e.g. `general.domain`)
leave stale zones behind, and a lost `pdns.sqlite3` loses every hosted zone. The owner
wants certo.db to be the single source of truth with PowerDNS rebuilt from it by a sync
step (automatic at startup, on demand from the admin area).

Acceptance: every zone write lands in certo.db; `Zones::sync` makes PowerDNS equal to the
database (zones, rrsets, DNSSEC keys with unchanged DS, TSIG keys, allow lists) and
removes zones the database does not know; it runs at startup and via `POST /admin/sync`
(button in the dashboard Server tab); deleting `pdns.sqlite3` and restarting restores a
hosted zone with records, DNSSEC (same DS) and TSIG; only certo.db needs a backup; Rust
gates, e2e, web gates green; docs updated.

## ActiveForm

Making certo.db the source of truth for DNS data.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Requested by the owner 2026-11-02: "为什么不用真数据源在certo.db里面，然后有一个同步的选项" → "可以".
Plan: 20261102-1006-certo-db-source-of-truth.

- complete: Implemented; owner DB migration pending (plan step 5)
