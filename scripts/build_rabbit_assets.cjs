// Package the approved generated poses for the existing shared sprite player.
// sharp is a build-time tool only; pass its module path if not locally installed.
const fs = require('node:fs/promises');
const path = require('node:path');
const sharp = require(process.argv[2] || 'sharp');
const root = path.resolve(__dirname, '..');
const out = path.join(root, 'src/assets/rabbit');
const size = 96;
const baseline = 90;
const raw = { width: size, height: size, channels: 4 };
const source = path.join(root, 'design/rabbit/upright-sleep-v2');

async function normalized(name, fit) {
  const input = await sharp(path.join(source, name)).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  let left = input.info.width, top = input.info.height, right = 0, bottom = 0;
  for (let y = 0; y < input.info.height; y++) for (let x = 0; x < input.info.width; x++) {
    if (input.data[(y * input.info.width + x) * 4 + 3] < 128) continue;
    left = Math.min(left, x); top = Math.min(top, y); right = Math.max(right, x); bottom = Math.max(bottom, y);
  }
  const width = right - left + 1, height = bottom - top + 1;
  const scale = fit / (name.startsWith('listening') ? height : width);
  const w = Math.round(width * scale), h = Math.round(height * scale);
  if (w > size || h > baseline) throw new Error('Pose exceeds shared cell');
  const pose = await sharp(input.data, { raw: input.info }).extract({ left, top, width, height })
    .resize(w, h, { kernel: 'nearest' }).png().toBuffer();
  return sharp({ create: { ...raw, background: '#00000000' } })
    .composite([{ input: pose, left: Math.round((size - w) / 2), top: baseline - h }]).raw().toBuffer();
}
function breathe(frame, sleeping = false, amount = 2) {
  const next = Buffer.from(frame);
  // Awake: lift the head/ears together and extend the torso, keeping feet planted.
  // Asleep: keep the closed face/paws still; visibly lift only the back/torso.
  const left = sleeping ? 65 : 0, right = sleeping ? 92 : size;
  const top = sleeping ? 50 : 0, bottom = 86;
  for (let y = top; y < bottom; y++) for (let x = left; x < right; x++) {
    const sourceY = y < 72 ? y + amount : Math.min(85, 72 + amount + Math.floor((y - 72) * (14 - amount) / 14));
    const from = (sourceY * size + x) * 4, to = (y * size + x) * 4;
    frame.copy(next, to, from, from + 4);
  }
  return next;
}
function shift(frame, dx, dy, lastRow = size) {
  const next = Buffer.from(frame);
  next.fill(0, 0, lastRow * size * 4);
  for (let y = 0; y < lastRow; y++) for (let x = 0; x < size; x++) {
    const xx = x + dx, yy = y + dy;
    if (xx < 0 || xx >= size || yy < 0 || yy >= lastRow) continue;
    frame.copy(next, (yy * size + xx) * 4, (y * size + x) * 4, (y * size + x + 1) * 4);
  }
  return next;
}
function blink(frame) {
  const next = Buffer.from(frame), seen = new Set(), eyes = [];
  const dark = (x, y) => {
    const p = (y * size + x) * 4;
    return frame[p + 3] >= 128 && frame[p] < 75 && frame[p + 1] < 110 && frame[p + 2] < 85;
  };
  for (let y = 43; y < 60; y++) for (let x = 30; x < 67; x++) {
    const id = y * size + x;
    if (seen.has(id) || !dark(x, y)) continue;
    const group = [id]; seen.add(id);
    for (let n = 0; n < group.length; n++) {
      const xx = group[n] % size, yy = Math.floor(group[n] / size);
      for (const [a, b] of [[xx - 1, yy], [xx + 1, yy], [xx, yy - 1], [xx, yy + 1]]) {
        const key = b * size + a;
        if (a >= 30 && a < 67 && b >= 43 && b < 60 && !seen.has(key) && dark(a, b)) { seen.add(key); group.push(key); }
      }
    }
    if (group.length >= 8 && group.length <= 90) eyes.push(group);
  }
  if (eyes.length !== 2) throw new Error(`Expected two isolated eyes, got ${eyes.length}`);
  for (const eye of eyes) {
    const xs = eye.map(p => p % size), ys = eye.map(p => Math.floor(p / size));
    const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
    for (const p of eye) {
      const y = Math.floor(p / size), sample = (y * size + minX - 2) * 4;
      frame.copy(next, p * 4, sample, sample + 4);
    }
    const y = Math.round((minY + maxY) / 2);
    for (let x = minX; x <= maxX; x++) {
      const p = (y * size + x) * 4;
      next[p] = 16; next[p + 1] = 62; next[p + 2] = 47; next[p + 3] = 255;
    }
  }
  return next;
}
async function main() {
  const manifest = JSON.parse(await fs.readFile(path.join(out, 'manifest.json'), 'utf8'));
  const frames = {};
  frames.listening = await normalized('listening-generated.png', 84);
  frames.blink = blink(frames.listening);
  frames.sleeping = await normalized('sleeping-generated.png', 84);
  frames.curious = shift(frames.listening, 2, 0, 66);
  frames.nod = shift(frames.blink, 0, 2);
  frames.stretch = shift(frames.listening, 0, -2);
  frames.wave = frames.listening; // Reserved, no product trigger; keep the same identity.
  for (const name of ['waiting', 'partial']) frames[name] = await sharp(path.join(out, 'poses', `${name}.png`))
    .resize(size, size, { kernel: 'nearest' }).ensureAlpha().raw().toBuffer();
  frames['listening-breath'] = breathe(frames.listening);
  frames['sleeping-breath'] = breathe(frames.sleeping, true);
  frames['sleeping-nod'] = shift(frames.sleeping, 0, 2);
  frames['listening-inhale'] = breathe(frames.listening, false, 4);
  frames['sleeping-inhale'] = breathe(frames.sleeping, true, 4);
  const keys = Object.keys(frames);
  for (const [name, frame] of Object.entries(frames)) await sharp(frame, { raw }).png().toFile(path.join(out, 'poses', `${name}.png`));
  // Cells do not overlap. Copy RGBA directly so compositing cannot round
  // translucent edge colors differently from the standalone fallback.
  const atlasWidth = size * 4, atlasHeight = size * Math.ceil(keys.length / 4);
  const atlas = Buffer.alloc(atlasWidth * atlasHeight * 4);
  keys.forEach((key, n) => {
    for (let y = 0; y < size; y++) frames[key].copy(atlas,
      ((Math.floor(n / 4) * size + y) * atlasWidth + n % 4 * size) * 4,
      y * size * 4, (y + 1) * size * 4);
  });
  await sharp(atlas, { raw: { width: atlasWidth, height: atlasHeight, channels: 4 } }).png().toFile(path.join(out, 'sprite-atlas.png'));
  manifest.version = 2;
  manifest.character = '薄荷兔 · 双竖耳与趴睡';
  manifest.cell = { width: size, height: size, footBaseline: baseline };
  manifest.frames = Object.fromEntries(keys.map((name, n) => [name, { x: n % 4 * size, y: Math.floor(n / 4) * size, width: size, height: size }]));
  manifest.clips.stretch.frames = [['sleeping', 120], ['sleeping-nod', 120], ['stretch', 200], ['listening', 160]];
  manifest.clips.listening.frames = [
    ['listening', 1000], ['listening-breath', 450], ['listening-inhale', 650], ['listening-breath', 450],
    ['listening', 1000], ['listening-breath', 450], ['listening-inhale', 650], ['listening-breath', 450],
    ['blink', 110], ['listening', 790],
  ];
  manifest.clips.sleeping.frames = [
    ['sleeping', 1600], ['sleeping-breath', 800], ['sleeping-inhale', 800], ['sleeping-breath', 800], ['sleeping', 800],
  ];
  manifest.status = '用户已确认v2形象；首页/notch/悬浮共用，状态由EntryRuntime快照驱动';
  await fs.writeFile(path.join(out, 'manifest.json'), JSON.stringify(manifest, null, '\t') + '\n');
  require('node:child_process').execFileSync(path.join(root, 'node_modules/.bin/biome'), ['format', '--write', path.join(out, 'manifest.json')]);
  console.log(`Packed ${keys.length} shared frames into ${size}px cells`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
