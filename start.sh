#!/usr/bin/env bash
# Build and launch the Hold My Beer desktop GUI.
#
# Usage:  ./start.sh
#
# What it does:
#   1. Checks that cargo, node, and pnpm are installed.
#   2. Builds the Tauri GUI (which also builds the Rust CLI + server sidecars).
#   3. Launches the built .app / binary.
#
# No CLI install needed — the GUI drives everything. If you also want the
# `collab` and `collab-server` binaries on your PATH, run ./build.sh too.

set -euo pipefail

RED=$'\033[31m'; YEL=$'\033[33m'; GRN=$'\033[32m'; DIM=$'\033[2m'; RST=$'\033[0m'

have() { command -v "$1" >/dev/null 2>&1; }

missing=()
have cargo || missing+=("cargo (Rust toolchain) → https://rustup.rs/")
have node  || missing+=("node (Node.js ≥ 20) → https://nodejs.org/")
have pnpm  || missing+=("pnpm → https://pnpm.io/installation  (or:  npm install -g pnpm)")

if [ ${#missing[@]} -gt 0 ]; then
  echo
  echo "${RED}✗ Missing required tools:${RST}"
  for m in "${missing[@]}"; do
    echo "  - $m"
  done
  echo
  echo "Install the tools above, open a fresh shell, then run ./start.sh again."
  exit 1
fi

echo "${GRN}✓${RST} cargo:  $(cargo --version)"
echo "${GRN}✓${RST} node:   $(node --version)"
echo "${GRN}✓${RST} pnpm:   $(pnpm --version)"
echo

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/collab-gui"

if [ ! -d node_modules ]; then
  echo "${YEL}→${RST} Installing frontend dependencies (first run only)…"
  pnpm install
  echo
fi

echo "${YEL}→${RST} Building the GUI (this takes a few minutes the first time)…"
pnpm run build
echo

APP_PATH="$SCRIPT_DIR/collab-gui/src-tauri/target/release/bundle/macos/Hold My Beer.app"
LINUX_BIN="$SCRIPT_DIR/collab-gui/src-tauri/target/release/hold-my-beer-gui"

case "$(uname -s)" in
  Darwin)
    if [ ! -d "$APP_PATH" ]; then
      echo "${RED}✗ Build succeeded but the .app wasn't found at:${RST}"
      echo "  $APP_PATH"
      exit 1
    fi
    echo "${GRN}✓${RST} Launching Hold My Beer.app"
    open "$APP_PATH"
    ;;
  Linux)
    if [ ! -x "$LINUX_BIN" ]; then
      echo "${RED}✗ Build succeeded but the binary wasn't found at:${RST}"
      echo "  $LINUX_BIN"
      exit 1
    fi
    echo "${GRN}✓${RST} Launching hold-my-beer-gui"
    "$LINUX_BIN" &
    ;;
  *)
    echo "${YEL}Unrecognized OS $(uname -s) — build finished but I can't auto-launch.${RST}"
    echo "${DIM}Launch manually from collab-gui/src-tauri/target/release/${RST}"
    ;;
esac
