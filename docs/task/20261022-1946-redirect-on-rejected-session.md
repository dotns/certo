# 20261022-1946-redirect-on-rejected-session Return to the sign-in page when the session is rejected

- **status**: completed
- **priority**: P1
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-10-22 19:46

## Description

When the API rejects the dashboard's session token (expired JWT, server restarted with a
new secret, deleted user), the pages stay open and show the error inline ("unauthorized")
instead of returning to the sign-in page. `request()` only clears the session for the
codes `invalid_token`, `token_expired` and `user_not_found`, but `certo-api` answers every
rejected bearer with `401 unauthorized`, so the sign-out never happens.

Acceptance: any `401` on a session-authenticated request clears the session and the app
layout redirects to `/login`; admin-key rejections (`X-Admin-Key`) keep the session;
a unit test covers the `unauthorized` code; web gates green.

## ActiveForm

Redirecting to sign-in on a rejected session.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Reported by the owner 2026-10-22 19:46: "当前如果校验错误，需要返回到登陆页面，不是在页面里面显示错误".

- complete: Any session 401 signs out and returns to /login
