// Complete, reproducible release assets. Remote mutations are explicit modes.
import { readFileSync, writeFileSync, mkdirSync, copyFileSync, mkdtempSync, rmSync } from 'node:fs';
import { join, resolve, dirname, basename } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { mergeManifests, repository, sha256 } from './updater-artifacts.mjs';
import { releaseMetadata } from './release_metadata.mjs';

export function prepareRelease(root, notesFile, manifests) {
  const version = JSON.parse(readFileSync(join(root, 'package.json'))).version;
  const metadata = releaseMetadata(root, `v${version}`);
  const pubkey = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'))).plugins.updater.pubkey;
  const notes = readFileSync(notesFile, 'utf8').trim();
  if (!notes) throw new Error('Release notes cannot be empty');
  const merged = mergeManifests(manifests, { ...metadata, notes, pubkey });
  const destination = join(root, 'build/releases', version, 'github');
  mkdirSync(destination, { recursive: true });
  const assets = new Map();
  const add = (file, name = basename(file)) => {
    if (!/^[A-Za-z0-9._-]+$/.test(name) || assets.has(name)) throw new Error(`Invalid or duplicate public asset: ${name}`);
    const target = join(destination, name);
    if (resolve(file) !== target) copyFileSync(file, target);
    const bytes = readFileSync(target);
    assets.set(name, { name, path: target, size: bytes.length, sha256: sha256(bytes) });
  };
  for (const file of merged.uploads) add(file);
  for (const path of manifests) {
    const item = JSON.parse(readFileSync(path, 'utf8'));
    const reportPath = join(dirname(path), 'release-info.json');
    const report = JSON.parse(readFileSync(reportPath, 'utf8'));
    if (report.version !== version || report.source !== metadata.source || report.dirty !== false || report.updater?.sha256 !== item.sha256 || report.updater?.artifact !== item.artifact) throw new Error('Release report must describe this clean source and exact updater artifact');
    const keys = Object.keys(item.platforms).sort().join(',');
    const platform = { 'darwin-aarch64': 'macos-arm64', 'darwin-x86_64': 'macos-x86_64', 'darwin-aarch64,darwin-x86_64': 'macos-universal2', 'windows-x86_64': 'windows-x64' }[keys];
    if (!platform) throw new Error('Unknown release report platform');
    add(reportPath, `loppi_${version}_${platform}.release-info.json`);
    if (platform.startsWith('macos-')) {
      const name = `loppi_${version}_${platform}.dmg`;
      const file = join(dirname(path), name);
      const bytes = readFileSync(file);
      if (report.dmg !== name || report.dmg_bytes !== bytes.length || report.dmg_sha256 !== sha256(bytes)) throw new Error('Mac installer does not match the release report');
      add(file);
    }
  }
  const latest = join(destination, 'latest.json');
  writeFileSync(latest, `${JSON.stringify(merged.manifest, null, 2)}\n`);
  // Retain the original local output path for existing preparation commands.
  copyFileSync(latest, join(root, 'build/releases', version, 'latest.json'));
  add(latest);
  const checksums = join(destination, 'SHA256SUMS');
  writeFileSync(checksums, [...assets.values()].sort((a,b) => a.name.localeCompare(b.name)).map(a => `${a.sha256}  ${a.name}\n`).join(''));
  add(checksums);
  return { ...metadata, destination, assets: [...assets.values()], manifest: merged.manifest };
}

export function githubClient(root, execute = execFileSync) {
  const gh = args => execute('gh', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  const api = path => JSON.parse(gh(['api', `repos/${repository}/${path}`]));
  const optional = operation => { try { return operation(); } catch (error) { if (String(error.stderr).includes('(HTTP 404)')) return null; throw error; } };
  const post = (path, body) => JSON.parse(execute('gh', ['api', '--method', 'POST', `repos/${repository}/${path}`, '--input', '-'], { cwd: root, input: JSON.stringify(body), encoding: 'utf8', stdio: ['pipe','pipe','pipe'] }));
  // Lists include authorized drafts. Filter each page in gh so unrelated release
  // bodies never fill the captured stdout buffer; read afresh after mutations.
  const findRelease = tag => {
    const query = `.[] | select(.tag_name == ${JSON.stringify(tag)}) | {id,tag_name,target_commitish,draft,prerelease,assets:[.assets[] | {name,size,state,digest}]}`;
    const output = gh(['api', '--paginate', `repos/${repository}/releases?per_page=100`, '--jq', query]).trim();
    return output ? JSON.parse(output) : null;
  };
  const tagSource = tag => {
    let object = api(`git/ref/tags/${tag}`).object;
    for (let depth=0; object.type==='tag' && depth<10; depth++) object=api(`git/tags/${object.sha}`).object;
    if(object.type!=='commit')throw new Error('Version tag must resolve to a commit');
    return object.sha;
  };
  return {
    release: tag => { const release = findRelease(tag); if (!release) throw new Error(`Release not found: ${tag}`); return release; },
    maybeRelease: findRelease,
    source: tagSource,
    maybeSource: tag => optional(() => tagSource(tag)),
    isPublic: () => JSON.parse(gh(['api', `repos/${repository}`])).private === false,
    latest: () => api('releases/latest'),
    upload: (tag, files) => gh(['release', 'upload', tag, ...files, '--repo', repository]),
    publish: tag => gh(['release', 'edit', tag, '--repo', repository, '--draft=false', '--latest']),
    createTag: (tag, source, notes) => {
      const object = post('git/tags', { tag, object: source, type: 'commit', message: notes });
      post('git/refs', { ref: `refs/tags/${tag}`, sha: object.sha });
    },
    createDraft: (tag, source, notesFile) => gh(['release', 'create', tag, '--repo', repository, '--verify-tag', '--target', source, '--draft', '--title', tag, '--notes-file', notesFile]),
    waitForVerification: async source => {
      let run;
      for (let attempt=0; attempt<12; attempt++) {
        const runs = JSON.parse(gh(['run','list','--repo',repository,'--workflow','Verify','--branch','main','--event','push','--commit',source,'--limit','100','--json','databaseId,headSha,status,conclusion']));
        run = runs.find(candidate => candidate.headSha === source);
        if (run) break;
        await new Promise(resolve => setTimeout(resolve,5000));
      }
      if (!run) throw new Error('No Verify run exists for this exact main commit');
      if(run.status==='completed' && run.conclusion!=='success')throw new Error('Verify failed or was cancelled for this main commit');
      // Watching may print progress for an hour. Stream it rather than keeping
      // an ever-growing captured buffer that can abort an otherwise healthy CI.
      execute('gh',['run','watch',String(run.databaseId),'--repo',repository,'--exit-status'],{cwd:root,stdio:'inherit'});
      const result=JSON.parse(gh(['run','view',String(run.databaseId),'--repo',repository,'--json','headSha,conclusion']));
      if(result.headSha!==source||result.conclusion!=='success')throw new Error('Verify did not succeed for the exact main commit');
    },
    bytes: (tag, asset) => {
      const temporary = mkdtempSync(join(tmpdir(), 'loppi-release-check-'));
      try {
        gh(['release', 'download', tag, '--repo', repository, '--pattern', asset.name, '--dir', temporary]);
        return readFileSync(join(temporary, asset.name));
      } finally { rmSync(temporary, { recursive: true, force: true }); }
    },
  };
}

function verifyAsset(expected, actual, prepared, client) {
  if (actual.state !== 'uploaded' || actual.size !== expected.size) throw new Error(`Remote asset is incomplete: ${expected.name}`);
  const digest = actual.digest ?? `sha256:${sha256(client.bytes(prepared.tag, actual))}`;
  if (digest !== `sha256:${expected.sha256}`) throw new Error(`Remote asset differs: ${expected.name}`);
}

export function verifyRemote(prepared, client, { draft, allowMissing = false } = {}) {
  const release = client.release(prepared.tag);
  if (release.tag_name !== prepared.tag || client.source(prepared.tag) !== prepared.source) throw new Error('Release tag must resolve to the artifact source commit');
  if (draft !== undefined && release.draft !== draft) throw new Error(draft ? 'An existing draft release is required' : 'Release has not been published');
  if (release.prerelease) throw new Error('Stable updater releases cannot be prereleases');
  const expected = new Map(prepared.assets.map(a => [a.name, a]));
  const seen = new Set();
  for (const actual of release.assets) {
    if (!expected.has(actual.name) || seen.has(actual.name)) throw new Error(`Unexpected remote asset: ${actual.name}`);
    verifyAsset(expected.get(actual.name), actual, prepared, client);
    seen.add(actual.name);
  }
  const missing = prepared.assets.filter(a => !seen.has(a.name));
  if (!allowMissing && missing.length) throw new Error(`Release assets missing: ${missing.map(a => a.name).join(', ')}`);
  return missing;
}

export function uploadDraft(prepared, client) {
  const missing = verifyRemote(prepared, client, { draft: true, allowMissing: true });
  // Retries reuse matching bytes; never replace a published or changed asset.
  if (missing.length) client.upload(prepared.tag, missing.map(a => a.path));
  verifyRemote(prepared, client, { draft: true });
}

export function requireClean(root) {
  if (execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).trim()) throw new Error('Remote release changes require a clean committed checkout');
}

export async function verifyPublished(prepared, client, request = fetch) {
  verifyRemote(prepared, client, { draft: false });
  if (!client.isPublic()) throw new Error('Anonymous updates require a public repository; repository visibility was not changed');
  if (client.latest().tag_name !== prepared.tag) throw new Error('Published release is not the repository Latest release');
  const response = await request(`https://github.com/${repository}/releases/latest/download/latest.json`, { signal: AbortSignal.timeout(30_000) });
  if (!response.ok) throw new Error(`Anonymous latest.json is unavailable (${response.status}); rerun verification after propagation`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const expected = prepared.assets.find(a => a.name === 'latest.json');
  if (sha256(bytes) !== expected.sha256) throw new Error('Anonymous latest.json differs from the validated release');
}

export async function publishRelease(prepared, client, request = fetch) {
  verifyRemote(prepared, client);
  if (!client.isPublic()) throw new Error('Anonymous updates require a public repository; repository visibility was not changed');
  if (client.release(prepared.tag).draft) client.publish(prepared.tag);
  await verifyPublished(prepared, client, request);
}

export async function main(argv, root = resolve(dirname(fileURLToPath(import.meta.url)), '..')) {
  const mode = argv[0]?.startsWith('--') ? argv.shift() : '--prepare';
  if (!['--prepare', '--upload-draft', '--verify-draft', '--publish', '--verify-published'].includes(mode)) throw new Error('Unknown release mode');
  const [notesFile, ...manifests] = argv;
  if (!notesFile || !manifests.length) throw new Error('Usage: node scripts/publish_update.mjs [--prepare|--upload-draft|--verify-draft|--publish|--verify-published] <notes.txt> <platform/updater.json> ...');
  const prepared = prepareRelease(root, notesFile, manifests);
  console.log(`Validated complete release assets: ${prepared.destination}`);
  if (mode === '--prepare') return;
  const client = githubClient(root);
  if (['--upload-draft', '--publish'].includes(mode)) requireClean(root);
  if (mode === '--upload-draft') uploadDraft(prepared, client);
  if (mode === '--verify-draft') verifyRemote(prepared, client, { draft: true });
  if (mode === '--publish') await publishRelease(prepared, client);
  if (mode === '--verify-published') await verifyPublished(prepared, client);
  console.log(`${prepared.tag}: ${mode.slice(2)} passed`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
