# certo e2e tests (Bun + TypeScript)

End-to-end tests that build and run the **real `certo` binary** (API + DNS server on a
temp DB and local ports), drive every HTTP API with `fetch`, and verify the full
challenge loop by querying the DNS server with `dig`.

## Requirements

- [Bun](https://bun.sh) (`bun test`)
- Go toolchain (the suite builds `certo` in `beforeAll`)
- `dig` (bind-tools) for DNS assertions

## Run

```bash
cd tests/e2e
bun install        # optional — only for editor type-checking; bun test runs without it
bun test
```

By default the suite builds `certo` itself (with a placeholder embedded SPA, since these
tests exercise the API/DNS, not the web UI). To test a prebuilt binary instead:

```bash
CERTO_BIN=/path/to/certo bun test
```

The server binds ephemeral free ports for the API and DNS listeners (so a leftover server
from a prior run can never collide); override via `startServer({ apiPort, dnsPort })`.

## Layout

All code lives in `src/`. The server is built and started once (memoized in
`src/context.ts`) and shared across every feature file, then torn down on process exit.

```
src/
  helpers.ts          build/run the binary, HTTP client, dig, utils
  context.ts          shared server (ensureServer) + req() + newUser()
  public.test.ts      /health, /api/info, /llms.txt
  account.test.ts     register, login, profile, account deletion
  keys.test.ts        API key create/list/delete, global-key guard
  domains.test.ts     add/list/duplicate/delete, isolation, scope filter
  dns.test.ts         base-domain A record
  httpreq.test.ts     /present + /cleanup (DNS-verified), scope-based auto-create
  acmedns.test.ts     HTTP storage Fetch/FetchAll, /update, rolling cap, cross-protocol
  admin.test.ts       X-Admin-Key auth, users/domains/records
```

Each feature file calls `await ensureServer()` in its `beforeAll`; the first call starts
the server, the rest reuse it.
