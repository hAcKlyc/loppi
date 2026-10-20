#!/bin/zsh
set -euo pipefail

hear_root=${0:A:h}
cd "$hear_root"

# Parse only release settings as data; never execute .env as shell commands.
exec node --input-type=module <<'JS'
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { parseEnv } from 'node:util';

const env = { ...process.env };
const local = existsSync('.env') ? parseEnv(readFileSync('.env', 'utf8')) : {};
for (const key of [
  'APPLE_SIGNING_IDENTITY', 'APPLE_NOTARY_PROFILE',
  'APPLE_API_KEY', 'APPLE_API_ISSUER', 'APPLE_API_KEY_PATH',
]) {
  if (env[key] === undefined && local[key] !== undefined) env[key] = local[key];
}
if (env.APPLE_API_KEY_PATH) {
  env.APPLE_API_KEY_PATH = env.APPLE_API_KEY_PATH.replace(/^~(?=\/)/, homedir());
}
for (const script of ['scripts/build_macos.sh', 'scripts/package_macos.sh']) {
  const result = spawnSync('/bin/zsh', [script], { env, stdio: 'inherit' });
  if (result.error) {
    console.error(`Could not run ${script}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}
JS
