# Container image: Alpine runtime, static musl binary cross-built with cargo-zigbuild

- **date**: 2026-10-17
- **status**: accepted
- **relatedPlan**: 20261016-1840-authoritative-zones-ptr

## Context

The Go image was `alpine` with `bind-tools` and `ca-certificates`, published for `linux/amd64` and `linux/arm64`. The Rust binary needs a C toolchain for `aws-lc-sys` and the bundled SQLite, and must also build for arm64 on amd64 CI runners.

## Decision

- Build stage: `ghcr.io/rust-cross/cargo-zigbuild` on the build platform, cross-compiling `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl` with the `dist` profile. The result is a static executable (no `PT_INTERP`).
- Runtime stage: keep `alpine:3.22` with `bind-tools` and `ca-certificates`, as before.

## Reasons

- Zig as the cross C compiler/linker avoids emulated (QEMU) compilation of aws-lc and SQLite, which takes far longer than a native build.
- Alpine keeps `dig` in the image for troubleshooting DNS from inside the container, and `ca-certificates` for the platform TLS verifier used by the ACME client. Operators of the Go image keep the same shell and tooling.
- A `scratch`/distroless image would be smaller but has no shell or `dig` for debugging; not worth the change for this service.

## Consequences

- cargo-zigbuild must be ≥ 0.23: 0.20 fails to link aarch64-musl because rustc passes `--fix-cortex-a53-843419`, which older zig wrappers reject.
- Only the runtime stage's `apk add` runs under emulation for arm64 (the workflow sets up QEMU).
