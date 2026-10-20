# 20261018-0445-pdns-control-plane Run PowerDNS as the data plane, certo as its control plane

- **status**: completed
- **completedAt**: 2026-10-18 06:40
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-10-18 04:45

## Description

Replace certo's own DNS implementation (`crates/certo-dns`, hickory-server) with PowerDNS
Authoritative: certo starts and supervises a bundled static `pdns_server`, generates its
configuration and SQLite schema, and drives it through the PowerDNS HTTP API. certo keeps
everything PowerDNS does not have: users, API keys and scopes, acme-dns/httpreq challenge
protocols, per-user zone ownership, the dashboard and its own TLS. The base domain becomes
an ordinary PowerDNS zone created at first start (seeded from `general.records`), so the
former "system zone" task is covered. One release asset ships both binaries and lode runs
them as one process tree.

Acceptance: `certo` started from a release directory brings up `pdns_server` on
`general.listen`, answers the base domain, challenge TXT (httpreq, acme-dns, own ACME
certificate) and hosted zones (records, DNSSEC with DS, AXFR by allow list / TSIG) through
PowerDNS; a user-created zone is served only after its NS delegation is verified or an
admin exempts it; existing schema-3 databases are imported into PowerDNS once; SIGTERM stops both
processes; the e2e suite is green on CI with a static `pdns_server` for amd64 and arm64
built in the release workflow; all Rust gates green; no `hickory-*` dependency remains.

## ActiveForm

Replaced the hickory DNS server with a supervised PowerDNS process.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Plan: 20261018-0445-pdns-control-plane. Supersedes 20261018-0429-system-zone.
Decision 2026-10-18 (owner): PowerDNS does authoritative DNS; certo only implements the
management layer and bundles a static `pdns_server` in the same package. Production is
rebuilt once from the old database (DS change accepted). Zone creation is open; a zone is
served only once its delegation points at `general.nsname`, admins can exempt a zone.
Done 2026-10-18 06:40: all plan steps complete; amd64 asset only (arm64 suspended by the
owner), release `v0.1.0` is tagged by the owner after commit.
