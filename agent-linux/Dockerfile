# syntax=docker/dockerfile:1
#
# Linux agent image (linux/amd64), one static binary. It talks to the hub over gRPC and needs no CA bundle: the
# trusted roots are compiled in (a private CA goes in HUB_CA_FILE).
# The build context is the repository root:
#   docker build -f agent-linux/Dockerfile -t hermes-agent-linux:dev .
#   docker buildx build -f agent-linux/Dockerfile --platform linux/amd64 -t REGISTRY/hermes-agent-linux:1.0.0 --push .
# CI does this when you push a tag agent-linux-X.Y.Z, with --build-arg VERSION=X.Y.Z.
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev musl-tools && rm -rf /var/lib/apt/lists/*
# a static binary for the architecture this stage runs on (buildx runs it once per platform)
RUN rustup target add "$(uname -m)-unknown-linux-musl"
# the release version, from the git tag (server-1.2.3 -> 1.2.3); without it this is a development build and reports itself as such
ARG VERSION=
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY proto ./proto
COPY pkg ./pkg
COPY agent-linux ./agent-linux
# cargo wants every member of the workspace to exist, not their code: the manifests of the hub and of the Windows agent, and an empty main.rs each
COPY hermes-server/Cargo.toml ./hermes-server/Cargo.toml
COPY agent-windows/Cargo.toml ./agent-windows/Cargo.toml
RUN mkdir hermes-server/src agent-windows/src && echo 'fn main() {}' | tee hermes-server/src/main.rs > agent-windows/src/main.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target \
    if [ -n "$VERSION" ]; then export HERMES_VERSION="$VERSION"; fi && \
    arch="$(uname -m)" && export "CC_${arch}_unknown_linux_musl=musl-gcc" && \
    cargo build --release --locked -p hermes-agent-linux --target "${arch}-unknown-linux-musl" && \
    install -D "target/${arch}-unknown-linux-musl/release/hermes-agent" /out/agent

# nothing but the binary, as an unprivileged user
FROM scratch
ARG VERSION=dev
LABEL org.opencontainers.image.version="$VERSION" org.opencontainers.image.title="hermes-agent-linux"
COPY --from=build /out/agent /agent
USER 65532:65532
ENTRYPOINT ["/agent"]
