FROM --platform=$BUILDPLATFORM node:22-alpine AS frontend
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM --platform=$BUILDPLATFORM golang:1.24-alpine AS backend
ARG TARGETOS TARGETARCH
RUN apk add --no-cache ca-certificates git
WORKDIR /build
COPY . .
COPY --from=frontend /build/web/dist ./web/dist
ARG VERSION=dev
RUN go mod download && GOOS=$TARGETOS GOARCH=$TARGETARCH CGO_ENABLED=0 go build -ldflags="-s -w -X main.version=${VERSION}" -o certo .

FROM alpine:3.22
RUN apk add --no-cache bind-tools ca-certificates
WORKDIR /app
COPY --from=backend /build/certo /app/certo
VOLUME ["/app/data"]
ENTRYPOINT ["/app/certo"]
EXPOSE 53 53/udp 3000
