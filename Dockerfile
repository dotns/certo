FROM --platform=$BUILDPLATFORM node:24-alpine AS frontend
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Cross-compiles static musl binaries on the build platform (no emulation).
FROM --platform=$BUILDPLATFORM ghcr.io/rust-cross/cargo-zigbuild:0.23.4 AS backend
ARG TARGETARCH
WORKDIR /build
COPY rust-toolchain.toml ./
RUN rustup show active-toolchain || rustup toolchain install \
 && rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=frontend /build/web/dist ./web/dist
ARG VERSION
# Both platforms build in parallel and share these caches: `sharing=locked`
# serializes them, otherwise the two cargo processes race unpacking crates.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported arch $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && CERTO_VERSION="${VERSION#v}" cargo zigbuild --locked --profile dist -p certo --target "$target" \
 && cp "target/$target/dist/certo" /certo

# Only the binary, for `docker buildx build --target binary --output type=local,dest=out`:
# the release workflow exports linux_amd64/certo and linux_arm64/certo from it.
FROM scratch AS binary
COPY --from=backend /certo /certo

FROM alpine:3.22
RUN apk add --no-cache bind-tools ca-certificates
WORKDIR /app
COPY --from=backend /certo /app/certo
VOLUME ["/app/data"]
ENTRYPOINT ["/app/certo"]
EXPOSE 53 53/udp 3000
