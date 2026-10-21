# 20261021-0213-base-zone-above-challenge-domain Serve a base zone above the challenge domain

- **status**: completed
- **completedAt**: 2026-10-21 02:23
- **approvedAt**: 2026-10-21 02:20
- **createdAt**: 2026-10-21 02:13
- **relatedTask**: 20261021-0213-base-zone-above-challenge-domain

## Context

`ZonePolicy { base_domain, nsname }` (`crates/certo-pdns/src/zones.rs`) uses
`general.domain` for three different things: the challenge base (`<sub>.<domain>` names,
acme-dns `fulldomain`, `_acme-challenge.<domain>` for certo's own certificate), the name
of the PowerDNS base zone (`ensure_base_zone`, `sync_txt`, `reconcile_txt`,
`set_own_challenge`) and the boundary user zones may not cross (`create`). The owner's
layout — zone `dnsall.com`, challenges under `s.dnsall.com` — needs the first and the
other two to differ. `check_overlap` (`crates/certo-db/src/zones.rs`) additionally treats
the base zone row (`user_id = NULL`) as another owner's zone, so an ancestor of the base
zone cannot be created either.

## Proposal

1. `certo-core` config: `general.domain` becomes the base zone and the new
   `general.acme_domain` (default empty = `domain`) the challenge domain. `Config::load`
   validates that `acme_domain` equals `domain` or is inside it, otherwise startup aborts
   with a clear message. (Revised from a `general.zone` key at the owner's request on
   2026-10-21: existing `domain = "s.dnsall.com"` deployments keep their layout by
   leaving `acme_domain` unset.)
2. `certo-pdns`: `ZonePolicy` gains `base_zone`. `ensure_base_zone` creates/looks up
   `base_zone` (seed records must be inside it); `sync_txt`, `reconcile_txt` and
   `set_own_challenge` patch `base_zone`; challenge names stay under `base_domain`
   and the own certificate (issued for `general.domain`) uses `_acme-challenge.<base_zone>`;
   `create` refuses names at or under `base_zone`; the base zone row in `zones` is named
   `base_zone`. `check_overlap` ignores the `user_id = NULL` row (descendants of the
   base zone are already refused in `create`; an ancestor can only be activated by whoever
   controls its delegation).
3. `certo` main wires `base_zone = general.domain`, `base_domain = general.acme_domain`;
   `certo-api` uses `acme_domain` for challenge names and `GET /api/info` additionally
   reports `base_zone`.
4. Tests (RED first): config default/validation; `certo-pdns` zone service with
   `base_zone = example.com`, `base_domain = s.example.com` (base zone created, TXT rrset
   lands in `example.com` at `<sub>.s.example.com`, own challenge name, `example.com` and
   `s.example.com` refused for users, `check_overlap` no longer blocks an ancestor);
   `certo-db` overlap test for the NULL owner; e2e helpers take `zone` (written as
   `domain`) and run challenges at `acme_domain = s.<zone>` so the real pdns path is
   covered (expectations that name the SOA owner move to the zone).
5. Docs: `configuration.md` (`general.acme_domain`), `architecture.md` base zone section,
   README config tables, sample `config.toml`, changelog; release `v0.1.1`.

## Steps

1. Config key + validation + tests → `cargo nextest run -p certo-core` green.
2. `ZonePolicy.base_zone`, service changes, overlap fix, tests → `just rs-ci` green.
3. e2e helper `zone` option and suite adaptation → `just e2e` green (sibling image).
4. Docs, changelog, tag `v0.1.1`.

## Risks

- Existing deployments keep working unchanged (`acme_domain` defaults to `domain`).
- A deployment that changes `domain` from the old challenge name to its parent
  (`s.dnsall.com` → `dnsall.com`) gets a new base zone created next to the old one in
  PowerDNS; the old one stays until removed through the admin zone API.
- The own certificate is now issued for `general.domain` (the base zone) instead of the
  challenge name; with `acme_domain` unset nothing changes.

## Alternatives

- Keep one key and ask the owner to run challenges at `<sub>.dnsall.com`: changes every
  user's CNAME target. Rejected.
- Create `dnsall.com` as a normal user zone and add the `s.dnsall.com` delegation inside
  it: needs the overlap fix only, but leaves two zones to maintain and an in-bailiwick
  delegation with glue that the owner must keep correct. Workable, more fragile.
