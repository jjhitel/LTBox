#!/usr/bin/env bash
#
# Assemble a universal (Apple Silicon + Intel) LTBox.app and package it as a
# .tar.gz for GitHub Releases (no App Store, no .dmg).
#
#   misc/macos/make-app.sh [OUTPUT_DIR]   # default: dist/macos
#
# Env:
#   SKIP_BUILD=1            reuse existing per-arch release binaries
#   MACOS_SIGN_IDENTITY=…   Developer ID Application identity → hardened-runtime
#                          sign + notarization-ready. Unset → ad-hoc sign (`-`).
#                          Ad-hoc is enough for non-App-Store distribution: it
#                          lets the binary launch, and a downloader just clears
#                          the Gatekeeper quarantine once (right-click → Open,
#                          or `xattr -dr com.apple.quarantine LTBox.app`).
#                          Developer-ID notarization only removes that one-time
#                          prompt and is optional.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
cd "$REPO"

# Universal = both Mac architectures lipo'd into one binary.
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
BIN_NAME="ltbox"
APP_NAME="LTBox"
OUT_DIR="${1:-$REPO/dist/macos}"
APP="$OUT_DIR/$APP_NAME.app"
TARBALL="$OUT_DIR/$APP_NAME-macos-universal.tar.gz"
ICON_SVG="$REPO/crates/ltbox-gui/assets/icon_source.svg"

# Workspace version → CFBundleShortVersionString.
VERSION="$(sed -n -E 's/^version = "([^"]+)".*/\1/p' "$REPO/Cargo.toml" | head -1)"
[ -n "$VERSION" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

# 1. Build each arch with C deps statically linked so the bundle is
#    self-contained:
#      - LIBUSB_STATIC  → libusb1-sys vendors libusb.
#      - LZMA_API_STATIC → lzma-sys (via xz2 → noto-fonts-dl) compiles the
#        bundled liblzma from source. Without it, lzma-sys pkg-config's a
#        dynamic liblzma, which on a GitHub runner resolves to Homebrew's
#        /opt/homebrew/opt/xz/lib/liblzma.5.dylib — exactly the non-system
#        dylib the otool guard (step 5) rejects.
slices=()
for t in "${TARGETS[@]}"; do
    if [ "${SKIP_BUILD:-0}" != "1" ]; then
        rustup target add "$t" >/dev/null 2>&1 || true
        LIBUSB_STATIC=1 LZMA_API_STATIC=1 cargo build --release --locked --target "$t" -p ltbox-gui
    fi
    slice="$REPO/target/$t/release/$BIN_NAME"
    [ -x "$slice" ] || { echo "missing slice: $slice (run without SKIP_BUILD?)" >&2; exit 1; }
    slices+=("$slice")
done

# 2. Bundle skeleton + one universal binary.
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create "${slices[@]}" -output "$APP/Contents/MacOS/$BIN_NAME"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# 3. Info.plist (substitute the version).
sed "s/__SHORT_VERSION__/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"

# 4. App icon (macOS-only Liquid Glass). Two independent parts:
#    A) Legacy AppIcon.icns (CFBundleIconFile) — used by macOS < 26 and anywhere
#       an .icns is consumed. Source: the committed full-res glass
#       misc/macos/AppIcon.icns (regenerate with render-icon.sh); else rasterise
#       the cross-platform app SVG with built-in tools (qlmanage → sips →
#       iconutil, no Homebrew) — the pre-Liquid-Glass path, so Windows/Linux art
#       (icon_source.svg) is never affected.
#    B) Dynamic Liquid Glass icon for macOS 26 (light / dark / clear / tinted) —
#       compile AppIcon.icon into an Assets.car with `actool`, which ships in
#       EVERY Xcode 26 (no Icon Composer needed), drop it in Resources/, and add
#       CFBundleIconName so macOS 26 renders the dynamic icon. Without actool
#       (no Xcode) only the static .icns ships. Point at an Xcode with
#       XCODE_APP=/path/to/Xcode.app. (If actool ever crashes on a given Xcode
#       — see the Xcode 26.5 .icon report — it degrades to static-only; pin a
#       known-good Xcode via XCODE_APP if needed.)
ICON_DIR="$HERE/AppIcon.icon"
RES="$APP/Contents/Resources"

# --- A) legacy .icns ---
if [ -f "$HERE/AppIcon.icns" ]; then
    echo "Icon: legacy AppIcon.icns <- committed glass icns"
    cp "$HERE/AppIcon.icns" "$RES/AppIcon.icns"
else
    echo "Icon: legacy AppIcon.icns <- SVG fallback ($(basename "$ICON_SVG"))"
    qdir="$(mktemp -d)"
    qlmanage -t -s 1024 -o "$qdir" "$ICON_SVG" >/dev/null 2>&1
    src="$qdir/$(basename "$ICON_SVG").png"
    [ -f "$src" ] || { echo "icon rasterize failed (qlmanage produced no PNG)" >&2; exit 1; }
    iconset="$(mktemp -d)/AppIcon.iconset"
    mkdir -p "$iconset"
    gen() { sips -z "$2" "$2" "$src" --out "$iconset/$1" >/dev/null; }
    gen icon_16x16.png 16;    gen icon_16x16@2x.png 32
    gen icon_32x32.png 32;    gen icon_32x32@2x.png 64
    gen icon_128x128.png 128; gen icon_128x128@2x.png 256
    gen icon_256x256.png 256; gen icon_256x256@2x.png 512
    gen icon_512x512.png 512; gen icon_512x512@2x.png 1024
    iconutil -c icns "$iconset" -o "$RES/AppIcon.icns"
fi

# --- B) dynamic Assets.car (macOS 26) via actool, when an Xcode is present ---
actool_bin() {
    local c
    for c in \
        ${XCODE_APP:+"$XCODE_APP/Contents/Developer/usr/bin/actool"} \
        "/Applications/Xcode.app/Contents/Developer/usr/bin/actool" \
        "$(xcrun -f actool 2>/dev/null || true)"; do
        [ -n "$c" ] && [ -x "$c" ] && { printf '%s\n' "$c"; return 0; }
    done
    return 1
}

if [ -d "$ICON_DIR" ] && ACT="$(actool_bin)"; then
    dd="${ACT%/usr/bin/actool}"   # → …/Contents/Developer, for DEVELOPER_DIR
    atmp="$(mktemp -d)"
    echo "Icon: compiling dynamic Liquid Glass Assets.car from $(basename "$ICON_DIR") via actool"
    # Require actool to exit 0 AND leave an Assets.car — so a crash/nonzero run
    # (e.g. the reported Xcode 26.5 .icon crash) that drops a partial file is
    # not shipped. Running it as an `if` condition keeps `set -e` from aborting.
    if DEVELOPER_DIR="$dd" "$ACT" "$ICON_DIR" \
        --compile "$atmp" \
        --app-icon AppIcon --include-all-app-icons \
        --output-partial-info-plist "$atmp/partial.plist" \
        --enable-on-demand-resources NO \
        --development-region en \
        --target-device mac \
        --minimum-deployment-target 26.0 \
        --platform macosx \
        --output-format human-readable-text >/dev/null 2>&1 \
        && [ -f "$atmp/Assets.car" ]; then
        cp "$atmp/Assets.car" "$RES/Assets.car"
        # Point macOS 26 at the named icon in Assets.car (dynamic appearances);
        # CFBundleIconFile (the .icns) stays the source for macOS < 26.
        /usr/libexec/PlistBuddy -c "Add :CFBundleIconName string AppIcon" "$APP/Contents/Info.plist" >/dev/null 2>&1 \
            || /usr/libexec/PlistBuddy -c "Set :CFBundleIconName AppIcon" "$APP/Contents/Info.plist" >/dev/null
        echo "Icon: dynamic Assets.car installed + CFBundleIconName set (macOS 26 Liquid Glass)"
    else
        echo "Icon: actool did not produce a usable Assets.car — shipping the static .icns only" >&2
    fi
else
    echo "Icon: no actool (Xcode) — static .icns only; build on macos-26 / Xcode 26 for the dynamic Liquid Glass icon."
fi

# 5. Guard against a non-self-contained bundle: no Homebrew/@rpath/libusb dylib.
if otool -L "$APP/Contents/MacOS/$BIN_NAME" \
    | tail -n +2 \
    | grep -Ei 'libusb|@rpath|/opt/homebrew|/usr/local/'; then
    echo "ERROR: bundle links a non-system dylib (above). Force static libusb." >&2
    exit 1
fi

# 6. Sign. Developer ID + hardened runtime when an identity is provided,
#    else ad-hoc — arm64 requires at least an ad-hoc signature to run.
ENTITLEMENTS="$HERE/LTBox.entitlements"
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --options runtime \
        --entitlements "$ENTITLEMENTS" --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
    codesign --force --entitlements "$ENTITLEMENTS" --sign - "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"

# 7. Package as .tar.gz for the Release (like the Linux artifacts). The ad-hoc
#    signature lives inside the bundle, so it survives tar; COPYFILE_DISABLE
#    keeps AppleDouble `._*` metadata files out of the archive.
COPYFILE_DISABLE=1 tar -C "$OUT_DIR" -czf "$TARBALL" "$APP_NAME.app"

echo "Built $APP [$(lipo -archs "$APP/Contents/MacOS/$BIN_NAME")]"
echo "Packaged $TARBALL  (version $VERSION)"
