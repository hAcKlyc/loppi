const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs/promises');
const path = require('node:path');
const sharp = require(process.argv[2] || 'sharp');
const root = path.resolve(__dirname, '../src/assets/rabbit');
const manifest = require('../src/assets/rabbit/manifest.json');
const pixels = file => sharp(file).ensureAlpha().raw().toBuffer();
test('every shared atlas frame matches its standalone fallback and fits the cell', async () => {
  const atlas = path.join(root, 'sprite-atlas.png');
  const meta = await sharp(atlas).metadata();
  assert.equal(meta.width, manifest.cell.width * 4);
  assert.equal(meta.height, manifest.cell.height * Math.ceil(Object.keys(manifest.frames).length / 4));
  assert.equal(meta.hasAlpha, true);
  for (const [name, frame] of Object.entries(manifest.frames)) {
    assert.equal(frame.width, manifest.cell.width);
    assert.equal(frame.height, manifest.cell.height);
    const slice = await sharp(atlas).extract({ left: frame.x, top: frame.y, width: frame.width, height: frame.height }).ensureAlpha().raw().toBuffer();
    assert.deepEqual(slice, await pixels(path.join(root, 'poses', `${name}.png`)), name);
  }
  for (const clip of Object.values(manifest.clips)) for (const [name, delay] of clip.frames) {
    assert.ok(manifest.frames[name], name);
    assert.ok(delay > 0);
  }
});
test('awake loop visibly breathes in pixel steps while retaining upright ears and planted feet', async () => {
  const size = manifest.cell.width;
  const base = await pixels(path.join(root, 'poses/listening.png'));
  const runs = [];
  for (let x = 0; x < size; x++) {
    if (base[(20 * size + x) * 4 + 3] < 128) continue;
    if (!runs.length || x !== runs.at(-1).at(-1) + 1) runs.push([]);
    runs.at(-1).push(x);
  }
  assert.equal(runs.length, 2, 'two separate upright ears');
  const blink = await pixels(path.join(root, 'poses/blink.png'));
  assert.deepEqual(blink.subarray(0, size * 43 * 4), base.subarray(0, size * 43 * 4), 'blink retains upright ears');
  for (const [name, shift] of [['listening-breath', 2], ['listening-inhale', 4]]) {
    const frame = await pixels(path.join(root, 'poses', `${name}.png`));
    assert.deepEqual(frame.subarray(0, size * 43 * 4), base.subarray(size * shift * 4, size * (43 + shift) * 4), 'head/ears move as one pixel shape');
    assert.deepEqual(frame.subarray(size * 86 * 4), base.subarray(size * 86 * 4), 'feet stay planted');
    assert.notDeepEqual(frame, base, 'animation frame must change');
  }
});
test('sleep loop and replies retain the approved lying pose; transitions never reuse legacy waiting art', async () => {
  const size = manifest.cell.width;
  const asleep = await pixels(path.join(root, 'poses/sleeping.png'));
  const breath = await pixels(path.join(root, 'poses/sleeping-inhale.png'));
  const bounds = [];
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (asleep[(y * size + x) * 4 + 3] >= 128) bounds.push([x, y]);
  const width = Math.max(...bounds.map(p => p[0])) - Math.min(...bounds.map(p => p[0])) + 1;
  const height = Math.max(...bounds.map(p => p[1])) - Math.min(...bounds.map(p => p[1])) + 1;
  assert.ok(width > height, 'lying silhouette');
  for (let y = 0; y < size; y++) {
    const offset = y * size * 4;
    assert.deepEqual(breath.subarray(offset, offset + 64 * 4), asleep.subarray(offset, offset + 64 * 4), 'sleeping face stays still');
  }
  assert.notDeepEqual(breath, asleep, 'sleeping torso breathes');
  for (const name of ['listening', 'sleeping', 'curious', 'nod', 'stretch', 'pause', 'sleeping-response']) {
    for (const [frame] of manifest.clips[name].frames) assert.ok(!['waiting', 'partial'].includes(frame), `${name}: no legacy shape flash`);
  }
});
