#!/usr/bin/env bash
# Build RustText for macOS and wrap it in an .app bundle.
#
# Usage: scripts/build-macos.sh [--debug] [--skip-tests] [--universal]
#
#   --universal  build a fat binary for Apple Silicon and Intel
#                (requires: rustup target add aarch64-apple-darwin x86_64-apple-darwin)
#
# Produces dist/macos/RustText.app and dist/rusttext-<version>-macos.zip
set -euo pipefail

cd "$(dirname "$0")/.."

PROFILE="release"
RUN_TESTS=1
UNIVERSAL=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) PROFILE="debug" ;;
        --skip-tests) RUN_TESTS=0 ;;
        --universal) UNIVERSAL=1 ;;
        -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

command -v cargo >/dev/null || { echo "error: cargo not found - install Rust from https://rustup.rs" >&2; exit 1; }

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f2)
CARGO_ARGS=()
[[ "$PROFILE" == "release" ]] && CARGO_ARGS+=(--release)

if [[ $RUN_TESTS -eq 1 ]]; then
    echo "==> Running tests"
    cargo test "${CARGO_ARGS[@]}"
fi

mkdir -p dist/macos
BIN="dist/macos/rusttext"
if [[ $UNIVERSAL -eq 1 ]]; then
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        echo "==> Building for $t"
        cargo build "${CARGO_ARGS[@]}" --target "$t"
    done
    lipo -create -output "$BIN" \
        "target/aarch64-apple-darwin/$PROFILE/rusttext" \
        "target/x86_64-apple-darwin/$PROFILE/rusttext"
else
    echo "==> Building RustText $VERSION ($PROFILE)"
    cargo build "${CARGO_ARGS[@]}"
    cp "target/$PROFILE/rusttext" "$BIN"
fi

echo "==> Creating RustText.app"
APP="dist/macos/RustText.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/rusttext"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>RustText</string>
    <key>CFBundleDisplayName</key><string>RustText</string>
    <key>CFBundleIdentifier</key><string>com.rusttext.editor</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleExecutable</key><string>rusttext</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Document</string>
            <key>CFBundleTypeRole</key><string>Editor</string>
            <key>CFBundleTypeExtensions</key>
            <array><string>docx</string><string>rtf</string><string>md</string><string>txt</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST

ARCHIVE="dist/rusttext-$VERSION-macos.zip"
rm -f "$ARCHIVE"
(cd dist/macos && zip -qry "../$(basename "$ARCHIVE")" RustText.app)
echo "==> Done: $APP and $ARCHIVE"
