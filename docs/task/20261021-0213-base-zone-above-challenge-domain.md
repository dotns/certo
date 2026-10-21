# 20261021-0213-base-zone-above-challenge-domain Serve a base zone above the challenge domain

- **status**: completed
- **completedAt**: 2026-10-21 02:23
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-10-21 02:13

## Description

The owner's deployment hosts `dnsall.com` as the zone certo is authoritative for while
challenge records live at `<subdomain>.s.dnsall.com`. Today `general.domain` is both the
challenge base and the name of the base zone, so the base zone is `s.dnsall.com` and
`dnsall.com` cannot be created as a hosted zone (the base zone row with `user_id = NULL`
trips the overlap check: `403 zone_not_allowed`).

Make the base zone `general.domain` and add `general.acme_domain` (default `domain`) for
the challenge domain, so that the challenge domain is a name inside the base zone,
challenge TXT rrsets are written into that zone, and user zones may not be at or under
the base zone.

Acceptance: with `domain = "dnsall.com"` and `acme_domain = "s.dnsall.com"`, certo creates
and serves `dnsall.com` as the system zone (seeded from `general.records`), answers
challenge TXT at `<sub>.s.dnsall.com`, solves its own DNS-01 at `_acme-challenge.dnsall.com`,
refuses user zones at or under `dnsall.com`, and the admin area edits `dnsall.com`
records. Rust gates and e2e green; docs updated.

## ActiveForm

Served a base zone above the challenge domain.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Reported by the owner 2026-10-21 02:13: "base zone 应该是 dnsall.com 才对". Plan: 20261021-0213-base-zone-above-challenge-domain.
