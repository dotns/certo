# 20261017-0403-lode-self-update Updates through the lode supervisor

- **status**: completed
- **priority**: P2
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-10-17 04:03

## Description

Make certo updatable through the lode supervisor instead of in-process
self-update: publish signed release assets (`certo-linux-{amd64,arm64}.tar.gz`
with `.sig` sidecars) on GitHub Releases from CI, ship a `lode.toml` and a
compose file that run certo under the generic `bkhq/lode` image, and document
how operators enable (`policy = "auto"`), observe (`check`) or disable
(`off`) updates. No certo image is published any more; no Rust change.

Acceptance: a tag build produces the two signed archives and a GitHub
release; a lode container started from the shipped files runs certo with its
data under `/srv/lode/data`, answers DNS and `/api/info`, and installs a newer
signed release automatically while refusing an unsigned one; the e2e suite is
unchanged and green.

## ActiveForm

Publishing signed release assets and the lode operator files.

## Dependencies

- **blocked by**: repository secret `LODE_SIGNING_KEY` (owner step)
- **blocks**: (none)

## Notes

Plan: 20261017-0403-lode-self-update. Decision 2026-10-17: external supervisor (lode image), not embedded self-update.

- complete: Release workflow, lode.toml, compose and docs done; verified under bkhq/lode:v0.1.0 locally (install, auto-update, bad digest refused).
