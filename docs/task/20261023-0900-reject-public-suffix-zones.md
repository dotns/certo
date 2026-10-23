# 20261023-0900-reject-public-suffix-zones Reject zones that are public suffixes

- **status**: completed
- **priority**: P1
- **owner**: roy/session-1a6b
- **createdAt**: 2026-10-23 09:00

## Description

`parse_zone_name` only requires two labels, so a user can create `in-addr.arpa`, `ip6.arpa`, `co.uk` or `github.io`. Such a zone stays pending (it can never be delegated) but still counts in the overlap check, so every other user's reverse zone (or `*.co.uk` zone) is refused with `zone_not_allowed` until the pending zone expires after 30 days, and it can be re-created right away. Reject zone creation when the name is itself a public suffix (Public Suffix List, ICANN and private sections).

Acceptance: creating `in-addr.arpa`, `ip6.arpa`, `co.uk` or `github.io` fails with `403 zone_not_allowed`; `23.251.162.in-addr.arpa`, `example.co.uk` and `example.test` are still accepted; existing zones stay readable; Rust gates and e2e green.

## ActiveForm

Rejecting public-suffix zone names

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

From the 2026-10-17 code review (finding 1). The review's other findings no longer apply: record writes are rrset patches to PowerDNS (no full rebuild/re-sign under a global lock), and the overlap check runs inside the create transaction (20261017-0350-audit-remediation).

- complete: Gates green: 77 nextest, deny, shear, typos, MSRV; e2e 45/45.
