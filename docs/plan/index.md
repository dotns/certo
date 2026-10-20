# certo - Plan Index

> Updated: 2026-10-18

## Usage

Each plan is a single line linking to its detail file. All detailed information lives in `docs/plan/<timestamp>-<feature-slug>.md`.

### Status Markers

| Marker | Meaning |
|--------|---------|
| `[ ]`  | Draft / Pending review |
| `[-]`  | Approved / Implementing |
| `[x]`  | Completed |
| `[~]`  | Rejected / Abandoned |
| `[d]`  | Deleted detail file; index entry retained |

### Rules

- Only update the checkbox marker; never delete the line or change its other content.
- New plans append to the end.

---

## Plans

- [x] [**20261016-1840-authoritative-zones-ptr Authoritative zones with PTR and general records**](20261016-1840-authoritative-zones-ptr.md) `2026-10-16`
- [x] [**20261017-0350-audit-remediation Remediate repository audit findings**](20261017-0350-audit-remediation.md) `2026-10-17`
- [x] [**20261017-0403-lode-self-update Updates through the lode supervisor**](20261017-0403-lode-self-update.md) `2026-10-17`
- [~] [**20261018-0429-system-zone Manage the base domain as a system zone instead of config records**](20261018-0429-system-zone.md) `2026-10-18`
- [x] [**20261018-0445-pdns-control-plane Run PowerDNS as the data plane, certo as its control plane**](20261018-0445-pdns-control-plane.md) `2026-10-18`
