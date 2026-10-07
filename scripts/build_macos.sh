#!/bin/zsh
set -euo pipefail

hear_root=${0:A:h:h}
cd "$hear_root"

# Native dependencies must target the same minimum OS as the app bundle.
export MACOSX_DEPLOYMENT_TARGET=13.0

hear_sign_identity=${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -1)}
if [[ -z "$hear_sign_identity" ]]; then
  print -u2 'A Developer ID Application signing identity is required.'
  exit 1
fi

cargo build --release -p hear-core -p hear-cli -p hear-worker
mkdir -p build/macos/HearCore.app/Contents/MacOS
cp target/release/hear-core build/macos/HearCore.app/Contents/MacOS/hear-core
cp target/release/hear-worker build/macos/HearCore.app/Contents/MacOS/hear-worker
cp src-tauri/native/HearCore-Info.plist build/macos/HearCore.app/Contents/Info.plist
cp target/release/hear build/macos/hear
npm run tauri build -- --bundles app

hear_app=target/release/bundle/macos/hear.app
hear_helper="$hear_app/Contents/Library/LoginItems/HearCore.app"
codesign --force --options runtime --timestamp --sign "$hear_sign_identity" "$hear_helper/Contents/MacOS/hear-worker"
codesign --force --options runtime --timestamp --entitlements src-tauri/native/HearCore.entitlements --sign "$hear_sign_identity" "$hear_helper"
codesign --force --options runtime --timestamp --sign "$hear_sign_identity" "$hear_app/Contents/MacOS/hear"
codesign --force --options runtime --timestamp --entitlements src-tauri/native/HearCore.entitlements --sign "$hear_sign_identity" "$hear_app"
codesign --verify --deep --strict --verbose=2 "$hear_app"
print "$hear_app"
