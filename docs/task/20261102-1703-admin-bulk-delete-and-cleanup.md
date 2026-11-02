# 20261102-1703-admin-bulk-delete-and-cleanup Bulk deletion of users and domains and cleanup of unused acme-dns registrations

- **status**: completed
- **priority**: P2
- **owner**: claude/session-f6963de7
- **createdAt**: 2026-11-02 17:03

## Description

The admin area deletes users and domains one at a time, and nothing removes the
anonymous acme-dns registrations (`POST /register` creates an `acme-<id>` account, a
domain row and a key each time) that are never used: the owner's database has 547 of
them, 546 without a single challenge value, next to 4 real accounts. The owner asked for
bulk deletion of users and domains and a cleanup function for "domains without any
user".

Acceptance: admin API and dashboard can delete a selected set of users and of domains in
one action; a cleanup action previews and removes anonymous acme-dns accounts (with their
domain, key and challenge values) that never published a value and are older than a
configurable number of days; Rust gates, e2e, web gates green; docs updated.

## ActiveForm

Adding bulk deletion and acme-dns cleanup to the admin area.

## Dependencies

- **blocked by**: (none)
- **blocks**: (none)

## Notes

Requested by the owner 2026-11-02: "当前不支持批量删除用户和域名，另外没有清理功能，需要清理没有任何用户的域名".
Plan: 20261102-1703-admin-bulk-delete-and-cleanup.

- complete: Delivered with plan 20261102-1703
