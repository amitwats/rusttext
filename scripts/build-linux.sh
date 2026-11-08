#!/usr/bin/env bash
# Build RustText for Linux.
#
# Usage: scripts/build-linux.sh [--debug] [--skip-tests] [--target <triple>]
#
# Produces dist/linux/rusttext and dist/rusttext-<version>-linux-<arch>.tar.gz
set -euo pipefail

cd "$(dirname "$0")/.."

PROFILE="release"
RUN_TESTS=1
TARGET=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) PROFILE="debug" ;;
        --skip-tests) RUN_TESTS=0 ;;
        --target) TARGET="$2"; shift ;;
        -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

command -v cargo >/dev/null || { echo "error: cargo not found - install Rust from https://rustup.rs" >&2; exit 1; }

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f2)
ARCH=${TARGET:-$(uname -m)}
CARGO_ARGS=()
[[ "$PROFILE" == "release" ]] && CARGO_ARGS+=(--release)
[[ -n "$TARGET" ]] && CARGO_ARGS+=(--target "$TARGET")

if [[ $RUN_TESTS -eq 1 ]]; then
    echo "==> Running tests"
    cargo test "${CARGO_ARGS[@]}"
fi

echo "==> Building RustText $VERSION ($PROFILE)"
cargo build "${CARGO_ARGS[@]}"

OUT_DIR="target/${TARGET:+$TARGET/}$PROFILE"
mkdir -p dist/linux
cp "$OUT_DIR/rusttext" dist/linux/
cp README.md dist/linux/

ARCHIVE="dist/rusttext-$VERSION-linux-$ARCH.tar.gz"
tar -czf "$ARCHIVE" -C dist/linux .
echo "==> Done: dist/linux/rusttext and $ARCHIVE"
