# syntax=docker/dockerfile:1
#
# Windows agent image (windows/amd64). It can be built from Linux: the binary is cross-compiled in a Linux stage and the final Windows stage only
# COPYs it, so no Windows host is needed. The build context is the repository root:
#   docker buildx build -f agent-windows/Dockerfile --platform windows/amd64 \
#       --build-arg WINDOWS_BASE=mcr.microsoft.com/windows/nanoserver:ltsc2022 --build-arg VERSION=1.0.0 \
#       -t REGISTRY/hermes-agent-windows:1.0.0-ltsc2022 --push .
# CI does this when you push a tag agent-windows-X.Y.Z, with --build-arg VERSION=X.Y.Z.
#
# WINDOWS_BASE must match your nodes: ltsc2022 for Windows Server 2022, ltsc2019 for Windows Server 2019.
# (Windows containers run on a host with the same kernel version unless Hyper-V isolation is used.)
ARG WINDOWS_BASE=mcr.microsoft.com/windows/nanoserver:ltsc2022

FROM --platform=$BUILDPLATFORM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev gcc-mingw-w64-x86-64 && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu
# the release version, from the git tag (agent-windows-1.2.3 -> 1.2.3); without it this is a development build and reports itself as such
ARG VERSION=
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY proto ./proto
COPY pkg ./pkg
COPY agent-windows ./agent-windows
# cargo wants every member of the workspace to exist, not their code: the manifests of the hub and of the Linux agent, and an empty main.rs each
COPY hermes-server/Cargo.toml ./hermes-server/Cargo.toml
COPY agent-linux/Cargo.toml ./agent-linux/Cargo.toml
RUN mkdir hermes-server/src agent-linux/src && echo 'fn main() {}' | tee hermes-server/src/main.rs > agent-linux/src/main.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target \
    if [ -n "$VERSION" ]; then export HERMES_VERSION="$VERSION"; fi && \
    export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc && \
    cargo build --release --locked -p hermes-agent-windows --target x86_64-pc-windows-gnu && \
    install -D target/x86_64-pc-windows-gnu/release/infraviz-agent.exe /out/agent.exe && \
    x86_64-w64-mingw32-objdump -p /out/agent.exe | grep "DLL Name" | sort -u

FROM ${WINDOWS_BASE}
ARG VERSION=dev
LABEL org.opencontainers.image.version="$VERSION" org.opencontainers.image.title="hermes-agent-windows"
COPY --from=build /out/agent.exe C:/agent.exe
# reading the Docker engine's named pipe needs administrator rights inside the container
USER ContainerAdministrator
ENTRYPOINT ["C:\\agent.exe"]
