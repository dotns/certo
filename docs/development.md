# Development

## Requirements

- Go 1.24+
- Node.js 22+ / npm (front-end)
- [`just`](https://github.com/casey/just) (optional, wraps the common tasks)
- [Bun](https://bun.sh) and `dig` (bind-tools) for the e2e suite

## Layout

```
main.go, umask_*.go     entrypoint
pkg/certo/              config, interfaces, types, logging, id generation
pkg/database/           SQLite implementation of certo.DB
pkg/api/                HTTP handlers, auth, TLS, SPA serving
pkg/nameserver/         authoritative DNS server
web/                    React + Vite SPA (embedded via web/embed.go)
tests/e2e/              Bun + TypeScript end-to-end suite
docs/                   this documentation
```

See [architecture.md](architecture.md) for how the pieces fit together.

## Common tasks

```bash
just build      # vite build + CGO_ENABLED=0 go build -o dist/certo
just test       # go test ./pkg/...
just lint       # go vet ./... + tsc --noEmit
just dev        # SPA + backend behind nsl (frontend and /api on one origin)
just serve      # backend only, via nsl
just clean      # remove dist/ and web/dist
just release 1.2.3   # tag v1.2.3 and push
```

Without `just`:

```bash
cd web && npm install && npx vite build && cd ..
CGO_ENABLED=0 go build -o dist/certo .
./dist/certo -c config.toml
```

`web/dist` must exist for `go build` to succeed, because `web/embed.go` embeds it. For
back-end-only work a placeholder is enough:

```bash
mkdir -p web/dist && echo '<!doctype html><title>certo</title>' > web/dist/index.html
```

Front-end dev server (proxies `/api` to `localhost:3000`):

```bash
cd web && npm run dev
```

An on-disk `web/dist` overrides the embedded SPA at runtime, so a `vite build` is picked up
without rebuilding the binary.

## Go tests

```bash
go test ./pkg/...            # all packages
go test -race ./pkg/...
go test -cover ./pkg/...
go test -run TestACMEDNSRollingCap ./pkg/api/
```

They use in-memory SQLite (`:memory:`) and `httptest`, so nothing binds real ports.
`pkg/api/api_test.go` builds its **own** router — adding a route in `pkg/api/api.go` does
not automatically expose it to those tests; register it in `setupTestServer` too.

## End-to-end tests

`tests/e2e` builds and runs the real binary on ephemeral ports with a temporary database,
drives the HTTP API with `fetch`, and verifies published records with `dig` — so it covers
the full HTTP → database → DNS path.

```bash
cd tests/e2e
bun test                       # builds ./certo automatically
CERTO_BIN=/path/to/certo bun test   # test a prebuilt binary
bun test src/acmedns.test.ts   # one feature file
```

One server is started once (memoised in `src/context.ts`) and shared by every feature file;
each file calls `await ensureServer()` in `beforeAll`. Files are split per feature:
`public`, `account`, `keys`, `domains`, `dns`, `httpreq`, `acmedns`, `register`, `admin`.
See [tests/e2e/README.md](../tests/e2e/README.md).

## Conventions

- Run `gofmt -l pkg/ main.go` (or `gofmt -w`) before committing; `go vet ./...` must be clean.
- Import order: standard library, then `github.com/dotns/certo/...`, then third party.
- Handlers respond via `jsonResp` / `jsonError` with the error codes listed in
  [api.md](api.md#error-codes) — reuse an existing code rather than inventing a synonym.
- Every `pkg/database` method takes the shared mutex; do not call one from another (it is
  not reentrant) — factor the shared work into an unlocked helper instead.

## Changing the database schema

1. Add the column/table to the `CREATE TABLE` statements in `pkg/database/db.go` (so fresh
   databases are correct).
2. Bump `DBVersion`.
3. Add an upgrade step to `handleDBUpgrades` that migrates the previous version and writes
   the new `db_version`.
4. Cover it with a test that seeds the old schema and asserts the upgrade, following
   `TestMigrateV1ToV2`.

## Documentation

Keep docs in step with the code — the config table in
[configuration.md](configuration.md) documents *code* defaults, and
[api.md](api.md) mirrors the routes registered in `pkg/api/api.go`. The
machine-readable copies (`llms.txt`, `pkg/api/llms-api.txt`, the latter served at
`/llms.txt`) must be updated together with any protocol or endpoint change.
