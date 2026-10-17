# 20261016-1840-authoritative-zones-ptr Authoritative zones with PTR and general records

- **status**: completed
- **priority**: P1
- **owner**: roy
- **createdAt**: 2026-10-16 18:40

## Description

Rewrite certo in Rust (hickory-server + axum) and extend it into a general authoritative DNS server: user-managed zones (forward and reverse, e.g. `23.251.162.in-addr.arpa`) with A/AAAA/CNAME/TXT/MX/NS/PTR/SRV/CAA records, while keeping the httpreq/acme-dns challenge flow and existing data unchanged.

Acceptance: `dig -x` against a configured reverse zone returns PTR answers with correct SOA/NS; existing tests pass untouched; existing challenge TXT/CNAME delegation behaves identically.

## ActiveForm

Adding authoritative zone and PTR support

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Plan: 20261016-1840-authoritative-zones-ptr (docs/plan).

- complete: Rust rewrite with hosted zones, DNSSEC, AXFR/TSIG and dashboard done; Rust gates and e2e 38/38 green; Go code removed. OpenAPI (utoipa) not added.
