#!/bin/sh
# Create a drag-to-Applications installer. No Finder automation is required.
set -eu
cd "$(dirname "$0")/.."
[ "$#" -le 1 ] || { echo "Usage: $0 [--skip-build]" >&2; exit 2; }
[ "$(uname -s)" = Darwin ] || { echo "This script packages macOS only." >&2; exit 1; }
case "$#:${1:-}" in
  0:) scripts/package-macos.sh ;;
  1:--skip-build) ;;
  *) echo "Usage: $0 [--skip-build]" >&2; exit 2 ;;
esac
bundle="$PWD/dist/StructureWeaver.app"
codesign --verify --deep --strict "$bundle"
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$bundle/Contents/Info.plist")
minimum_os=$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$bundle/Contents/Info.plist")
binary="$bundle/Contents/MacOS/structureweaver"
architecture=$(lipo -archs "$binary")
case "$architecture" in
  arm64|x86_64) ;;
  'x86_64 arm64'|'arm64 x86_64') architecture=universal ;;
  *) echo "Unsupported macOS architecture: $architecture" >&2; exit 1 ;;
esac
# Validate values read from the bundle before using them in paths.
printf '%s\n' "$version" | awk -F. '
  NF != 3 { exit 1 }
  { for (i = 1; i <= NF; i++) if ($i !~ /^[0-9]+$/) exit 1 }
' || { echo "Invalid bundle version." >&2; exit 1; }
# A universal bundle must declare a minimum OS that satisfies every slice.
# This also protects --skip-build from packaging stale or inconsistent metadata.
xcrun vtool -show-build "$binary" | awk -v declared="$minimum_os" '
  function newer(candidate, current, a, b, i) {
    split(candidate, a, "."); split(current, b, ".")
    for (i = 1; i <= 3; i++) {
      if (a[i] + 0 != b[i] + 0) return a[i] + 0 > b[i] + 0
    }
    return 0
  }
  $1 == "minos" { found = 1; if (newer($2, declared)) mismatch = 1 }
  END { exit !found || mismatch }
' || { echo "Bundle minimum macOS version is lower than the binary requires, or cannot be verified." >&2; exit 1; }
stage=$(mktemp -d "$PWD/dist/.dmg-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
payload="$stage/payload"
mkdir "$payload"
ditto "$bundle" "$payload/StructureWeaver.app"
ln -s /Applications "$payload/Applications"
identity=${STRUCTUREWEAVER_SIGNING_IDENTITY:--}
signature=$(codesign -dv "$bundle" 2>&1)
case "$signature" in
  *Signature=adhoc*) signing_note='This local preview is ad hoc signed and has not been notarized by Apple.' ;;
  *) signing_note='Code signing is present. This script does not submit the app for Apple notarization.' ;;
esac
cat > "$payload/Install.txt" <<INSTALL
StructureWeaver $version — macOS $minimum_os or later ($architecture)

설치: StructureWeaver.app을 옆의 Applications 폴더로 끌어 놓은 뒤 실행하세요.
Install: Drag StructureWeaver.app into Applications, then launch it there.

Excel 설치 없이 Rust 자체 렌더러로 파일을 표시합니다.
구조 분석에는 Llama.app의 로컬 API와 이미지 입력을 지원하는 모델이 필요합니다.
Rendering does not require Excel. Analysis uses a local vision model in Llama.app.

$signing_note
https://github.com/pileuszu/structureweaver
INSTALL
name="StructureWeaver-$version-macos-$architecture"
for artifact in "dist/$name.dmg" "dist/$name.dmg.sha256"; do
  [ ! -d "$artifact" ] || { echo "Release output is a directory: $artifact" >&2; exit 1; }
done
hdiutil create -volname "StructureWeaver $version" -srcfolder "$payload" \
  -fs HFS+ -format UDZO -imagekey zlib-level=9 -nospotlight "$stage/$name.dmg"
if [ "$identity" != - ]; then
  codesign --force --timestamp --sign "$identity" "$stage/$name.dmg"
fi
hdiutil verify "$stage/$name.dmg"
(cd "$stage" && shasum -a 256 "$name.dmg" > "$name.dmg.sha256")
# Existing release artifacts are replaced only after creation and verification succeed.
mv -f "$stage/$name.dmg" "dist/$name.dmg"
mv -f "$stage/$name.dmg.sha256" "dist/$name.dmg.sha256"
printf '%s\n' "$PWD/dist/$name.dmg" "$PWD/dist/$name.dmg.sha256"
