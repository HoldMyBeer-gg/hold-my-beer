#!/usr/bin/env bash
# Build collab + collab-server from the workspace and copy them into
# src-tauri/binaries/<name>-<host-triple>, which is where Tauri's externalBin
# machinery expects to find sidecars.
#
# Run this before `pnpm tauri dev` / `pnpm tauri build`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$SCRIPT_DIR/binaries"

TRIPLE="$(rustc -vV | awk '/^host/ { print $2 }')"
if [[ -z "${TRIPLE}" ]]; then
  echo "could not determine host target triple" >&2
  exit 1
fi

PROFILE="${PROFILE:-release}"
CARGO_FLAG=""
if [[ "$PROFILE" == "release" ]]; then
  CARGO_FLAG="--release"
fi

echo "Building collab + collab-server ($PROFILE, $TRIPLE)..."
cargo build $CARGO_FLAG --manifest-path "$REPO_ROOT/Cargo.toml" \
  -p holdmybeer-cli -p holdmybeer-server

mkdir -p "$BIN_DIR"
for name in collab collab-server; do
  src="$REPO_ROOT/target/$PROFILE/$name"
  dst="$BIN_DIR/$name-$TRIPLE"
  if [[ ! -f "$src" ]]; then
    echo "missing built binary: $src" >&2
    exit 1
  fi
  cp "$src" "$dst"
  chmod +x "$dst"
  echo "staged $dst"
done
