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

# `pnpm dev` runs `tauri dev`, whose beforeDevCommand builds collab and
# collab-server and stages them as sidecars (src-tauri/stage-sidecars.mjs).
# Ask it for debug builds, which are much quicker than the default release.
export PROFILE=debug

exec pnpm dev
