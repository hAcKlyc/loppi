// Visual asset player. The host remains the sole owner of actual capture state.
const bundles = new Map();
async function loadBundle(base) {
  const url = new URL(base, document.baseURI);
  const key = url.href;
  if (!bundles.has(key)) bundles.set(key, (async () => {
    const response = await fetch(new URL('manifest.json', url));
    if (!response.ok) throw new Error('Rabbit manifest unavailable');
    const manifest = await response.json();
    const atlas = new Image();
    atlas.src = new URL(manifest.atlas, url).href;
    await atlas.decode();
    return { manifest, atlas };
  })().catch(error => { bundles.delete(key); throw error; }));
  return bundles.get(key);
}
export class RabbitPlayer {
  constructor(canvas, { base = './', state = 'listening', reducedMotion = null } = {}) {
    this.canvas = canvas;
    this.state = state;
    this.reducedMotion = reducedMotion;
    this.disposed = false;
    this.timer = null;
    this.media = matchMedia('(prefers-reduced-motion: reduce)');
    this.onVisibility = () => {
      this.stop();
      if (!document.hidden) this.run(this.state);
    };
    this.onMotion = () => { this.stop(); this.run(this.state); };
    document.addEventListener('visibilitychange', this.onVisibility);
    this.media.addEventListener('change', this.onMotion);
    this.ready = loadBundle(base).then(bundle => {
      if (this.disposed) return;
      this.bundle = bundle;
      canvas.width = bundle.manifest.cell.width;
      canvas.height = bundle.manifest.cell.height;
      this.context = canvas.getContext('2d');
      this.context.imageSmoothingEnabled = false;
      canvas.dataset.state = this.state;
      this.run(this.state);
    });
  }
  get quiet() { return this.reducedMotion ?? this.media.matches; }
  stop() { clearTimeout(this.timer); this.timer = null; }
  draw(frame, badge = false) {
    const {manifest, atlas} = this.bundle;
    const rect = manifest.frames[frame];
    const context = this.context;
    context.clearRect(0, 0, this.canvas.width, this.canvas.height);
    context.drawImage(atlas, rect.x, rect.y, rect.width, rect.height, 0, 0, rect.width, rect.height);
    if (badge) {
      context.fillStyle = '#eab677'; context.fillRect(37, 35, 7, 9);
      context.fillStyle = '#543b1d'; context.fillRect(40, 37, 1, 3); context.fillRect(40, 41, 1, 1);
    }
    this.canvas.dataset.frame = frame;
  }
  run(name) {
    if (this.disposed || !this.bundle) return;
    const clip = this.bundle.manifest.clips[name];
    if (!clip) throw new Error('Unknown rabbit clip: ' + name);
    this.stop();
    this.clip = name;
    this.canvas.dataset.clip = name;
    const advance = index => {
      if (this.disposed) return;
      const [frame, delay] = clip.frames[index];
      this.draw(frame, clip.badge);
      if (document.hidden || this.quiet || clip.frames.length === 1) return;
      this.timer = setTimeout(() => {
        if (index + 1 < clip.frames.length) advance(index + 1);
        else if (clip.loop) advance(0);
        else this.run(this.state);
      }, delay);
    };
    advance(0);
  }
  setState(state, { transition = false } = {}) {
    if (!['listening','sleeping','waiting','partial','interrupted'].includes(state)) throw new Error('Unknown rabbit state: ' + state);
    const previous = this.state;
    this.state = state;
    this.canvas.dataset.state = state;
    this.stop();
    if (transition && !this.quiet && previous !== state && (state === 'sleeping' || (previous === 'sleeping' && state === 'listening'))) {
      this.run(state === 'sleeping' ? 'pause' : 'stretch');
    } else this.run(state);
  }
  playAction(action) {
    if (!['curious','nod','stretch','wave'].includes(action)) throw new Error('Unknown rabbit action: ' + action);
    if (this.disposed || this.quiet || document.hidden || !this.bundle) return;
    if (this.state === 'sleeping') this.run('sleeping-response');
    else if (this.state === 'listening') this.run(action);
  }
  setReducedMotion(value) {
    this.reducedMotion = value;
    this.stop(); this.run(this.state);
  }
  dispose() {
    this.disposed = true;
    this.stop();
    document.removeEventListener('visibilitychange', this.onVisibility);
    this.media.removeEventListener('change', this.onMotion);
  }
}
