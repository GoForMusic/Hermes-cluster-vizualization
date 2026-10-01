#!/usr/bin/env bash
# Named volumes are created owned by root; hand them to the dev user, then show what the toolchain looks like.
set -euo pipefail
sudo chown -R "$(id -u):$(id -g)" target /usr/local/cargo/registry /usr/local/cargo/git 2>/dev/null || true
cargo --version
rustc --version
protoc --version
node --version
npm --version
docker --version 2>/dev/null || echo "docker: not available in this container"
