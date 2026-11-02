# Schema reset to version 1 without an upgrade chain

- **date**: 2026-11-02
- **status**: accepted

## Context

`certo-db` carried four schema versions and their upgrade steps (Go era `1 → 2`, hickory
zone tables `2 → 3`, PowerDNS `3 → 4` with a one-time zone import). The chain did not cover
configuration changes such as moving `general.domain`, which left a stale base zone in both
stores and made challenge records unreachable on the owner's server. The owner asked for a
breaking reset instead of more migration code.

## Decision

The current layout is schema version **1** of a new generation. `Db::open` creates it for
new databases and refuses every other version, including a Go-era version 1 (detected by
the missing `zones.exempt` column). No upgrade code is kept; migrating an earlier database
is a documented manual procedure (`docs/deployment.md`). All previous GitHub releases and
tags were removed, so nothing installs the old generation anymore.

## Consequences

- Operators of the previous Rust releases run two SQL statements before starting the new
  binary and remove a stale base zone from PowerDNS if `general.domain` ever changed.
- Go-era and schema-3 databases start fresh.
- `certo-db` and `certo-pdns` lose the legacy import, its fixture and tests; the version
  check is a few lines.
