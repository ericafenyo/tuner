#!/usr/bin/env bash
# Build "Bass Tuner.app" in target/release/ (macOS only).
set -euo pipefail

cd "$(dirname "$0")/.."

NAME="Bass Tuner"
BIN="tuner"
ID="local.basstuner"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

OUT="target/release"
APP="$OUT/$NAME.app"
WORK="$OUT/app-build"

echo "Building release binary..."
cargo build --release --bin "$BIN"

echo "Rendering icon..."
rm -rf "$WORK" && mkdir -p "$WORK/AppIcon.iconset"
cargo run --quiet --release --example render_icon -- "$WORK/icon.tga" 1024
sips -s format png "$WORK/icon.tga" --out "$WORK/icon.png" >/dev/null
for size in 16 32 128 256 512; do
    sips -z $size $size "$WORK/icon.png" --out "$WORK/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$WORK/icon.png" --out "$WORK/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$WORK/AppIcon.iconset" -o "$WORK/AppIcon.icns"

echo "Assembling $APP..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$OUT/$BIN" "$APP/Contents/MacOS/$BIN"
cp "$WORK/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>                <string>$NAME</string>
    <key>CFBundleDisplayName</key>         <string>$NAME</string>
    <key>CFBundleIdentifier</key>          <string>$ID</string>
    <key>CFBundleExecutable</key>          <string>$BIN</string>
    <key>CFBundleIconFile</key>            <string>AppIcon</string>
    <key>CFBundlePackageType</key>         <string>APPL</string>
    <key>CFBundleVersion</key>             <string>$VERSION</string>
    <key>CFBundleShortVersionString</key>  <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>      <string>11.0</string>
    <key>NSHighResolutionCapable</key>     <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>Bass Tuner listens to your instrument to measure its pitch.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature so macOS ties the microphone permission to this bundle.
codesign --force --deep --sign - "$APP"
rm -rf "$WORK"

echo "Done: $APP"
