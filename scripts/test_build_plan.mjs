import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { buildPlan, hostArchitecture } from './build.mjs';

const thinTargets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
for (const command of ['build', 'release']) {
  test(`${command} defaults to two separate thin targets`, () => {
    assert.deepEqual(buildPlan([command]), { command, targets: thinTargets });
  });
  for (const target of [...thinTargets, 'universal-apple-darwin']) {
    test(`${command} respects one explicit target: ${target}`, () => {
      assert.deepEqual(buildPlan([command, '--target', target]), { command, targets: [target] });
    });
  }
}
test('omitting command preserves release entry semantics', () => {
  assert.deepEqual(buildPlan([]), { command: 'release', targets: thinTargets });
});
test('bad arguments fail before any build or download', () => {
  for (const argv of [['dev'], ['build', '--target'], ['build', '--other', thinTargets[0]], ['release', '--target', 'linux'], ['build', '--target', thinTargets[0], '--target', thinTargets[1]]]) {
    assert.throws(() => buildPlan(argv));
  }
});
test('invalid default target configuration cannot silently skip or duplicate a package', () => {
  for (const targets of [undefined, [], [thinTargets[0], thinTargets[0]], ['unsupported']]) {
    assert.throws(() => buildPlan(['build'], { default_targets: targets, targets: { [thinTargets[0]]: {} } }));
  }
});

test('host architecture stays correct when Node runs as Intel on Apple Silicon', { skip: process.platform !== 'darwin' }, t => {
  const expected = hostArchitecture();
  const probe = `import {hostArchitecture} from ${JSON.stringify(new URL('./build.mjs', import.meta.url).href)}; console.log(JSON.stringify({node:process.arch,host:hostArchitecture()}));`;
  for (const node of new Set([process.execPath, '/usr/local/bin/node'])) {
    const result = spawnSync('/usr/bin/arch', ['-x86_64', node, '--input-type=module', '-e', probe], { encoding: 'utf8' });
    if (result.status !== 0) continue;
    const actual = JSON.parse(result.stdout);
    assert.equal(actual.node, 'x64');
    assert.equal(actual.host, expected);
    return;
  }
  t.skip('No executable Intel Node installed; no translation provisioning is required');
});
