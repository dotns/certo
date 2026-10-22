# 20261021-0408-dashboard-feature-completion Complete the dashboard feature set

- **status**: completed
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-10-21 04:08

## Description

The dashboard maps the API almost one-to-one but stops there: there is no overview after
sign-in, no account page (the `DELETE /api/profile` endpoint and password changes have no
UI), challenge domains cannot be searched or checked for a correct CNAME, a new API key
is not surfaced for copying, the client setup page only knows lego and Traefik and uses
a placeholder key, zone records cannot be filtered, imported or exported, the admin area
has no per-user overview and cannot reset a password, and nothing confirms a successful
action.

Add the missing pieces so the dashboard is a complete control panel for users and
administrators: overview, account, domain CNAME check, key reveal and real client
snippets (lego, Traefik, Caddy, acme.sh, cert-manager), record filter plus zone-file
import/export, admin user overview and password reset, success toasts and a mobile menu.
The small backend additions needed for this (password change, admin password reset,
CNAME check, `allow_registration` in `GET /api/info`) are part of the task.

Acceptance: every new screen works against the real server in the e2e suite or a
Playwright smoke run; web gates (`lint`, `typecheck`, `test:coverage` ≥ 80%, `build`)
and Rust gates green; `docs/api.md`, `llms.txt` and the README updated.

## ActiveForm

Completing the dashboard feature set.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Requested by the owner 2026-10-21 04:08: "当前的ui太不完善，有太多功能缺失，完善一下".
Plan: 20261021-0408-dashboard-feature-completion.

- complete: Dashboard feature set completed; released as v0.2.0
