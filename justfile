set dotenv-load

# Build frontend
web-build:
    cd web && npx vite build

# Build Go binary (with embedded frontend)
version := `git describe --tags --always --dirty 2>/dev/null || echo dev`

# Build Go binary (with embedded frontend)
build: web-build
    mkdir -p dist
    CGO_ENABLED=0 go build -ldflags="-s -w -X main.version={{version}}" -o dist/certo .

# Run all tests
test:
    go test ./pkg/...

# Start dev server (frontend + backend on same domain via nsl path prefix)
dev: build
    #!/usr/bin/env bash
    cd web && nsl run -n certo npx vite &
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

# Format and lint
lint:
    go vet ./...
    cd web && npx tsc --noEmit

# Tag and push a release
release version:
    git tag v{{version}}
    git push origin v{{version}}
