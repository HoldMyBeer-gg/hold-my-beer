#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

# Use rustup-managed Rust (Homebrew's rustc lacks cross-compilation targets)
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export RUSTC="$CARGO_HOME/bin/rustc"
export RUSTUP_TOOLCHAIN=stable
export PATH="$CARGO_HOME/bin:$PATH"

# Clear out what an earlier run of this script left behind, and nothing else.
#
# A dev build runs from cargo's debug directory, so that path is what gets
# matched. The installed app ("/Applications/Hold My Beer.app/Contents/MacOS/
# hold-my-beer-gui") has the same program name and must not be touched, and
# neither must a `tauri dev` in some other project.
pkill -f "/debug/hold-my-beer-gui" 2>/dev/null || true

# The dev page is served on port 1421. If our own dev server from an earlier
# run still holds it, stop that. If anything else holds it, say so and stop:
# killing a stranger's process is not this script's call.
for pid in $(lsof -ti tcp:1421 2>/dev/null || true); do
    cmd=$(ps -p "$pid" -o command= 2>/dev/null || true)
    case "$cmd" in
        *dev-server.js*)
            kill "$pid" 2>/dev/null || true
            ;;
        *)
            echo "Port 1421 is in use by another program (pid $pid): $cmd" >&2
            echo "Close it, then run this script again." >&2
            exit 1
            ;;
    esac
done

# Short pause to let the port release
sleep 1

# `pnpm dev` runs `tauri dev`, whose beforeDevCommand builds collab and
# collab-server and stages them as sidecars (src-tauri/stage-sidecars.mjs).
# Ask it for debug builds, which are much quicker than the default release.
export PROFILE=debug

exec pnpm dev
