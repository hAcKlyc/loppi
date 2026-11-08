// Acquire checksum-locked build inputs before they are extracted or bundled.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
export const modelLockBytes = readFileSync(join(root, 'config/models.lock.json'));
export const models = JSON.parse(modelLockBytes);
export const sha = bytes => createHash('sha256').update(bytes).digest('hex');

export function checked(path, spec) {
  if (!existsSync(path)) return false;
  const stat = lstatSync(path);
  return stat.isFile() && !stat.isSymbolicLink() && stat.size === spec.bytes && sha(readFileSync(path)) === spec.sha256;
}

function fetchAsset(path, spec) {
  if (checked(path, spec)) return path;
  mkdirSync(dirname(path), { recursive: true });
  // Download to a temporary sibling. Never accept a partial or stale cache hit.
  const temporary = `${path}.download-${process.pid}`;
  const headers = spec.accept ? ['--header', `Accept: ${spec.accept}`] : [];
  const curl = process.platform === 'win32' ? 'curl.exe' : 'curl';
  const result = spawnSync(curl, [
    '--fail', '--location', '--retry', '6', '--retry-delay', '15', '--retry-max-time', '150',
    '--silent', '--show-error', ...headers, spec.url, '--output', temporary,
  ], { stdio: 'inherit', windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Asset download failed (${result.status}): ${spec.url}`);
  if (!checked(temporary, spec)) throw new Error(`Asset checksum mismatch: ${spec.url}`);
  renameSync(temporary, path);
  return path;
}

export function prepareMacNativeArchive(native) {
  return fetchAsset(join(root, 'build/cache/native', native.sha256, native.archive), native);
}

export function prepareWindowsNativeArchive(native) {
  return fetchAsset(join(root, 'build/cache/windows', native.archive), native);
}

export function prepareModelCache() {
  const cache = join(root, 'build/cache/models', sha(modelLockBytes));
  const seen = new Set();
  if (models.schema !== 1 || !models.files.length) throw new Error('Invalid model lock');
  for (const asset of models.files) {
    if (!asset.path || asset.path.split('/').some(p => !p || p === '.' || p === '..') || asset.path.includes('\\') || seen.has(asset.path)) throw new Error('Invalid model asset path');
    seen.add(asset.path);
    fetchAsset(join(cache, asset.path), asset);
  }
  writeFileSync(join(cache, 'models.lock.json'), modelLockBytes);
  return cache;
}
