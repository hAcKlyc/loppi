// Release artifacts and metadata share one implementation on macOS and Windows.
import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { parseEnv } from 'node:util';
import { execFileSync } from 'node:child_process';

export const repository = 'hAcKlyc/loppi';
export const platforms = ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'];
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function createMacUpdateArchive(app, archive) {
  // Tauri strips the first component of every tar entry. bsdtar's AppleDouble
  // root file (._loppi.app) would therefore unpack onto the extraction directory
  // itself. Keep real App files, including its stapled ticket, without sidecars.
  execFileSync('/usr/bin/tar', ['--no-mac-metadata', '--no-xattrs', '-czf', archive, '-C', dirname(app), basename(app)], { env: { ...process.env, COPYFILE_DISABLE: '1' }, stdio: 'pipe' });
}

export function signingEnv(root, required, base = process.env) {
  const env = { ...base };
  const path = join(root, '.env');
  const local = existsSync(path) ? parseEnv(readFileSync(path, 'utf8')) : {};
  for (const key of ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD']) {
    if (env[key] === undefined && local[key] !== undefined) env[key] = local[key];
  }
  if (env.TAURI_SIGNING_PRIVATE_KEY) {
    const file = env.TAURI_SIGNING_PRIVATE_KEY.replace(/^~(?=\/)/, homedir());
    if (existsSync(file)) env.TAURI_SIGNING_PRIVATE_KEY = readFileSync(file, 'utf8').trim();
  }
  if (required && !env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Release requires TAURI_SIGNING_PRIVATE_KEY; see docs/updates.md');
  return env;
}

// Minisign Ed25519 verification using Node's crypto (no additional runtime).
// Check both the artifact and the signed version before publishing a manifest.
export function verifyArtifact(bytes, signature, pubkey, version) {
  const publicLines = Buffer.from(pubkey, 'base64').toString('utf8').trim().split(/\r?\n/);
  const key = Buffer.from(publicLines.at(-1), 'base64');
  const lines = Buffer.from(signature, 'base64').toString('utf8').trim().split(/\r?\n/);
  if (key.length !== 42 || lines.length !== 4 || !lines[2].startsWith('trusted comment: ')) throw new Error('Invalid updater signature format');
  const sig = Buffer.from(lines[1], 'base64');
  const comment = lines[2].slice('trusted comment: '.length);
  if (sig.length !== 74 || !sig.subarray(2, 10).equals(key.subarray(2, 10))) throw new Error('Updater signing key does not match the application public key');
  const algorithm = sig.subarray(0, 2).toString();
  if (!['ED', 'Ed'].includes(algorithm)) throw new Error('Invalid updater signature algorithm');
  const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]), format: 'der', type: 'spki' });
  const message = algorithm === 'ED' ? createHash('blake2b512').update(bytes).digest() : bytes;
  if (!verify(null, message, publicKey, sig.subarray(10)) || !verify(null, Buffer.concat([sig.subarray(10), Buffer.from(comment)]), publicKey, Buffer.from(lines[3], 'base64'))) throw new Error('Updater signature verification failed');
  if (!comment.split('\t').includes(`version:${version}`)) throw new Error('Updater signature must bind the release version');
}

export function writePlatformManifest({ root, artifact, version, source, targets, env }) {
  execFileSync(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'signer', 'sign', '--app-version', version, artifact], { cwd: root, env, stdio: 'pipe' });
  const signature = readFileSync(`${artifact}.sig`, 'utf8').trim();
  const bytes = readFileSync(artifact);
  const pubkey = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'))).plugins.updater.pubkey;
  verifyArtifact(bytes, signature, pubkey, version);
  const entry = { url: `https://github.com/${repository}/releases/download/v${version}/${basename(artifact)}`, signature };
  const manifest = { version, source, platforms: Object.fromEntries(targets.map(target => [target, entry])), artifact: basename(artifact), bytes: bytes.length, sha256: sha256(bytes) };
  const path = join(dirname(artifact), 'updater.json');
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

export function mergeManifests(paths, { version, source, notes = '', pubkey, pubDate = new Date().toISOString() }) {
  const entries = {};
  const uploads = new Map();
  for (const path of paths) {
    const item = JSON.parse(readFileSync(path, 'utf8'));
    if (item.version !== version || item.source !== source) throw new Error('Updater manifests must share the release version and source commit');
    if (basename(item.artifact) !== item.artifact) throw new Error('Invalid updater artifact name');
    const artifact = resolve(dirname(path), item.artifact);
    const bytes = readFileSync(artifact);
    if (!statSync(artifact).isFile() || bytes.length !== item.bytes || sha256(bytes) !== item.sha256) throw new Error('Updater artifact checksum mismatch');
    const signature = readFileSync(`${artifact}.sig`, 'utf8').trim();
    verifyArtifact(bytes, signature, pubkey, version);
    for (const [target, value] of Object.entries(item.platforms)) {
      if (!platforms.includes(target) || entries[target]) throw new Error(`Unknown or duplicate updater target: ${target}`);
      const url = `https://github.com/${repository}/releases/download/v${version}/${item.artifact}`;
      if (value.url !== url || value.signature !== signature) throw new Error('Updater manifest URL or signature mismatch');
      if (target.startsWith('darwin-') ? !item.artifact.endsWith('.app.tar.gz') : !item.artifact.endsWith('-setup.exe')) throw new Error('Wrong updater artifact format');
      entries[target] = value;
    }
    for (const file of [artifact, `${artifact}.sig`]) {
      if (uploads.has(basename(file))) throw new Error('Duplicate release asset name');
      uploads.set(basename(file), file);
    }
  }
  if (platforms.some(target => !entries[target])) throw new Error('Release needs both macOS architectures and Windows x64');
  return { manifest: { version, notes, pub_date: pubDate, platforms: entries }, uploads: [...uploads.values()] };
}
