# Development

## Requirements

- Rust (the toolchain in `rust-toolchain.toml`, installed automatically by `rustup`; MSRV 1.88)
- `cargo-nextest`, `cargo-deny`, `cargo-shear`, `cargo-hack`, `typos` for the quality gates
- Node.js 22+ / npm (front-end)
- [`just`](https://github.com/casey/just) (optional, wraps the common tasks)
- [Bun](https://bun.sh), `dig` (bind-tools) and `ldns-verify-zone` (ldnsutils) for the e2e suite

## Layout

```
crates/certo-core/      config, domain types, id generation, scope matching
crates/certo-db/        SQLite storage (rusqlite), schema and upgrades
crates/certo-dns/       challenge responder, hosted zones, DNSSEC, AXFR/TSIG
crates/certo-api/       HTTP handlers, auth, CORS, TLS/ACME, SPA serving
crates/certo/           binary: wiring, logging, shutdown
web/                    React + Vite SPA (embedded into certo-api at build time)
tests/e2e/              Bun + TypeScript end-to-end suite
docs/                   this documentation
```

See [architecture.md](architecture.md) for how the pieces fit together.

## Common tasks

```bash
just build      # vite build + cargo build --profile dist -> dist/certo
just test       # cargo nextest run --workspace
just e2e        # debug build + tests/e2e
just lint       # rustfmt check + clippy + tsc --noEmit
just rs-ci      # every Rust gate CI runs (fmt, clippy, nextest, doc tests, deny, shear, typos, MSRV)
just dev        # SPA + backend behind nsl (frontend and /api on one origin)
just serve      # backend only, via nsl
just clean      # remove dist/, web/dist and target/
just release 1.2.3   # tag v1.2.3 and push (the release workflow publishes the assets)
```

Without `just`:

```bash
cd web && npm install && npx vite build && cd ..
cargo build --release -p certo
./target/release/certo -c config.toml
```

A missing `web/dist` does not break the build; the binary then serves no SPA. Set
`CERTO_VERSION` at build time to override the reported version (release builds use the
git tag).

Front-end dev server (proxies `/api` to `localhost:3000`):

```bash
cd web && npm run dev
```

An on-disk `web/dist` overrides the embedded SPA at runtime, so a `vite build` is picked up
without rebuilding the binary.

## Rust tests

```bash
cargo nextest run --workspace                # all crates
cargo nextest run -p certo-dns hosted        # one crate, filtered
cargo test --doc --workspace
```

Storage tests use in-memory SQLite plus `tests/fixtures/go-v2.db`, a database written by
the Go release, to check upgrades. DNS tests bind ephemeral UDP/TCP ports on localhost and
query with raw sockets and hickory-proto; API tests drive the router with `tower::ServiceExt::oneshot`.

## End-to-end tests

`tests/e2e` builds and runs the real binary on ephemeral ports with a temporary database,
drives the HTTP API with `fetch`, and verifies published records with `dig` — so it covers
the full HTTP → database → DNS path.

```bash
cd tests/e2e
bun test                       # runs cargo build -p certo automatically
CERTO_BIN=/path/to/certo bun test   # test a prebuilt binary
bun test src/acmedns.test.ts   # one feature file
```

One server is started once (memoised in `src/context.ts`) and shared by every feature file;
each file calls `await ensureServer()` in `beforeAll`. Files are split per feature:
`public`, `account`, `keys`, `domains`, `dns`, `httpreq`, `acmedns`, `register`, `admin`,
`zones`.
See [tests/e2e/README.md](../tests/e2e/README.md).

## Releasing

A `v*` tag runs `.github/workflows/release.yml`:

1. The `binary` stage of the `Dockerfile` cross-compiles static musl binaries for
   `linux/amd64` and `linux/arm64` (`CERTO_VERSION` is the tag without `v`, so
   `certo --version` reports the release id).
2. They are packed as `certo-linux-<arch>.tar.gz` (holding `certo`) and published in a
   GitHub release together with `certo-<tag>.tar.gz` (sample `config.toml`, `lode.toml`,
   `docker-compose.yml`). lode deployments pick the asset by name and verify it against the
   sha256 digest GitHub records for it; the assets are not signed with a publisher key.

Build the same binaries locally with

```bash
docker buildx build --target binary --platform linux/amd64,linux/arm64 \
  --build-arg VERSION=v1.2.3 --output type=local,dest=out .
```

## Conventions

- Lint policy lives in `[workspace.lints]` (`Cargo.toml`): warnings are errors, clippy
  pedantic + nursery, no `unwrap`/`expect`/`panic` outside tests, `#![forbid(unsafe_code)]`
  in every crate. Run `cargo fmt --all` and `cargo clippy --workspace --all-targets`.
- Errors are typed per crate (`thiserror`); handlers return `ApiError` with the codes listed
  in [api.md](api.md#error-codes) — reuse an existing code rather than inventing a synonym.
- Secrets (`jwt_secret`, `admin_key`, DNSSEC and TSIG keys) are wrapped in `secrecy` types
  and never logged.
- `Db` methods each take the connection mutex; compose multi-statement work inside one
  method (and one transaction) instead of calling methods from each other.

## Changing the database schema

1. Add the column/table to the `CREATE TABLE` statements in
   `crates/certo-db/src/schema.rs` (so fresh databases are correct).
2. Bump `DB_VERSION`.
3. Add an upgrade step to the chain in `schema.rs` that migrates the previous version in
   one transaction and writes the new `db_version`.
4. Cover it with a test that opens an old-version database (e.g. `tests/fixtures/go-v2.db`)
   and asserts the upgrade.

## Documentation

Keep docs in step with the code — the config table in
[configuration.md](configuration.md) documents *code* defaults, and
[api.md](api.md) mirrors the routes registered in `crates/certo-api/src/lib.rs`. The
machine-readable copies (`llms.txt`, `crates/certo-api/src/llms-api.txt`, the latter served
at `/llms.txt`) must be updated together with any protocol or endpoint change. Record
decisions in `docs/decisions/` and progress in `docs/changelog.md`.
