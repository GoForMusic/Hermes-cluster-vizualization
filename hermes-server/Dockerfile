# syntax=docker/dockerfile:1
#
# The hub image (linux/amd64): the hub itself (REST + SSE for the browser, gRPC for the agents, one port) and HERMES, its web app.
# The build context is the repository root:
#   docker build -f hermes-server/Dockerfile -t hermes-server:dev .
#   docker buildx build -f hermes-server/Dockerfile --platform linux/amd64 -t REGISTRY/hermes-server:1.0.0 --push .
# CI does this when you push a tag server-X.Y.Z, with --build-arg VERSION=X.Y.Z.
# HERMES, the web app: React + TypeScript, built to static files. The TypeScript types it uses come from the Rust model and are committed
# (src/generated), so this stage needs no Rust.
FROM node:22-bookworm AS web
WORKDIR /web
COPY hermes-server/web/package.json hermes-server/web/package-lock.json ./
RUN npm ci
COPY hermes-server/web ./
RUN npm run build

FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev musl-tools && rm -rf /var/lib/apt/lists/*
# a static binary for the architecture this stage runs on (buildx runs it once per platform); SQLite is compiled in
RUN rustup target add "$(uname -m)-unknown-linux-musl"
# the release version, from the git tag (server-1.2.3 -> 1.2.3); without it this is a development build and reports itself as such
ARG VERSION=
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY proto ./proto
# only what the hub is built from: a change in the web app, in an agent or in a library the hub does not use must not build the hub again
COPY pkg/proto ./pkg/proto
COPY hermes-server/Cargo.toml ./hermes-server/Cargo.toml
COPY hermes-server/src ./hermes-server/src
COPY hermes-server/templates ./hermes-server/templates
# cargo wants every member of the workspace to exist, not their code: their manifests, and an empty main.rs / lib.rs each
COPY agent-linux/Cargo.toml ./agent-linux/Cargo.toml
COPY agent-windows/Cargo.toml ./agent-windows/Cargo.toml
COPY pkg/agentkit/Cargo.toml ./pkg/agentkit/Cargo.toml
COPY pkg/swarm/Cargo.toml ./pkg/swarm/Cargo.toml
RUN mkdir agent-linux/src agent-windows/src pkg/agentkit/src pkg/swarm/src && \
    echo 'fn main() {}' | tee agent-linux/src/main.rs > agent-windows/src/main.rs && \
    touch pkg/agentkit/src/lib.rs pkg/swarm/src/lib.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target \
    if [ -n "$VERSION" ]; then export HERMES_VERSION="$VERSION"; fi && \
    arch="$(uname -m)" && export "CC_${arch}_unknown_linux_musl=musl-gcc" && \
    cargo build --release --locked -p hermes-hub --target "${arch}-unknown-linux-musl" && \
    install -D "target/${arch}-unknown-linux-musl/release/hermes-hub" /out/hub && \
    mkdir -p /out/data

# nothing but the binary, the web app and an empty data directory, as an unprivileged user
FROM scratch
ARG VERSION=dev
LABEL org.opencontainers.image.version="$VERSION" org.opencontainers.image.title="hermes-server"
COPY --from=build /out/hub /hub
# the hub calls container registries over https: it needs the certificate authorities to trust them
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
# the database lives here: a volume, owned by the user the hub runs as
COPY --from=build --chown=65532:65532 /out/data /data
COPY --from=web /web/dist /web
USER 65532:65532
ENV HUB_WEB=/web HUB_DATA=/data HUB_ADDR=:8080
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["/hub"]
