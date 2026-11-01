#!/usr/bin/env bash
# Build OxyBrush.app (universal: Apple Silicon + Intel) and a .dmg.
#
# Usage (on macOS, from the repo root, after building both targets):
#   cargo build --profile dist -p oxy-app --target aarch64-apple-darwin
#   cargo build --profile dist -p oxy-app --target x86_64-apple-darwin
#   packaging/macos/make_app.sh 0.1.0-alpha
#
# The app is ad-hoc signed (no Apple Developer ID yet), so Gatekeeper asks
# users to confirm the first launch; see docs/TESTING_GUIDE.md.
set -euo pipefail

VERSION="${1:?usage: make_app.sh VERSION}"
NUMERIC="${VERSION%%-*}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/dist"
WORK="$(mktemp -d)"
APP="$WORK/OxyBrush.app"

mkdir -p "$OUT" "$APP/Contents/MacOS" "$APP/Contents/Resources"

# Universal binary from both architectures.
lipo -create -output "$APP/Contents/MacOS/oxybrush" \
  "$ROOT/target/aarch64-apple-darwin/dist/oxybrush" \
  "$ROOT/target/x86_64-apple-darwin/dist/oxybrush"
lipo -info "$APP/Contents/MacOS/oxybrush"

# Icon: .iconset -> .icns
ICONSET="$WORK/oxybrush.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  cp "$ROOT/packaging/icons/oxybrush-$s.png" "$ICONSET/icon_${s}x${s}.png"
  cp "$ROOT/packaging/icons/oxybrush-$((s * 2)).png" "$ICONSET/icon_${s}x${s}@2x.png"
done
iconutil -c icns -o "$APP/Contents/Resources/oxybrush.icns" "$ICONSET"

sed -e "s/@VERSION@/$VERSION/g" -e "s/@NUMERIC_VERSION@/$NUMERIC/g" \
  "$ROOT/packaging/macos/Info.plist.in" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$APP/Contents/Resources/"

# Ad-hoc signature: required for Apple Silicon to run the binary at all.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

# Disk image with an Applications shortcut for drag-and-drop install.
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/README.md" "$STAGE/README.md"
DMG="$OUT/OxyBrush-$VERSION-macos-universal.dmg"
hdiutil create -volname "OxyBrush $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
hdiutil verify "$DMG"

# Keep the bundle for the smoke test.
rm -rf "$OUT/OxyBrush.app"
cp -R "$APP" "$OUT/OxyBrush.app"
echo "built $DMG"
