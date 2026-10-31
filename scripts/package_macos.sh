#!/bin/zsh
set -euo pipefail

loppi_root=${0:A:h:h}
cd "$loppi_root"

loppi_dev=0
if [[ "${1:-}" == --dev ]]; then
  loppi_dev=1
  shift
fi
if [[ $# -ne 2 ]]; then
  print -u2 'Usage: package_macos.sh [--dev] /absolute/loppi.app /absolute/output.dmg'
  exit 2
fi
loppi_app="$1"
loppi_dmg="$2"
if (( ! loppi_dev )); then
  loppi_identity=${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -1)}
  if [[ -z "$loppi_identity" ]]; then
    print -u2 'A Developer ID Application signing identity is required.'
    exit 1
  fi
  codesign --verify --deep --strict "$loppi_app"
fi

loppi_version=$(node -p "require('./package.json').version")
loppi_work=$(mktemp -d "${TMPDIR:-/tmp/}loppi-dmg.XXXXXX")
loppi_mount="$loppi_work/mounted"
trap 'hdiutil detach "$loppi_mount" >/dev/null 2>&1 || true; rm -rf "$loppi_work"' EXIT

mkdir -p "$loppi_work/staging" "$loppi_mount" "${loppi_dmg:h}"
ditto "$loppi_app" "$loppi_work/staging/loppi.app"
ln -s /Applications "$loppi_work/staging/Applications"
hdiutil create -quiet -volname "loppi ${loppi_version}" -srcfolder "$loppi_work/staging" -format UDZO -ov "$loppi_dmg"
if (( ! loppi_dev )); then
  codesign --force --timestamp --sign "$loppi_identity" "$loppi_dmg"
  codesign --verify --verbose=2 "$loppi_dmg"
fi
hdiutil verify -quiet "$loppi_dmg"

hdiutil attach -quiet -nobrowse -readonly -mountpoint "$loppi_mount" "$loppi_dmg"
if (( ! loppi_dev )); then codesign --verify --deep --strict "$loppi_mount/loppi.app"; fi
[[ -L "$loppi_mount/Applications" && "$(readlink "$loppi_mount/Applications")" == /Applications ]]
hdiutil detach -quiet "$loppi_mount"

if (( ! loppi_dev )); then
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
fi
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$loppi_mount" "$loppi_dmg"
if (( ! loppi_dev )); then codesign --verify --deep --strict "$loppi_mount/loppi.app"; fi
# Check the bytes in the actual final image, including the single model copy.
diff -qr "$loppi_app" "$loppi_mount/loppi.app"
# Execute only when this machine can run the mounted Core. Rosetta is optional;
# its presence permits translated checks, never a claim of Intel hardware QA.
loppi_core="$loppi_mount/loppi.app/Contents/Library/LoginItems/LoppiCore.app/Contents/MacOS/loppi-core"
loppi_archs=" $(lipo -archs "$loppi_core") "
if [[ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || true)" == 1 ]]; then
  loppi_host=arm64
else
  loppi_host=x86_64
fi
if [[ "$loppi_archs" == *" $loppi_host "* ]] || { [[ "$loppi_host" == arm64 && "$loppi_archs" == *" x86_64 "* ]] && /usr/bin/arch -x86_64 /usr/bin/true >/dev/null 2>&1; }; then
  python3 scripts/smoke_model_preparation.py --bundle "$loppi_mount/loppi.app"
else
  print 'Runtime check unavailable on this host; mounted bytes and architectures verified by the build.'
fi
hdiutil detach -quiet "$loppi_mount"
print "$loppi_dmg"
