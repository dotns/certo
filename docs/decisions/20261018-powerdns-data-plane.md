# Serve DNS with a bundled PowerDNS Authoritative instead of hickory-server

- **date**: 2026-10-18
- **status**: accepted
- **relatedPlan**: 20261018-0445-pdns-control-plane

## Context

Until 2026-10-18 `crates/certo-dns` answered DNS itself on `hickory-server`: a challenge
responder for the base domain and a catalog of hosted zones rebuilt and signed from the
database on every change, with a home-grown AXFR/TSIG wrapper because hickory has none.
Every DNS feature (NSEC3, NOTIFY, secondaries, more record types, caches) was ours to
build and maintain, and the base domain was a special case served from config records.

A spike showed that PowerDNS Authoritative 5.0.7 builds as a fully static musl binary
(gsqlite3 built in, about 12 MB) and that its HTTP API covers everything certo needs:
zones, rrsets, cryptokeys with DS, TSIG keys and zone metadata.

## Decision

certo becomes a control plane. It starts and supervises a bundled `pdns_server`,
generates its configuration and SQLite schema, and drives it through the PowerDNS HTTP
API; PowerDNS owns every DNS answer. certo keeps identity, API keys and scopes, the
challenge protocols, zone ownership, the dashboard and its own TLS.

- The base domain is an ordinary PowerDNS zone, created at first start and seeded from
  `general.records`; challenge TXT values are pushed into it as TTL 1 rrsets after every
  write and reconciled from certo's database at startup.
- The release asset ships `certo` and `pdns_server` together; lode runs them as one
  process tree. arm64 assets are suspended until the static build is verified on a native
  arm runner.
- Zone creation is open to every user; a zone is served only after certo verifies that the
  parent delegates it to `general.nsname`, or an admin exempts it. This replaces the
  `api.allow_zone_creation` switch.
- The record API takes the PowerDNS rrset shape (`PATCH /rrsets` with `REPLACE`/`DELETE`
  changes); the per-record id routes are gone.
- Schema 3 databases are imported into PowerDNS once at startup and the old record tables
  are dropped; DNSSEC zones get fresh keys (new DS), TSIG secrets are preserved.

## Alternatives considered

- Keep hickory-server and add the base domain as a system zone: no new runtime dependency,
  but every DNS feature stays home-grown. Rejected by the owner.
- Run the official PowerDNS image as a second container with certo as a pure API client:
  no static build, but two images and two lifecycles that lode cannot update as one unit.
- Write to the PowerDNS SQLite file directly: fewer moving parts, but bypasses PowerDNS
  caches and locking.

## Consequences

- PowerDNS security fixes require a certo release; the version is pinned in one place in
  the `Dockerfile`.
- Delegation checks need an outbound recursive resolver; without one zones stay pending
  until an admin exempts them (resolver errors never deactivate a served zone).
- certo validates records only superficially; PowerDNS messages are surfaced as
  `400 pdns_rejected`.
- Port 53 is bound by `pdns_server`, which certo starts as the same user.
- DNSSEC keys and TSIG secrets live in PowerDNS' database under `[pdns] dir`, which must be
  backed up together with certo's database.
