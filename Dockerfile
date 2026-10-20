FROM --platform=$BUILDPLATFORM oven/bun:1-alpine AS frontend
WORKDIR /build/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/ ./
RUN bun run build

# Fully static PowerDNS Authoritative (gsqlite3 built in, no dynamic modules). certo
# spawns this binary from the directory of its own executable.
FROM alpine:3.22 AS pdns
RUN apk add --no-cache build-base autoconf automake libtool pkgconf curl \
    boost-dev boost1.84-static openssl-dev openssl-libs-static sqlite-dev sqlite-static \
    luajit-dev zlib-dev zlib-static linux-headers
ARG PDNS_VERSION=5.0.7
WORKDIR /src
RUN curl -fsSL "https://downloads.powerdns.com/releases/pdns-${PDNS_VERSION}.tar.bz2" \
  | tar -xj --strip-components=1
# libtool drops `-static` from LDFLAGS at link time, hence `-all-static` for make.
RUN ./configure --prefix=/opt/pdns --sysconfdir=/etc/pdns \
      --with-modules="gsqlite3" --with-dynmodules="" --with-lua=luajit \
      --without-libsodium --without-libdecaf --disable-lua-records --disable-ixfrdist \
      --disable-systemd --disable-unit-tests --disable-dependency-tracking \
      --enable-static --disable-shared LDFLAGS="-static" > configure.log 2>&1 \
 || { tail -n 40 configure.log; exit 1; }
RUN make -j"$(nproc)" LDFLAGS="-all-static -static-libgcc -static-libstdc++" > make.log 2>&1 \
 || { tail -n 80 make.log; exit 1; }
RUN make install-strip > install.log 2>&1 && /opt/pdns/sbin/pdns_server --version

# Only pdns_server, for the CI e2e job: `docker buildx build --target pdns-binary --output type=local,dest=target/debug .`
FROM scratch AS pdns-binary
COPY --from=pdns /opt/pdns/sbin/pdns_server /pdns_server

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

# Both binaries, for `docker buildx build --target binary --output type=local,dest=out`:
# the release workflow packs certo and pdns_server from it.
FROM scratch AS binary
COPY --from=backend /certo /certo
COPY --from=pdns /opt/pdns/sbin/pdns_server /pdns_server

FROM alpine:3.22
RUN apk add --no-cache bind-tools ca-certificates
WORKDIR /app
COPY --from=backend /certo /app/certo
COPY --from=pdns /opt/pdns/sbin/pdns_server /app/pdns_server
VOLUME ["/app/data"]
ENTRYPOINT ["/app/certo"]
EXPOSE 53 53/udp 3000
