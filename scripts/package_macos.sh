#!/bin/zsh
set -euo pipefail

loppi_root=${0:A:h:h}
cd "$loppi_root"

loppi_identity=${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -1)}
if [[ -z "$loppi_identity" ]]; then
  print -u2 'A Developer ID Application signing identity is required.'
  exit 1
fi

if [[ $# -ne 2 ]]; then
  print -u2 'Usage: package_macos.sh /absolute/loppi.app /absolute/output.dmg'
  exit 2
fi
loppi_app="$1"
loppi_dmg="$2"
codesign --verify --deep --strict "$loppi_app"

loppi_version=$(node -p "require('./package.json').version")
loppi_work=$(mktemp -d "${TMPDIR:-/tmp/}loppi-dmg.XXXXXX")
loppi_mount="$loppi_work/mounted"
trap 'hdiutil detach "$loppi_mount" >/dev/null 2>&1 || true; rm -rf "$loppi_work"' EXIT

mkdir -p "$loppi_work/staging" "$loppi_mount" "${loppi_dmg:h}"
ditto "$loppi_app" "$loppi_work/staging/loppi.app"
ln -s /Applications "$loppi_work/staging/Applications"
hdiutil create -quiet -volname "loppi ${loppi_version}" -srcfolder "$loppi_work/staging" -format UDZO -ov "$loppi_dmg"
codesign --force --timestamp --sign "$loppi_identity" "$loppi_dmg"
codesign --verify --verbose=2 "$loppi_dmg"
hdiutil verify -quiet "$loppi_dmg"

hdiutil attach -quiet -nobrowse -readonly -mountpoint "$loppi_mount" "$loppi_dmg"
codesign --verify --deep --strict "$loppi_mount/loppi.app"
[[ -L "$loppi_mount/Applications" && "$(readlink "$loppi_mount/Applications")" == /Applications ]]
hdiutil detach -quiet "$loppi_mount"

loppi_notary_args=()
if [[ -n "${APPLE_NOTARY_PROFILE:-}" ]]; then
  loppi_notary_args=(--keychain-profile "$APPLE_NOTARY_PROFILE")
elif [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
  if [[ ! -r "$APPLE_API_KEY_PATH" ]]; then
    print -u2 'APPLE_API_KEY_PATH must point to a readable App Store Connect private key file.'
    exit 2
  fi
  loppi_notary_args=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY")
  if [[ -n "${APPLE_API_ISSUER:-}" ]]; then
    loppi_notary_args+=(--issuer "$APPLE_API_ISSUER")
  fi
else
  print -u2 "Signed disk image: $loppi_dmg"
  print -u2 'Notarization is pending. Configure APPLE_NOTARY_PROFILE or APPLE_API_KEY + APPLE_API_KEY_PATH (and APPLE_API_ISSUER for a team key), then rerun before distribution.'
  exit 2
fi

xcrun notarytool submit "$loppi_dmg" "${loppi_notary_args[@]}" --wait
xcrun stapler staple "$loppi_dmg"
xcrun stapler validate "$loppi_dmg"
spctl -a -vv -t open --context context:primary-signature "$loppi_dmg"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$loppi_mount" "$loppi_dmg"
codesign --verify --deep --strict "$loppi_mount/loppi.app"
# Check the bytes in the actual final image, including the single model copy.
diff -qr "$loppi_app" "$loppi_mount/loppi.app"
python3 scripts/smoke_model_preparation.py --bundle "$loppi_mount/loppi.app"
hdiutil detach -quiet "$loppi_mount"
print "$loppi_dmg"
