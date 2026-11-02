import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, access } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import sharp from 'sharp';

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
    ['color-live-text', 'color-canvas'], ['color-live-text', 'color-surface'], ['color-text-muted', 'color-surface-card-hover'],
    ['color-warning-text', 'color-warning-surface'], ['color-danger', 'color-danger-surface'],
    ['color-on-brand', 'color-brand'], ['entry-text', 'entry-surface'],
    ['entry-text-secondary', 'entry-menu'],
    ['entry-accent', 'entry-surface'], ['entry-accent', 'entry-action'], ['entry-accent', 'entry-action-hover'],
  ];
  for (const [text, surface] of pairs) {
    const a = luminance(color(text)), b = luminance(color(surface));
    const ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
    assert(ratio >= 4.5, `${text} on ${surface}: ${ratio.toFixed(2)} < 4.5`);
  }
});
test('switch fills or outlines retain visible thumb and surface contrast', () => {
  assert.notEqual(color('color-switch-on'), color('color-switch-off'));
  for (const track of ['color-switch-on', 'color-switch-off-border']) {
    for (const surface of ['color-surface', 'color-canvas']) {
      const a=luminance(color(track)), b=luminance(color(surface));
      const ratio=(Math.max(a,b)+.05)/(Math.min(a,b)+.05);
      assert(ratio>=3, `${track} against ${surface}: ${ratio.toFixed(2)} < 3`);
    }
  }
  const a=luminance(color('color-switch-off-border')), b=luminance(color('color-switch-off'));
  assert((Math.max(a,b)+.05)/(Math.min(a,b)+.05)>=3, 'Off track and thumb outlines remain distinct from the light fill');
});
test('source assets and exported token definitions agree', async () => {
  const exported = JSON.parse(await readFile(resolve(root, 'design/system-v1/tokens.json')));
  assert.deepEqual(exported.tokens, tokens);
  assert.deepEqual(Object.values(exported.groups).flat().sort(),Object.keys(tokens).sort(),'Every exported token belongs to one group');
  const approvedIcon = JSON.parse(await readFile(resolve(root, 'design/system-v1/source/product-icon.json')));
  for (const file of approvedIcon.files) {
    const bytes = await readFile(resolve(root, file.path));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), file.sha256, `Product ICON changed: ${file.path}`);
  }
  assert.deepEqual(await readFile(resolve(root, 'design/system-v1/icons/loppi.icns')), await readFile(resolve(root, 'src-tauri/icons/icon.icns')));
  assert.deepEqual(await readFile(resolve(root, 'design/system-v1/icons/loppi.ico')), await readFile(resolve(root, 'src-tauri/icons/icon.ico')));
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

test('formal swatches, covers and wordmark variants use the canonical palette', async () => {
  const base = resolve(root, 'design/system-v1');
  const approved = await readFile(resolve(root, 'src/assets/brand/wordmark.svg'), 'utf8');
  assert.equal(await readFile(resolve(root, 'src/assets/brand/wordmark-reversed.svg'), 'utf8'), approved.replaceAll(color('color-text').toUpperCase(), color('entry-text').toUpperCase()));
  for (const [variant, expected] of Object.entries({
    color: approved,
    mono: approved.replaceAll(color('color-logo-dot').toUpperCase(), color('color-text').toUpperCase()),
    reversed: approved.replaceAll(color('color-text').toUpperCase(), color('entry-text').toUpperCase()),
  })) assert.equal(await readFile(resolve(base, `brand/wordmark-${variant}.svg`), 'utf8'), expected);

  const allowed = new Set(Object.entries(tokens)
    .filter(([name, value]) => name.startsWith('palette-') && /^#[a-f\d]{6}$/i.test(value))
    .map(([, value]) => value.toLowerCase()));
  const usage = await readFile(resolve(base, 'usage.svg'), 'utf8');
  for (const [label, token] of [['炭黑', 'ink'], ['纸白', 'paper'], ['暖石', 'stone'], ['完成绿', 'mint'], ['进行中', 'coral'], ['i 方点', 'rust']]) {
    assert(usage.includes(`${label} · ${color('palette-' + token).toUpperCase()}`), `Stale swatch: ${label}`);
  }
  for (const name of ['usage', 'release/readme-header', 'release/social-cover']) {
    const svg = await readFile(resolve(base, `${name}.svg`), 'utf8');
    const background = svg.match(/<rect\b[^>]*fill="(#[a-f\d]{6})"/i)?.[1];
    assert.equal(background?.toLowerCase(), color('color-canvas').toLowerCase(), `${name} background`);
    for (const [, paint] of svg.matchAll(/\b(?:fill|stroke)="(#[a-f\d]{6})"/gi)) {
      assert(allowed.has(paint.toLowerCase()), `${name} has stale color ${paint}`);
    }
    const png = await sharp(await readFile(resolve(base, `${name}.png`))).ensureAlpha().raw().toBuffer({resolveWithObject: true});
    const dimensions = svg.match(/<svg\b[^>]*width="(\d+)" height="(\d+)"/);
    assert.deepEqual([png.info.width, png.info.height], dimensions.slice(1).map(Number), `${name} PNG dimensions`);
    const pixel = (x, y) => '#' + png.data.subarray((y * png.info.width + x) * 4, (y * png.info.width + x) * 4 + 3).toString('hex');
    // Check flat colors, not re-rasterized text: system fonts differ by OS.
    assert.equal(pixel(0, 0), color('color-canvas').toLowerCase(), `${name} PNG background is stale`);
    if (name === 'usage') for (const [, x, paint] of svg.matchAll(/<rect x="(\d+)" y="146" width="192" height="72"[^>]*fill="(#[a-f\d]{6})"/gi)) {
      assert.equal(pixel(Number(x) + 96, 182), paint.toLowerCase(), `PNG swatch at ${x} is stale`);
    }
  }
});

test('Windows ICO frames preserve the approved folded-ear product artwork', async () => {
 const ico=await readFile(resolve(root,'src-tauri/icons/icon.ico'));
 const master=await readFile(resolve(root,'src/assets/rabbit/app-icon-square.png'));
 const approved=JSON.parse(await readFile(resolve(root,'design/system-v1/source/product-icon.json'))).windows_ico;
 assert.equal(createHash('sha256').update(master).digest('hex'),approved.source_sha256);
 assert.equal(ico.readUInt16LE(0),0);assert.equal(ico.readUInt16LE(2),1);
 const sizes=[];
 for(let i=0;i<ico.readUInt16LE(4);i++){
  const at=6+i*16,size=ico[at]||256;
  assert.equal(ico[at+1]||256,size);
  const length=ico.readUInt32LE(at+8),offset=ico.readUInt32LE(at+12);
  assert(offset>=6+16*ico.readUInt16LE(4)&&offset+length<=ico.length);
  const frame=ico.subarray(offset,offset+length);
  const {data:actual,info}=await sharp(frame).ensureAlpha().raw().toBuffer({resolveWithObject:true});
  assert.equal(info.width,size);assert.equal(info.height,size);
  assert.equal(createHash('sha256').update(actual).digest('hex'),approved.rgba_sha256[size],`Changed approved product artwork at ${size}px`);
  sizes.push(size);
 }
 assert.deepEqual(sizes,[16,24,32,48,64,128,256]);
});

test('tokens are acyclic, complete, and preserve the approved foundations', async () => {
  const definitions=[...css.matchAll(/--([\w-]+):\s*([^;]+);/g)];
  assert.equal(definitions.length,Object.keys(tokens).length,'Duplicate canonical token');
  const visit=(name,ancestors=[])=>{
    assert(tokens[name],`Undefined canonical token: ${name}`);
    assert(!ancestors.includes(name),`Cyclic token: ${[...ancestors,name].join(' → ')}`);
    for(const match of tokens[name].matchAll(/var\(--([\w-]+)/g))visit(match[1],[...ancestors,name]);
  };
  for(const name of Object.keys(tokens))visit(name);
  assert.deepEqual(['ink','paper','stone','mint','coral','rust'].map(n=>tokens['palette-'+n]),['#292c28','#f6f5f1','#ecebe5','#93b29c','#d6a08a','#b65b42']);
  for(const name of ['font-ui','font-time','type-body','space-7','radius-control','stroke-icon','shadow-modal','ring-focus','motion-fast','ease-standard','opacity-disabled','layer-drawer','control-height-default','layout-sidebar-width','map-track-height','entry-top-radius'])assert(tokens[name],name);
});

test('the whole frontend consumes tokens without private palettes or undefined references', async () => {
  const {readdir}=await import('node:fs/promises');
  async function sources(dir){const out=[];for(const entry of await readdir(dir,{withFileTypes:true})){const path=resolve(dir,entry.name);if(entry.isDirectory()&&entry.name!=='assets')out.push(...await sources(path));else if(/\.(css|tsx?)$/.test(entry.name))out.push([path,await readFile(path,'utf8')]);}return out;}
  const files=await sources(resolve(root,'src'));
  const declared=new Set(files.flatMap(([,s])=>[...s.matchAll(/--([\w-]+)["']?\s*:/g)].map(m=>m[1])));
  for(const [path,s] of files){
    if(basename(path)==='tokens.css')continue;
    const source=s.replace(/\/\*[\s\S]*?\*\//g,'');
    assert(!/#[a-f\d]{3,8}\b|\b(?:rgb|hsl|oklch|lab)a?\(/i.test(source),`Private palette: ${path}`);
    for(const m of source.matchAll(/var\(--([\w-]+)/g))assert(declared.has(m[1]),`Undefined --${m[1]} in ${path}`);
    if(path.endsWith('.css'))for(const m of source.matchAll(/(?:font-size|font-weight|font-family|border-radius|stroke-width):\s*([^;]+);/g))assert(/var\(|inherit|none|^0$/.test(m[1]),`Untokenized ${m[0]} in ${path}`);
  }
});
