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

hear_notary_args=()
if [[ -n "${APPLE_NOTARY_PROFILE:-}" ]]; then
  hear_notary_args=(--keychain-profile "$APPLE_NOTARY_PROFILE")
elif [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
  if [[ ! -r "$APPLE_API_KEY_PATH" ]]; then
    print -u2 'APPLE_API_KEY_PATH must point to a readable App Store Connect private key file.'
    exit 2
  fi
  hear_notary_args=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY")
  if [[ -n "${APPLE_API_ISSUER:-}" ]]; then
    hear_notary_args+=(--issuer "$APPLE_API_ISSUER")
  fi
else
  print -u2 "Signed disk image: $hear_dmg"
  print -u2 'Notarization is pending. Configure APPLE_NOTARY_PROFILE or APPLE_API_KEY + APPLE_API_KEY_PATH (and APPLE_API_ISSUER for a team key), then rerun before distribution.'
  exit 2
fi

xcrun notarytool submit "$hear_dmg" "${hear_notary_args[@]}" --wait
xcrun stapler staple "$hear_dmg"
xcrun stapler validate "$hear_dmg"
spctl -a -vv -t open --context context:primary-signature "$hear_dmg"
print "$hear_dmg"
