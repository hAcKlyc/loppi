#!/bin/zsh
set -euo pipefail

hear_root=${0:A:h:h}
cd "$hear_root"

hear_identity=${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -1)}
if [[ -z "$hear_identity" ]]; then
  print -u2 'A Developer ID Application signing identity is required.'
  exit 1
fi

hear_app="$hear_root/target/release/bundle/macos/hear.app"
codesign --verify --deep --strict "$hear_app"

hear_version=$(node -p "require('./package.json').version")
hear_arch=$(uname -m)
hear_dmg="$hear_root/target/release/bundle/dmg/hear_${hear_version}_${hear_arch}.dmg"
hear_work=$(mktemp -d "${TMPDIR:-/tmp/}hear-dmg.XXXXXX")
hear_mount="$hear_work/mounted"
trap 'hdiutil detach "$hear_mount" >/dev/null 2>&1 || true; rm -rf "$hear_work"' EXIT

mkdir -p "$hear_work/staging" "$hear_mount" "${hear_dmg:h}"
ditto "$hear_app" "$hear_work/staging/hear.app"
ln -s /Applications "$hear_work/staging/Applications"
hdiutil create -quiet -volname "hear ${hear_version}" -srcfolder "$hear_work/staging" -format UDZO -ov "$hear_dmg"
codesign --force --timestamp --sign "$hear_identity" "$hear_dmg"
codesign --verify --verbose=2 "$hear_dmg"
hdiutil verify -quiet "$hear_dmg"

hdiutil attach -quiet -nobrowse -readonly -mountpoint "$hear_mount" "$hear_dmg"
codesign --verify --deep --strict "$hear_mount/hear.app"
[[ -L "$hear_mount/Applications" && "$(readlink "$hear_mount/Applications")" == /Applications ]]
hdiutil detach -quiet "$hear_mount"

if [[ -z "${APPLE_NOTARY_PROFILE:-}" ]]; then
  print -u2 "Signed disk image: $hear_dmg"
  print -u2 'Notarization is pending. Set APPLE_NOTARY_PROFILE to a notarytool Keychain profile and rerun this script before distribution.'
  exit 2
fi

xcrun notarytool submit "$hear_dmg" --keychain-profile "$APPLE_NOTARY_PROFILE" --wait
xcrun stapler staple "$hear_dmg"
xcrun stapler validate "$hear_dmg"
spctl -a -vv -t open --context context:primary-signature "$hear_dmg"
print "$hear_dmg"
