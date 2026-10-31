import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';

const root = resolve(import.meta.dirname, '..');
const css = await readFile(resolve(root, 'src/tokens.css'), 'utf8');
const tokens = Object.fromEntries([...css.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(m => [m[1], m[2].trim()]));
function color(name, seen = new Set()) {
  assert(!seen.has(name), `Token cycle: ${name}`); seen.add(name);
  const raw = tokens[name]; assert(raw, `Undefined token: ${name}`);
  const alias = raw.match(/^var\(--([\w-]+)\)$/);
  return alias ? color(alias[1], seen) : raw;
}
function luminance(hex) {
  assert.match(hex, /^#[a-f\d]{6}$/i);
  return [0, 2, 4].map(i => parseInt(hex.slice(i + 1, i + 3), 16) / 255)
    .map(x => x <= .04045 ? x / 12.92 : ((x + .055) / 1.055) ** 2.4)
    .reduce((n, x, i) => n + x * [.2126, .7152, .0722][i], 0);
}
test('semantic text colors remain readable on their actual surfaces', () => {
  const pairs = [
    ['color-text', 'color-canvas'], ['color-text-secondary', 'color-surface'],
    ['color-text-muted', 'color-sidebar'],
    ['color-text-secondary', 'color-surface-selected'], ['color-text-secondary', 'color-surface-subtle'], ['color-placeholder', 'color-surface-subtle'],
    ['color-live-text', 'color-live-soft'], ['color-complete-text', 'color-complete-soft'],
    ['color-warning-text', 'color-warning-surface'], ['color-danger', 'color-danger-surface'],
    ['color-on-brand', 'color-brand'], ['entry-text', 'entry-surface'],
    ['entry-text-secondary', 'entry-menu'],
  ];
  for (const [text, surface] of pairs) {
    const a = luminance(color(text)), b = luminance(color(surface));
    const ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
    assert(ratio >= 4.5, `${text} on ${surface}: ${ratio.toFixed(2)} < 4.5`);
  }
});
test('source assets and exported token definitions agree', async () => {
  const exported = JSON.parse(await readFile(resolve(root, 'design/system-v1/tokens.json')));
  assert.deepEqual(exported.tokens, tokens);
  const approvedIcon = JSON.parse(await readFile(resolve(root, 'design/system-v1/source/product-icon.json')));
  for (const file of approvedIcon.files) {
    const bytes = await readFile(resolve(root, file.path));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), file.sha256, `Product ICON changed: ${file.path}`);
  }
  assert.deepEqual(await readFile(resolve(root, 'design/system-v1/icons/loppi.icns')), await readFile(resolve(root, 'src-tauri/icons/icon.icns')));
  assert.deepEqual(await readFile(resolve(root, 'design/system-v1/icons/app-square-1024.png')), await readFile(resolve(root, 'src/assets/rabbit/app-icon-square.png')));
  const svg = await readFile(resolve(root, 'src/assets/brand/wordmark.svg'), 'utf8');
  assert(!/<(?:text|image)\b/.test(svg), 'Wordmark must be real vector outlines');
  assert(svg.includes('#B65B42'), 'Approved deep warm-red dot is present');
  const atlas = JSON.parse(await readFile(resolve(root, 'src/assets/rabbit/manifest.json')));
  assert.equal(atlas.cell.width, 96);
  for (const pose of ['listening', 'sleeping', 'waiting', 'partial']) await access(resolve(root, `design/system-v1/rabbit/poses/${pose}.png`));
});
test('formal asset manifest hashes correspond to the shipped files', async () => {
  const manifest = JSON.parse(await readFile(resolve(root, 'design/system-v1/manifest.json')));
  assert(manifest.files.length >= 50);
  for (const file of manifest.files) {
    assert(!file.path.includes('..'));
    const bytes = await readFile(resolve(root, 'design/system-v1', file.path));
    assert.equal(bytes.length, file.bytes, file.path);
    assert.equal(createHash('sha256').update(bytes).digest('hex'), file.sha256, file.path);
  }
});
