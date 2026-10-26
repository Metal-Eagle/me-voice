#!/usr/bin/env bash
# Builds me-voice on macOS and packages it as an app bundle (and optionally a disk image).
#
#   ./scripts/build-macos.sh              release build -> dist/me-voice.app
#   ./scripts/build-macos.sh --dmg        also make installer/output/me-voice-<version>.dmg
#   ./scripts/build-macos.sh --test       run the unit tests
#
# Needs: Xcode command-line tools (xcode-select --install), Rust (https://rustup.rs),
# CMake and LLVM (brew install cmake llvm). Optional: librsvg (brew install librsvg) for the app icon.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "build-macos.sh only runs on macOS. On Windows use build.ps1; in WSL see the README." >&2
    exit 1
fi

DMG=0
TEST=0
for arg in "$@"; do
    case "$arg" in
        --dmg) DMG=1 ;;
        --test) TEST=1 ;;
        *) echo "Unknown option: $arg" >&2; exit 2 ;;
    esac
done

cd "$(dirname "$0")/.."

# .cargo/config.toml points bindgen at the Windows LLVM install; environment variables win over it.
LLVM_PREFIX="$(brew --prefix llvm 2>/dev/null || true)"
if [[ -n "$LLVM_PREFIX" && -d "$LLVM_PREFIX/lib" ]]; then
    export LIBCLANG_PATH="$LLVM_PREFIX/lib"
else
    export LIBCLANG_PATH="$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib"
fi
export BINDGEN_EXTRA_CLANG_ARGS="-isysroot $(xcrun --show-sdk-path)"

if [[ $TEST -eq 1 ]]; then
    cargo test
    exit $?
fi

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
APP="dist/me-voice.app"
BIN="target/release/me-voice"

# Apple silicon only: the ONNX Runtime crate (ort) has no prebuilt binary for Intel Macs.
cargo build --release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/me-voice"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# App icon from assets/icon.svg (skipped when rsvg-convert is not installed).
if command -v rsvg-convert >/dev/null 2>&1; then
    ICONSET="$(mktemp -d)/me-voice.iconset"
    mkdir -p "$ICONSET"
    for size in 16 32 128 256 512; do
        rsvg-convert -w $size -h $size assets/icon.svg -o "$ICONSET/icon_${size}x${size}.png"
        rsvg-convert -w $((size * 2)) -h $((size * 2)) assets/icon.svg -o "$ICONSET/icon_${size}x${size}@2x.png"
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/me-voice.icns"
else
    echo "rsvg-convert not found: building without an app icon (brew install librsvg)."
fi

# Ad-hoc signature: not a Developer ID, but it gives the app a stable identity so macOS
# remembers the Microphone and Accessibility permissions you grant it.
codesign --force --deep --sign - "$APP"

echo "Built $APP (version $VERSION)"

if [[ $DMG -eq 1 ]]; then
    DMG_FILE="installer/output/me-voice-$VERSION.dmg"
    STAGE="$(mktemp -d)"
    cp -R "$APP" "$STAGE/"
    ln -s /Applications "$STAGE/Applications"
    mkdir -p installer/output
    rm -f "$DMG_FILE"
    hdiutil create -volname "me-voice" -srcfolder "$STAGE" -ov -format UDZO "$DMG_FILE" >/dev/null
    echo "Built $DMG_FILE"
fi
