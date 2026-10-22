# certo - Task List

> Updated: 2026-10-18

## Usage

Each task is a single line linking to its detail file. All detailed information lives in `docs/task/<timestamp>-<feature-slug>.md`.

### Status Markers

| Marker | Meaning |
|--------|---------|
| `[ ]`  | Pending |
| `[-]`  | In progress |
| `[x]`  | Completed |
| `[~]`  | Closed / Won't do |
| `[d]`  | Deleted detail file; index entry retained |

### Priority: P0 (blocking) > P1 (high) > P2 (medium) > P3 (low)

### Rules

- Only update the checkbox marker; never delete the line or change its other content.
- New tasks append to the end.

---

## Tasks

- [x] [**20261016-1840-authoritative-zones-ptr Authoritative zones with PTR and general records**](20261016-1840-authoritative-zones-ptr.md) `P1`
- [x] [**20261017-0350-audit-remediation Remediate repository audit findings**](20261017-0350-audit-remediation.md) `P1`
- [x] [**20261017-0403-lode-self-update Updates through the lode supervisor**](20261017-0403-lode-self-update.md) `P2`
- [x] [**20261017-0515-ci-spa-build Build the dashboard in the CI e2e job and refresh workflow actions**](20261017-0515-ci-spa-build.md) `P1`
- [~] [**20261018-0429-system-zone Manage the base domain as a system zone instead of config records**](20261018-0429-system-zone.md) `P1`
- [x] [**20261018-0445-pdns-control-plane Run PowerDNS as the data plane, certo as its control plane**](20261018-0445-pdns-control-plane.md) `P1`
- [x] [**20261021-0213-base-zone-above-challenge-domain Serve a base zone above the challenge domain**](20261021-0213-base-zone-above-challenge-domain.md) `P1`
- [x] [**20261021-0408-dashboard-feature-completion Complete the dashboard feature set**](20261021-0408-dashboard-feature-completion.md) `P1`
- [x] [**20261022-0431-dashboard-i18n Localize the dashboard (English and Simplified Chinese)**](20261022-0431-dashboard-i18n.md) `P1`
