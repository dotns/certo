# 20261017-0403-lode-self-update Updates through the lode supervisor

- **status**: completed
- **createdAt**: 2026-10-17 04:03
- **approvedAt**: 2026-10-17 04:25
- **relatedTask**: 20261017-0403-lode-self-update

## Context

certo ships as one static musl binary. Today `.github/workflows/docker.yml`
builds and pushes the `dotns/certo` image on `v*` tags and attaches a tarball
of `config.toml` + `docker-compose.yml` to a GitHub release. Updating means
pulling a new image; there is no update code and no release binary asset.

The owner decided (2026-10-17) against embedding `lode-core` in certo: certo
is to run under the generic lode image (`docker.io/bkhq/lode`, 0.1.0,
amd64 + arm64, runs as root, system CA certificates present), which fetches,
verifies, launches and hot-updates the app, so certo need not publish its own
image. Operators enable or disable updates in `lode.toml`
(`[update].policy = off | check | auto`), or keep running the bare binary.

What lode needs from the publisher (lode `docs/integration.md` §3,
`docs/source-adapters.md` §5): a release feed. GitHub Releases is a native
source: assets named `<app>-<os>-<arch>.<ext>`, each with a `<asset>.sig`
sidecar produced by `lode-cli sign --app <app> --version <tag minus v>`;
`channel = stable` resolves `/releases/latest`, other channels the newest
pre-release, `pin` a tag. Unauthenticated API calls are limited to 60/h per
IP. The alternative, the `lode.bk.io` registry (`lode-cli publish`), needs a
package created from a signed-in session, a token and a resign cron.

What lode needs from the app (§2): exit 0 on SIGTERM within `stop_timeout`
(certo drains for 10 s), non-zero exit on startup failure (certo does). Lode
runs the app with cwd = the version directory, which is replaced on every
update and pruned, so certo's relative paths (`./data/config.toml`,
`data/db/certo.db`, `data/certs`) must resolve elsewhere: `[command].workdir`
sets the cwd; `./certo` as the run program is absolutized against the version
directory. `readiness = "none"` with `health_grace` commits a version after it
survives that long; a version that exits earlier rolls back to the last good
one. `state.json` readiness needs the SDK and is not required.

certo verifies TLS with the platform store (`rustls-platform-verifier`), so
the default lode image (ubase) and the `mini` variant work; `slim` ships no
CA certificates.

## Proposal

No change to certo's Rust code. The work is the release feed, the operator
files and the docs.

### 1. Release assets (`.github/workflows/release.yml`, replaces `docker.yml`)

- `Dockerfile`: add `FROM scratch AS binary` with `COPY --from=backend /certo
  /certo`; the default target stays the runnable image for local builds.
- On a `v*` tag: `docker buildx build --platform linux/amd64,linux/arm64
  --target binary --build-arg VERSION=<tag> --output type=local` and pack
  `dist/certo-linux-amd64.tar.gz` and `dist/certo-linux-arm64.tar.gz`, each
  holding `certo` at the top level.
- Sign each asset with `lode-cli sign --app certo --version ${TAG#v}
  --key-env LODE_SIGNING_KEY` into `<asset>.sig` when the secret is set
  (`lode-cli` fetched from the pinned lode 0.1.0 release with its published
  sha256); without the secret the job fails for a tag, because an unsigned
  release is useless to operators with `require_signature = "enforce"`.
- `gh release create <tag> --generate-notes` (or upload to an existing one)
  with the two archives, their `.sig` files and the existing
  `certo-<tag>.tar.gz` (sample config, compose file, now also `lode.toml`).
- The Docker Hub push and its secrets go away; the `Dockerfile` remains for
  building and for anyone who wants a plain image.

### 2. Publisher key

Generate a dedicated ed25519 key with `lode-cli keygen`, seed stored only
under `/srv/dotns/.work/lode-keys/certo-release.key` (0600, never printed).
Owner step: put the seed in the repository secret `LODE_SIGNING_KEY`. The
public entry (`<key_id>:<base64>`) goes into `lode.toml` and the docs.

### 3. Operator files (repository root, shipped in the release tarball)

`lode.toml`:

```toml
[global]
app = "certo"
dir = "/srv/lode"

[update]
github         = "dotns/certo"
asset          = "certo-linux-amd64.tar.gz"   # certo-linux-arm64.tar.gz on arm64
channel        = "stable"
policy         = "auto"                       # off = never update, check = log only, auto = install
check_interval = 3600

[trust]
require_signature = "enforce"
trusted_keys = ["<key_id>:<base64>"]

[command]
run     = "./certo"
exec    = "./certo"
workdir = "/srv/lode"                         # certo reads ./data/config.toml here

[supervise]
restart      = "on-failure"
readiness    = "none"
health_grace = 15
stop_timeout = 15
```

`docker-compose.yml` switches to `image: bkhq/lode:v0.1.0`, one bind mount
`./lode:/srv/lode` (holds `lode.toml` and `data/` with `config.toml`, the
database, certificates, logs), the same ports. Operators who do not want
updates set `policy = "off"` or keep running the binary directly.

### 4. Docs

- `docs/deployment.md`: "Docker Compose" becomes the lode-based setup;
  directory layout, `policy` as the on/off switch, `pin`, `lode-cli status
  / update / rollback` via `docker exec`, rollback semantics, GitHub API
  rate limit, how to move an existing `data/` directory.
- `README.md` / `README_CN.md`: quick start and the release assets.
- `docs/development.md`: release process (tag, signing secret, assets).
- `docs/architecture.md`: deployment note. `docs/changelog.md` entry.

### Verification

- Workflow: `actionlint` if available, otherwise a careful read; the build
  step reproduced locally with `docker buildx build --target binary --output`.
- End to end, locally: build the two archives from the working tree, sign
  them with a throwaway key, serve a native `lode/v1` manifest from a sibling
  HTTP container on the `traefik` network (`allow_insecure = true` for the
  test only), run `bkhq/lode:v0.1.0` as a sibling with the proposed
  `lode.toml` (source swapped to that manifest) and the sample `config.toml`;
  check DNS and `/api/info` answer, that data lands under `/srv/lode/data`,
  then publish a second version and watch lode install and relaunch it.
- e2e suite unchanged.

## Risks

- The `dotns/certo` Docker image stops being published; existing users of it
  switch to the lode compose file or build the image themselves.
  (Reversible: keep the push job if wanted.)
- Trust is only as good as the secret: a `LODE_SIGNING_KEY` leak lets anyone
  publish installable releases until operators change `trusted_keys`. Keep
  the seed offline besides the GitHub secret; lode's delegated signing can
  come later.
- A version that survives `health_grace` and fails later is not rolled back
  automatically; `lode-cli rollback` or `pin` recovers.
- GitHub API rate limit: `check_interval = 3600` is far below 60/h; a shared
  NAT with many certo hosts could hit it (checks fail softly).
- Relative paths in `config.toml` depend on `workdir`; the docs make that
  explicit.

## Scope

`.github/workflows/release.yml` (new, `docker.yml` removed), `Dockerfile`,
`lode.toml` (new), `docker-compose.yml`, `README.md`, `README_CN.md`,
`docs/deployment.md`, `docs/development.md`, `docs/architecture.md`,
`docs/changelog.md`. No Rust changes.

## Alternatives

- Embed `lode-core` `SelfUpdate` in certo: rejected by the owner (needs its
  own release feed anyway, raises MSRV to 1.96, no rollback).
- `lode.bk.io` registry as the source: signed manifest with expiry (freeze
  protection) but needs a package, a token and a resign cron; GitHub
  Releases is already where certo releases live. Can be added later as a
  second source without changing the assets.
- Keep publishing the `dotns/certo` image next to the assets: possible, but
  the owner wants to drop it.
- `readiness = "state"` with the lode Rust SDK: more precise health, but
  needs code and `signal-hook`; not needed for a server that binds within
  milliseconds.

## Annotations

Approved by the owner with "可以了" after the `LODE_SIGNING_KEY` secret was set (2026-10-17 04:24). The public key entry for `trusted_keys` is still to be supplied.

Owner decision during implementation (2026-10-17 04:30): no publisher signing for
now; the workflow publishes unsigned assets, `lode.toml` uses
`require_signature = "auto"`, and the `LODE_SIGNING_KEY` secret is unused.
`docs/architecture.md` needed no change. Verified as recorded in the changelog
entry of 2026-10-17 04:35.
