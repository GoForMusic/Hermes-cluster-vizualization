# Development image: Rust (hub + agents), Node (React + TypeScript), protoc (gRPC contract).
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev pkg-config sqlite3 musl-tools \
    && rm -rf /var/lib/apt/lists/*

# static agent binaries: the agent image is FROM scratch / distroless
RUN rustup target add x86_64-unknown-linux-musl

# Node comes from the official image, so the version matches what CI builds with
COPY --from=node:22-bookworm /usr/local/bin/node /usr/local/bin/node
COPY --from=node:22-bookworm /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx

