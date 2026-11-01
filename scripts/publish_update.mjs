// Prepare a complete latest.json, optionally upload to an existing draft.
// Publishing the draft is a separate, explicit maintainer action.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { mergeManifests, repository } from './updater-artifacts.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
function main(argv) {
  const upload = argv[0] === '--upload-draft';
  if (upload) argv = argv.slice(1);
  const [notesFile, ...manifests] = argv;
  if (!notesFile || !manifests.length) throw new Error('Usage: node scripts/publish_update.mjs [--upload-draft] <release-notes.txt> <platform/updater.json> ...');
  const version = JSON.parse(readFileSync(join(root, 'package.json'))).version;
  const source = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  const pubkey = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'))).plugins.updater.pubkey;
  const { manifest, uploads } = mergeManifests(manifests, { version, source, notes: readFileSync(notesFile, 'utf8'), pubkey });
  const destination = join(root, 'build/releases', version);
  mkdirSync(destination, { recursive: true });
  const output = join(destination, 'latest.json');
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Validated all three platforms: ${output}`);
  if (!upload) return;
  const gh = args => execFileSync('gh', args, { cwd: root, encoding: 'utf8' });
  const release = JSON.parse(gh(['release', 'view', `v${version}`, '--repo', repository, '--json', 'isDraft,targetCommitish,tagName']));
  if (!release.isDraft) throw new Error('Only an existing draft release may receive update assets; published releases are immutable');
  const tagged = gh(['api', `repos/${repository}/commits/v${version}`, '--jq', '.sha']).trim();
  if (tagged !== source) throw new Error('Release tag must resolve to the artifact source commit');
  // Upload all versioned resources first; latest.json becomes visible together
  // with them only when the maintainer publishes the complete draft.
  gh(['release', 'upload', `v${version}`, ...uploads, output, '--repo', repository]);
  console.log('Update resources uploaded to draft. Publish only after platform acceptance.');
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
