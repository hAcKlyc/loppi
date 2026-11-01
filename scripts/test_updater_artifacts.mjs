import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { createMacUpdateArchive, mergeManifests, platforms, signingEnv, verifyArtifact, writePlatformManifest } from './updater-artifacts.mjs';

test('Mac archive excludes AppleDouble entries and retains the actual App tree', { skip: process.platform !== 'darwin' }, () => {
  const root = mkdtempSync(join(tmpdir(), 'loppi-update-archive-'));
  try {
    const app = join(root, 'loppi.app'), contents = join(app, 'Contents');
    mkdirSync(contents, { recursive: true });
    writeFileSync(join(contents, 'Info.plist'), 'synthetic plist');
    writeFileSync(join(contents, 'ticket-fixture'), 'stapled ticket fixture bytes');
    execFileSync('/usr/bin/xattr', ['-w', 'com.loppi.updater-test', 'synthetic metadata', app]);
    execFileSync('/usr/bin/xattr', ['-w', 'com.loppi.updater-test', 'synthetic metadata', join(contents, 'Info.plist')]);
    const archive = join(root, 'update.app.tar.gz');
    createMacUpdateArchive(app, archive);
    const entries = execFileSync('/usr/bin/tar', ['-tzf', archive], { encoding: 'utf8' }).trim().split('\n');
    assert(entries.includes('loppi.app/Contents/Info.plist'));
    assert(entries.includes('loppi.app/Contents/ticket-fixture'));
    assert(entries.every(name => name.startsWith('loppi.app/') && !name.split('/').some(part => part.startsWith('._'))), 'every entry must have the App root; no AppleDouble root file');
    const extracted = join(root, 'extracted'); mkdirSync(extracted);
    // Match the native updater's removal of the outer App component.
    execFileSync('/usr/bin/tar', ['-xzf', archive, '--strip-components', '1', '-C', extracted]);
    assert.equal(readFileSync(join(extracted, 'Contents/ticket-fixture'), 'utf8'), 'stapled ticket fixture bytes');
    assert.equal(readFileSync(join(extracted, 'Contents/Info.plist'), 'utf8'), 'synthetic plist');
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('real CLI signatures, release version and all platforms are verified before generating GitHub JSON', () => {
  const root = mkdtempSync(join(tmpdir(), 'loppi-update-signing-'));
  try {
    const cli = resolve('node_modules/@tauri-apps/cli/tauri.js');
    const key = join(root, 'updater.key');
    execFileSync(process.execPath, [cli, 'signer', 'generate', '--ci', '--password', '', '-w', key], { stdio: 'pipe' });
    const pubkey = readFileSync(`${key}.pub`, 'utf8').trim();
    mkdirSync(join(root, 'src-tauri'));
    writeFileSync(join(root, 'src-tauri/tauri.conf.json'), JSON.stringify({ plugins: { updater: { pubkey } } }));
    symlinkSync(resolve('node_modules'), join(root, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');
    writeFileSync(join(root, '.env'), `TAURI_SIGNING_PRIVATE_KEY="${key.replaceAll('\\', '/')}"\nTAURI_SIGNING_PRIVATE_KEY_PASSWORD=""\n`);
    const env = signingEnv(root, true, { ...process.env, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' });
    const version = '1.2.3', source = 'a'.repeat(40);
    const options = { version, source, notes: 'Synthetic release', pubkey };
    const manifests = platforms.map((target, index) => {
      const directory = join(root, target); mkdirSync(directory);
      const artifact = join(directory, `loppi_${version}_${target}${index === 2 ? '-setup.exe' : '.app.tar.gz'}`);
      writeFileSync(artifact, `SYNTHETIC UPDATE ${target}`);
      writePlatformManifest({ root, artifact, version, source, targets: [target], env });
      return join(directory, 'updater.json');
    });
    const merged = mergeManifests(manifests, options);
    assert.deepEqual(Object.keys(merged.manifest.platforms), platforms);
    assert.equal(merged.uploads.length, 6);
    assert.match(merged.manifest.platforms['darwin-aarch64'].url, /^https:\/\/github.com\/hAcKlyc\/loppi\/releases\/download\/v1\.2\.3\//);
    assert.throws(() => mergeManifests(manifests.slice(0, 2), options), /both macOS/);
    assert.throws(() => mergeManifests([...manifests, manifests[0]], options), /duplicate/);
    assert.throws(() => mergeManifests(manifests, { ...options, version: '1.2.4' }), /version and source/);
    assert.throws(() => mergeManifests(manifests, { ...options, source: 'b'.repeat(40) }), /version and source/);
    const first = JSON.parse(readFileSync(manifests[0], 'utf8'));
    const file = join(root, platforms[0], first.artifact);
    const signature = readFileSync(`${file}.sig`, 'utf8').trim();
    assert.throws(() => verifyArtifact(readFileSync(file), signature, pubkey, '1.2.4'), /bind/);
    assert.throws(() => verifyArtifact(Buffer.from('tampered'), signature, pubkey, version), /verification/);
    first.platforms[platforms[0]].url = 'https://example.com/update.app.tar.gz';
    writeFileSync(manifests[0], JSON.stringify(first));
    assert.throws(() => mergeManifests(manifests, options), /URL or signature/);
    first.platforms[platforms[0]].url = merged.manifest.platforms[platforms[0]].url;
    writeFileSync(manifests[0], JSON.stringify(first));
    writeFileSync(file, 'modified after signing');
    assert.throws(() => mergeManifests(manifests, options), /checksum/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('missing signing credentials fail release setup and development builds need no key', () => {
  const root = mkdtempSync(join(tmpdir(), 'loppi-update-env-'));
  try {
    assert.throws(() => signingEnv(root, true, {}), /TAURI_SIGNING_PRIVATE_KEY/);
    assert.deepEqual(signingEnv(root, false, {}), {});
    writeFileSync(join(root, '.env'), 'TAURI_SIGNING_PRIVATE_KEY="synthetic-key"\n');
    assert.equal(signingEnv(root, true, {}).TAURI_SIGNING_PRIVATE_KEY, 'synthetic-key');
    assert.equal(signingEnv(root, true, { TAURI_SIGNING_PRIVATE_KEY: 'environment' }).TAURI_SIGNING_PRIVATE_KEY, 'environment');
  } finally { rmSync(root, { recursive: true, force: true }); }
});
