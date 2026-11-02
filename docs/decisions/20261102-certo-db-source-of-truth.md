# Keep every DNS fact in certo's database and derive PowerDNS from it

- **date**: 2026-11-02
- **status**: accepted
- **relatedPlan**: 20261102-1006-certo-db-source-of-truth
- **supersedes**: the "DNSSEC keys and TSIG secrets live in PowerDNS' database" consequence
  of `20261018-powerdns-data-plane.md`

## Context

Since the PowerDNS data plane, hosted-zone rrsets, DNSSEC keys, TSIG secrets and transfer
metadata existed only in `pdns.sqlite3`; certo's database held users, keys, challenge TXT
and zone ownership (plus a JSON snapshot of a deactivated zone). Two files had to be backed
up and kept consistent, a configuration change (`general.domain`) left a stale zone in
PowerDNS that shadowed the challenge names, and a lost `pdns.sqlite3` lost every hosted
zone. The owner asked for certo's database to be the single source of truth with a sync
option, and for a version number that makes the difference between the two sides visible.

## Decision

- `certo.db` stores everything PowerDNS serves: `zone_rrsets`, `zone_dnssec` (private key
  in ISC format, so a re-import keeps the DNSKEY and DS), `zone_tsig_keys` (with secrets),
  `zones.axfr_allow_from`, next to `txt` and `zones`. The snapshot column is gone.
- Writes go to PowerDNS first (validation), then to the database; reads come from the
  database except the DNSKEY/DS strings.
- Every zone carries a data `version` incremented in the same transaction as any change to
  what PowerDNS serves; PowerDNS zones are stamped with it (`X-CERTO-VERSION` metadata).
  `GET /admin/sync` reports both sums and the zones that differ.
- `Zones::sync` derives PowerDNS from the database: orphan zones removed, active zones
  created or converged, keys imported, TSIG keys recreated, metadata set. It runs at
  startup (skipping zones whose stamp matches) and on demand (`POST /admin/sync`, dashboard
  button; every zone). A PowerDNS zone without a stamp is adopted into the database once.
- Only `certo.db` needs a backup; `[pdns] dir` is disposable.

## Alternatives considered

- Keep PowerDNS as the record store and only remove orphan zones at startup: fixes the
  shadowing but leaves two files to back up and no way to rebuild hosted zones.
- Store records in the database but keep reading them from PowerDNS: two sources for one
  screen, no benefit.

## Consequences

- DNSSEC private keys and TSIG secrets are in `certo.db` as well; file permissions and
  backup handling of that file matter even more.
- The rrset list no longer contains the SOA row (PowerDNS maintains it).
- Every challenge TXT write costs one extra metadata `PUT` on the base zone.
- The first start on an existing server deletes PowerDNS zones the database does not know
  and adopts the rest (records, key, allow list; not TSIG keys).
