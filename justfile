set dotenv-load

# Build frontend
web-build:
    cd web && bun install --frozen-lockfile && bun run build

version := `git describe --tags --always --dirty 2>/dev/null || echo dev`

# Build release binary (with embedded frontend)
build: web-build
    mkdir -p dist
    CERTO_VERSION={{version}} cargo build --locked --profile dist -p certo
    cp target/dist/certo dist/certo

# Run all tests
test:
    cargo nextest run --workspace --locked

# Build the static pdns_server next to the debug certo binary (needs docker)
pdns-build:
    docker buildx build --target pdns-binary --output type=local,dest=target/debug .

# Run the e2e suite against a debug build
e2e: pdns-build
    cargo build --locked -p certo
    cd tests/e2e && CERTO_BIN={{justfile_directory()}}/target/debug/certo bun test

# Start dev server (frontend + backend on same domain via nsl path prefix)
dev: build
    #!/usr/bin/env bash
    cd web && bun run dev &
    FRONTEND_PID=$!
    nsl run -n certo:/api dist/certo -c config.toml &
    BACKEND_PID=$!
    trap "kill $FRONTEND_PID $BACKEND_PID 2>/dev/null" EXIT
    wait

# Start backend only via nsl
serve: build
    nsl run -n certo dist/certo -c config.toml

# Clean build artifacts and data
clean:
    rm -rf dist
    rm -rf web/dist
    cargo clean

# Format and lint
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked
    cd web && bun run lint && bun run typecheck

# Rust quality gates
rs-ci:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features --locked
    cargo nextest run --workspace --all-features --locked
    cargo test --doc --workspace --all-features --locked
    cargo deny check
    cargo shear
    typos
    cargo hack check --rust-version --workspace --all-targets --locked

# Tag and push a release
release version:
    git tag v{{version}}
    git push origin v{{version}}
