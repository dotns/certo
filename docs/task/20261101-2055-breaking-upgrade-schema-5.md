# 20261101-2055-breaking-upgrade-schema-5 Breaking upgrade: schema 5 with a single upgrade path and a config-derived base zone

- **status**: completed
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-11-01 20:55

## Description

After `general.domain` moved from the challenge domain to its parent (v0.1.1, e.g.
`s.dnsall.com` -> `dnsall.com` with `acme_domain = s.dnsall.com`), the previous system
zone row (`user_id IS NULL`) and its PowerDNS zone survived. PowerDNS answers
`*.s.dnsall.com` from the more specific stale zone, so challenge TXT records written into
`dnsall.com` are invisible (NXDOMAIN with the `s.dnsall.com` SOA) while the dashboard lists
them from certo's database. The stale zone shows as "Base zone" in the admin area and
cannot be deleted there. The owner worked around it by adding an NS record for
`s.dnsall.com`.

The owner asked for a breaking reset instead of compatibility code: the current layout
becomes schema version 1 of a new generation, no upgrade chain is kept, earlier databases
are migrated by hand, all GitHub releases and tags are deleted and `main` is force-pushed.

Acceptance: a new database is created at version 1; any other version (and a Go-era
version 1 layout) is refused with a message pointing at the manual migration; the legacy
import is gone; the manual migration for a schema-4 database and for a stale base zone is
documented; Rust gates, e2e and web gates green; docs and changelog updated; releases and
tags removed; `main` force-pushed.

## ActiveForm

Implementing the schema 5 breaking upgrade.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Reported by the owner 2026-11-01: TXT visible in the dashboard but not resolving for
`65bc0c2f.s.dnsall.com`. Verified against ns1.dnsall.com (34.179.234.162): a separate
`s.dnsall.com` zone (SOA serial 2026102102) exists next to `dnsall.com`. Owner's
instructions: "修改为最新版本不要兼容旧版本", "做一次破坏性升级". Plan:
20261101-2101-breaking-upgrade-schema-5.

- complete: Schema reset to 1, legacy import removed, manual migration documented
