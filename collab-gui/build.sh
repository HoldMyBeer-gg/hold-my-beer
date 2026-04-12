#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

# Use rustup-managed Rust (Homebrew's rustc lacks cross-compilation targets)
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export RUSTC="$CARGO_HOME/bin/rustc"
export RUSTUP_TOOLCHAIN=stable
export PATH="$CARGO_HOME/bin:$PATH"

# Kill stale hold-my-beer-gui processes
pkill -f "hold-my-beer-gui" 2>/dev/null || true

# Kill any node dev-server on port 1421
lsof -ti tcp:1421 | xargs kill -9 2>/dev/null || true

# Kill any lingering tauri dev processes
pkill -f "tauri dev" 2>/dev/null || true

# Short pause to let ports release
sleep 1

# Build collab + collab-server and stage them as sidecars next to the GUI
# binary. Tauri's externalBin machinery expects them at
# src-tauri/binaries/<name>-<host-triple>.
PROFILE=debug ./src-tauri/stage-sidecars.sh

exec pnpm dev
