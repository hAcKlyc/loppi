#!/bin/zsh
set -euo pipefail

hear_root=${0:A:h:h}
cd "$hear_root"

# A running bundle must keep its on-disk code identity for Keychain access.
# Build outputs are disposable; launch the installed copy in /Applications.
node --input-type=module <<'JS'
import { execFileSync } from 'node:child_process';
const bundle = `${process.cwd()}/target/release/bundle/macos/hear.app/`;
const running = execFileSync('/bin/ps', ['-axo', 'comm='], { encoding: 'utf8' })
  .split('\n').some(path => path.trim().startsWith(bundle));
if (running) {
  console.error('hear is running from the build output. Quit hear before rebuilding, or run the installed copy in /Applications instead.');
  process.exit(2);
}
JS

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
# Final signing happens below, after all embedded executables are in place.
# Keep Tauri from signing/notarizing an intermediate app with release credentials.
env -u APPLE_SIGNING_IDENTITY -u APPLE_API_KEY -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH \
  -u APPLE_ID -u APPLE_PASSWORD npm run tauri build -- --bundles app

hear_app=target/release/bundle/macos/hear.app
hear_helper="$hear_app/Contents/Library/LoginItems/HearCore.app"
codesign --force --options runtime --timestamp --sign "$hear_sign_identity" "$hear_helper/Contents/MacOS/hear-worker"
codesign --force --options runtime --timestamp --entitlements src-tauri/native/HearCore.entitlements --sign "$hear_sign_identity" "$hear_helper"
codesign --force --options runtime --timestamp --sign "$hear_sign_identity" "$hear_app/Contents/MacOS/hear"
codesign --force --options runtime --timestamp --entitlements src-tauri/native/HearCore.entitlements --sign "$hear_sign_identity" "$hear_app"
codesign --verify --deep --strict --verbose=2 "$hear_app"
print "$hear_app"
