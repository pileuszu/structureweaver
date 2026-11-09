#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
[ "$#" -eq 0 ] || { echo "Usage: $0" >&2; exit 2; }
[ "$(uname -s)" = Darwin ] || { echo "This script packages macOS only." >&2; exit 1; }
# Keep the binary's deployment target and bundle metadata in agreement.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
scripts/cargo.sh build --release --locked
version=$(awk '
  /^\[package\]/ { package = 1; next }
  /^\[/ { package = 0 }
  package && /^version[[:space:]]*=/ {
    sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit
  }
' Cargo.toml)
bundle_version=${version%%-*}
bundle_version=${bundle_version%%+*}
printf '%s\n' "$bundle_version" | awk -F. '
  NF != 3 { exit 1 }
  { for (i = 1; i <= NF; i++) if ($i !~ /^[0-9]+$/) exit 1 }
' || { echo "Cargo package version must have three numeric components." >&2; exit 1; }
binary="${CARGO_TARGET_DIR:-target}/release/structureweaver"
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
  binary="${CARGO_TARGET_DIR:-target}/$CARGO_BUILD_TARGET/release/structureweaver"
fi
minimum_os=$(xcrun vtool -show-build "$binary" | awk '
  function newer(candidate, current, a, b, i) {
    split(candidate, a, "."); split(current, b, ".")
    for (i = 1; i <= 3; i++) {
      if (a[i] + 0 != b[i] + 0) return a[i] + 0 > b[i] + 0
    }
    return 0
  }
  $1 == "minos" { if (maximum == "" || newer($2, maximum)) maximum = $2 }
  END { print maximum }
')
[ -n "$minimum_os" ] || { echo "Cannot determine the binary's minimum macOS version." >&2; exit 1; }
mkdir -p dist
stage=$(mktemp -d "$PWD/dist/.app-package.XXXXXX")
destination="$PWD/dist/StructureWeaver.app"
cleanup() {
  # A signal between the two renames must not discard the previous app.
  if { [ -e "$stage/previous.app" ] || [ -L "$stage/previous.app" ]; } &&
     ! { [ -e "$destination" ] || [ -L "$destination" ]; }; then
    mv "$stage/previous.app" "$destination" || return 1
  fi
  rm -rf "$stage"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
bundle="$stage/StructureWeaver.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$binary" "$bundle/Contents/MacOS/structureweaver"
mkdir -p "$bundle/Contents/Resources/licenses"
cp assets/fonts/OFL.txt "$bundle/Contents/Resources/licenses/Pretendard-OFL.txt"
if [ -f design/brand@2x.png ]; then
  iconset="$stage/StructureWeaver.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" design/brand@2x.png --out "$iconset/icon_$size"x"$size.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" design/brand@2x.png --out "$iconset/icon_$size"x"$size@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/StructureWeaver.icns"
fi
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>structureweaver</string>
<key>CFBundleIconFile</key><string>StructureWeaver.icns</string>
<key>CFBundleIdentifier</key><string>dev.structureweaver.app</string>
<key>CFBundleName</key><string>StructureWeaver</string>
<key>CFBundleDisplayName</key><string>StructureWeaver</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$bundle_version</string>
<key>CFBundleVersion</key><string>$bundle_version</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>$minimum_os</string>
</dict></plist>
PLIST
identity=${STRUCTUREWEAVER_SIGNING_IDENTITY:--}
if [ "$identity" = - ]; then
  codesign --force --sign - "$bundle"
else
  codesign --force --options runtime --timestamp --sign "$identity" "$bundle"
fi
codesign --verify --deep --strict "$bundle"
if [ -e "$destination" ] || [ -L "$destination" ]; then
  mv "$destination" "$stage/previous.app"
fi
if ! mv "$bundle" "$destination"; then
  if [ -e "$stage/previous.app" ] || [ -L "$stage/previous.app" ]; then
    mv "$stage/previous.app" "$destination"
  fi
  exit 1
fi
printf '%s\n' "$destination"
