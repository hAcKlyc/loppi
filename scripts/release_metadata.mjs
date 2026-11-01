// Portable preflight shared by release Actions and local release preparation.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

export function releaseMetadata(root, tag) {
  const read = path => readFileSync(join(root, path), 'utf8');
  const version = JSON.parse(read('package.json')).version;
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version) || tag !== `v${version}`) throw new Error('Release tag must match the stable application version (vX.Y.Z)');
  const lock = JSON.parse(read('package-lock.json'));
  const versions = [lock.version, lock.packages[''].version, JSON.parse(read('src-tauri/tauri.conf.json')).version,
    read('Cargo.toml').match(/\[workspace.package\]\s+version = "([^"]+)"/)?.[1],
    read('src-tauri/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1],
    read('src-tauri/native/LoppiCore-Info.plist').match(/<key>CFBundleShortVersionString<\/key>\s*<string>([^<]+)<\/string>/)?.[1]];
  for (const name of ['loppi-cli', 'loppi-core', 'loppi-desktop', 'loppi-protocol', 'loppi-worker']) {
    versions.push(read('Cargo.lock').match(new RegExp(`name = "${name}"\\r?\\nversion = "([^"]+)"`))?.[1]);
  }
  if (versions.some(value => value !== version)) throw new Error('Project version sources disagree');
  const git = args => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  const source = git(['rev-parse', 'HEAD']);
  const pubDate = git(['show', '-s', '--format=%cI', 'HEAD']);
  return { tag, version, source, pubDate };
}

export function releaseNotes(changelog, version) {
  const lines = changelog.split(/\r?\n/);
  const escaped = version.replaceAll('.', '\\.');
  const header = new RegExp(`^## (?:\\[${escaped}\\]|${escaped})(?:\\s|$)`);
  const first = lines.findIndex(line => header.test(line));
  if (first < 0 || !/\d{4}-\d{2}-\d{2}/.test(lines[first])) throw new Error('CHANGELOG needs a dated section for the release version');
  let end = first + 1;
  while (end < lines.length && !/^## |^---\s*$/.test(lines[end])) end++;
  const notes = lines.slice(first, end).join('\n').trim();
  if (!notes.includes('\n- ')) throw new Error('Release notes need user-facing changes');
  return notes + '\n';
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
    const [tag, notesPath] = process.argv.slice(2);
    if (!tag) throw new Error('Usage: node scripts/release_metadata.mjs <vX.Y.Z> [notes-output.txt]');
    const metadata = releaseMetadata(root, tag);
    const notes = releaseNotes(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'), metadata.version);
    if (notesPath) writeFileSync(notesPath, notes);
    console.log(JSON.stringify(metadata));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
