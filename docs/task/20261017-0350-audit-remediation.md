# 20261017-0350-audit-remediation Remediate repository audit findings

- **status**: completed
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-10-17 03:50

## Description

Fix the five findings of the 2026-10-17 repository audit of the Rust implementation:

1. `DynamicCatalog` holds a tokio `RwLock` read guard across the whole DNS request; a pending writer (zone publish) blocks every new query. Replace with an immutable catalog snapshot swapped atomically.
2. `Zones::publish` runs `build_zone` (record parsing, DNSSEC signing) synchronously on the async runtime while holding the publish mutex. Move the build to `spawn_blocking`.
3. User deletion is orchestrated by `certo_dns::Zones::delete_user` and challenge TXT cleanup only happens in `DELETE /api/profile`; `DELETE /admin/users/{id}` and both `remove_domain` endpoints leave TXT rows behind. Make `Db::delete_user` and `Db::remove_user_domain` cascade to TXT rows, and orchestrate deletion from the API crate.
4. Zone overlap and CNAME exclusivity are checked outside the write transaction (TOCTOU). Enforce both inside the `certo-db` transactions.
5. Registration maps every storage error to `409 username_taken`. Only unique violations may map to 409.

Acceptance: all Rust gates green; new tests cover TXT cascade on user/domain deletion, DB-level overlap and CNAME rejection, and catalog replacement without awaiting under a lock; e2e suite unchanged and green.

## ActiveForm

Remediating audit findings in catalog, zones, db and api crates.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Plan: 20261017-0350-audit-remediation. Out of scope (separate decision): in-memory copy of the `txt` table for the DNS hot path.

- complete: All gates green: clippy, 77 nextest, doc tests, deny, shear, typos, MSRV, e2e 38/38.
