#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo "This script packages macOS only." >&2; exit 1; }
scripts/cargo.sh build --release --locked
bundle="$PWD/dist/StructureWeaver.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/structureweaver "$bundle/Contents/MacOS/structureweaver"
if [ -f design/brand@2x.png ]; then
  iconset="$PWD/dist/StructureWeaver.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" design/brand@2x.png --out "$iconset/icon_$size"x"$size.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" design/brand@2x.png --out "$iconset/icon_$size"x"$size@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/StructureWeaver.icns"
fi
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>structureweaver</string>
<key>CFBundleIconFile</key><string>StructureWeaver.icns</string>
<key>CFBundleIdentifier</key><string>dev.structureweaver.app</string>
<key>CFBundleName</key><string>StructureWeaver</string>
<key>CFBundleDisplayName</key><string>StructureWeaver</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>12.0</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$bundle"
printf '%s\n' "$bundle"
