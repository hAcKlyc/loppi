import { build } from 'vite';
import react from '@vitejs/plugin-react';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixture = await readFile(resolve(root, 'scripts/test_design_scene.html'), 'utf8');
const moduleSource = fixture.match(/<script type="module">([\s\S]*?)<\/script>/)?.[1];
if (!moduleSource) throw Error('The isolated real-component fixture has no module');
const entry = resolve(root, 'scripts/design-preview-entry.tsx');
const result = await build({
  root, configFile: false,
  define: { 'process.env.NODE_ENV': '"production"' },
  plugins: [react(), {
    name: 'isolated-design-preview',
    resolveId(id) { if (id === entry) return entry; },
    load(id) { if (id === entry) return moduleSource.replace("'/src/App.tsx'", JSON.stringify(resolve(root, 'src/App.tsx'))); },
  }],
  build: { write: false, assetsInlineLimit: Infinity, cssCodeSplit: false, minify: true,
    lib: { entry, formats: ['iife'], name: 'LoppiDesignPreview' } },
});
const output = Array.isArray(result) ? result.flatMap(x => x.output) : result.output;
const js = output.find(x => x.type === 'chunk').code.replace(/<\/script/gi, '<\\/script');
const css = output.filter(x => x.type === 'asset' && x.fileName.endsWith('.css')).map(x => String(x.source)).join('\n');
const html = `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>loppi · 设计样板 · 合成数据</title><style>${css}</style><div id="root"></div><output id="fixture-evidence" hidden></output><script>${js}</script></html>\n`;
await writeFile(resolve(root, 'design/system-v1/preview.html'), html);
console.log('Built offline preview from the real App and isolated fixture');
