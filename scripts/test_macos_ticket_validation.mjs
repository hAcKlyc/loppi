import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

// Exercise the real packaging entry point with disposable files and external
// tool substitutes. These tests make no signing, mounting or network requests.
const tool = String.raw`#!/usr/bin/env node
const fs = require('node:fs'), path = require('node:path');
const command = path.basename(process.argv[1]), args = process.argv.slice(2);
const root = process.env.TEST_MAC_PACKAGE_ROOT;
fs.appendFileSync(path.join(root, 'calls.jsonl'), JSON.stringify({ command, args }) + '\n');
if (command === 'xcrun' && args[0] === 'stapler' && args[1] === 'validate') {
  const kind = args[2].endsWith('.dmg') ? 'dmg' : 'app';
  const counter = path.join(root, kind + '-attempts');
  const attempt = fs.existsSync(counter) ? Number(fs.readFileSync(counter, 'utf8')) + 1 : 1;
  fs.writeFileSync(counter, String(attempt));
  const mode = process.env.TEST_MAC_PACKAGE_MODE;
  const failed = (mode === 'recover-both' && attempt === 1)
    || (mode === 'network-app' && kind === 'app')
    || (mode === 'network-dmg' && kind === 'dmg');
  if (failed) {
    console.error('Error Domain=NSURLErrorDomain Code=-1005 "The network connection was lost."');
    process.exit(68);
  }
  if (kind === 'app' && mode === 'invalid-ticket') {
    console.error('The validate action failed! Error 68. Invalid ticket.');
    process.exit(68);
  }
  if (kind === 'app' && mode === 'other-error') {
    console.error('Error Domain=NSURLErrorDomain Code=-1005; permanent failure 65.');
    process.exit(65);
  }
  console.log('The validate action worked!');
} else if (command === 'ditto') {
  if (args[0] === '-c') fs.writeFileSync(args.at(-1), 'synthetic archive');
  else fs.cpSync(args[0], args[1], { recursive: true });
} else if (command === 'hdiutil') {
  if (args[0] === 'create') fs.writeFileSync(args.at(-1), 'synthetic image');
  if (args[0] === 'attach') {
    const mount = args[args.indexOf('-mountpoint') + 1];
    fs.mkdirSync(mount, { recursive: true });
    fs.cpSync(process.env.TEST_MAC_PACKAGE_APP, path.join(mount, 'loppi.app'), { recursive: true });
    fs.symlinkSync('/Applications', path.join(mount, 'Applications'));
  }
  if (args[0] === 'detach') fs.rmSync(args.at(-1), { recursive: true, force: true });
} else if (command === 'lipo') {
  console.log('test-only-architecture');
}
`;

const cases = [
  { mode: 'ok', exit: 0, app: 1, dmg: 1, sleeps: 0 },
  { mode: 'recover-both', exit: 0, app: 2, dmg: 2, sleeps: 2 },
  { mode: 'invalid-ticket', exit: 68, app: 1, dmg: 0, sleeps: 0 },
  { mode: 'other-error', exit: 65, app: 1, dmg: 0, sleeps: 0 },
  { mode: 'network-app', exit: 68, app: 3, dmg: 0, sleeps: 2 },
  { mode: 'network-dmg', exit: 68, app: 1, dmg: 3, sleeps: 2 },
];
for (const scenario of cases) {
  test(`Mac ticket validation: ${scenario.mode}`, { skip: process.platform !== 'darwin' && 'macOS packaging uses zsh and Apple tools' }, () => {
    const root = mkdtempSync(join(tmpdir(), 'loppi-ticket-validation-'));
    try {
      const bin = join(root, 'bin');
      const app = join(root, '应用 with spaces', 'loppi.app');
      const core = join(app, 'Contents/Library/LoginItems/LoppiCore.app/Contents/MacOS/loppi-core');
      const temporary = join(root, 'temporary');
      mkdirSync(bin, { recursive: true });
      mkdirSync(dirname(core), { recursive: true });
      mkdirSync(temporary);
      writeFileSync(core, 'synthetic Core; never executed');
      for (const name of ['codesign', 'xcrun', 'ditto', 'hdiutil', 'lipo', 'spctl', 'sleep']) {
        writeFileSync(join(bin, name), tool, { mode: 0o755 });
      }
      const result = spawnSync('/bin/zsh', ['scripts/package_macos.sh', app, join(root, 'output with spaces', 'loppi.dmg')], {
        cwd: resolve('.'), encoding: 'utf8', timeout: 15000,
        env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, TMPDIR: temporary,
          APPLE_SIGNING_IDENTITY: 'Synthetic test identity', APPLE_NOTARY_PROFILE: 'synthetic-test-profile',
          TEST_MAC_PACKAGE_ROOT: root, TEST_MAC_PACKAGE_APP: app, TEST_MAC_PACKAGE_MODE: scenario.mode },
      });
      assert.equal(result.status, scenario.exit, result.stderr);
      const calls = readFileSync(join(root, 'calls.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
      const validations = calls.filter(c => c.command === 'xcrun' && c.args[0] === 'stapler' && c.args[1] === 'validate');
      assert.equal(validations.filter(c => c.args[2].endsWith('.app')).length, scenario.app);
      assert.equal(validations.filter(c => c.args[2].endsWith('.dmg')).length, scenario.dmg);
      assert.equal(calls.filter(c => c.command === 'sleep').length, scenario.sleeps);
      assert.equal(calls.some(c => c.command === 'spctl'), scenario.exit === 0, 'Gatekeeper follows successful ticket validation');
      assert.equal(calls.some(c => c.command === 'hdiutil' && c.args[0] === 'create'), scenario.dmg > 0, 'A rejected App cannot advance to image creation');
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
