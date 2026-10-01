# syntax=docker/dockerfile:1
#
# Windows agent image (windows/amd64). Built natively on Windows now, no cross-compile: compile the binary first
# (cargo build --release -p hermes-agent-windows, the default host target, no mingw), then this just COPYs it in.
# From the repository root, on a Windows machine with Docker in Windows-container mode:
#   cargo build --release --locked -p hermes-agent-windows
#   Copy-Item target/release/infraviz-agent.exe agent-windows/agent.exe   # target/ is in .dockerignore, this isn't
#   docker build -f agent-windows/Dockerfile --build-arg WINDOWS_BASE=mcr.microsoft.com/windows/nanoserver:ltsc2022 --build-arg VERSION=1.0.0 -t hermes-agent-windows:1.0.0-ltsc2022 .
# CI does this when you push a tag agent-windows-X.Y.Z, with --build-arg VERSION=X.Y.Z.
#
# WINDOWS_BASE must match your nodes: ltsc2022 for Windows Server 2022, ltsc2019 for Windows Server 2019.
# (Windows containers run on a host with the same kernel version unless Hyper-V isolation is used.)
ARG WINDOWS_BASE=mcr.microsoft.com/windows/nanoserver:ltsc2022
FROM ${WINDOWS_BASE}
ARG VERSION=dev
LABEL org.opencontainers.image.version="$VERSION" org.opencontainers.image.title="hermes-agent-windows"
COPY agent-windows/agent.exe C:/agent.exe
# reading the Docker engine's named pipe needs administrator rights inside the container
USER ContainerAdministrator
ENTRYPOINT ["C:\\agent.exe"]
