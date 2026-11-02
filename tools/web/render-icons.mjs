#!/usr/bin/env node
// Exports every native app's Dock icon to PNG for the website (web/src/app-icons/<id>.png).
//
//   ICON_DEPS=/path/with/node_modules node tools/web/render-icons.mjs [id …]
//
// The desktop draws app icons in code: each app exports `Icon({ size })`, a tree of <View>s with
// gradients, radii and shadows. This bundles that component with esbuild, renders it with React to
// static HTML (View -> div, Text -> span, numbers -> px, the same layout defaults as the engine:
// flex column, relative position) and screenshots it in headless Chromium with a transparent
// background. ICON_DEPS must contain react, react-dom and playwright (dev-only tools, not
// dependencies of LordOS).
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const DEPS = process.env.ICON_DEPS;
if (!DEPS) { console.error('set ICON_DEPS to a folder whose node_modules has react, react-dom and playwright'); process.exit(1); }
const dreq = createRequire(path.join(DEPS, 'package.json'));
const esbuild = createRequire(path.join(REPO, 'package.json'))('esbuild');
const React = dreq('react');
const { renderToStaticMarkup } = dreq('react-dom/server');
const { chromium } = dreq('playwright');

const OUT = path.join(REPO, 'web/src/app-icons');
const SIZE = 128;
fs.mkdirSync(OUT, { recursive: true });

// Where each app's Icon lives (index.jsx unless a separate icons file exports it).
const ids = process.argv.slice(2).length ? process.argv.slice(2)
  : fs.readdirSync(path.join(REPO, 'apps')).filter((d) => fs.existsSync(path.join(REPO, 'apps', d, 'manifest.json')));
const iconFile = (id) => {
  for (const f of ['icons.jsx', 'icon.jsx', 'index.jsx']) {
    const p = path.join(REPO, 'apps', id, f);
    if (fs.existsSync(p) && /export\s+(function|const)\s+Icon\b/.test(fs.readFileSync(p, 'utf8'))) return p;
  }
  return null;
};

// @lordos/sdk stand-in: real View/Text, everything else an inert proxy (top-level code of the app
// module may call SDK functions; the icon only needs View and Text).
const SDK_STUB = `
const React = require('react');
const UNITLESS = new Set(['flex','flexGrow','flexShrink','opacity','zIndex','fontWeight','lineHeight','aspectRatio','scale']);
function css(st) {
  const o = {};
  for (const [k, v0] of Object.entries(Object.assign({}, ...[].concat(st || []).filter(Boolean)))) {
    if (v0 == null || v0 === false) continue;
    const v = typeof v0 === 'number' && !UNITLESS.has(k) ? v0 + 'px' : v0;
    if (k === 'paddingHorizontal') { o.paddingLeft = o.paddingRight = v; }
    else if (k === 'paddingVertical') { o.paddingTop = o.paddingBottom = v; }
    else if (k === 'marginHorizontal') { o.marginLeft = o.marginRight = v; }
    else if (k === 'marginVertical') { o.marginTop = o.marginBottom = v; }
    else if (k === 'backgroundColor' && typeof v === 'string' && v.includes('gradient')) o.backgroundImage = v;
    else if (k === 'background' && typeof v === 'string' && v.includes('gradient')) o.backgroundImage = v;
    else o[k] = v;
  }
  return o;
}
const base = { display: 'flex', flexDirection: 'column', position: 'relative', boxSizing: 'border-box', flexShrink: 0, minWidth: 0 };
const View = ({ style, children }) => React.createElement('div', { style: { ...base, ...css(style) } }, children);
const Text = ({ style, children }) => React.createElement('span', { style: { display: 'block', fontFamily: 'Inter, system-ui, sans-serif', whiteSpace: 'pre', ...css(style) } }, children);
// <Image pixels={{ width, height, data: RGBA }}> -> an inline PNG
const zlib = require('zlib');
function png(w, h, rgba) {
  const crcT = []; for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; crcT[n] = c >>> 0; }
  const crc = (b) => { let c = 0xffffffff; for (const x of b) c = crcT[(c ^ x) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => { const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const td = Buffer.concat([Buffer.from(type), data]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([len, td, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) Buffer.from(rgba.buffer, rgba.byteOffset + y * w * 4, w * 4).copy(raw, y * (w * 4 + 1) + 1);
  return 'data:image/png;base64,' + Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]).toString('base64');
}
const Image = ({ style, pixels }) => {
  const st = { ...base, ...css(style) };
  if (pixels && pixels.data && pixels.width) {
    const d = pixels.data instanceof Uint8Array || pixels.data instanceof Uint8ClampedArray ? pixels.data : new Uint8Array(pixels.data.buffer || pixels.data);
    return React.createElement('img', { src: png(pixels.width, pixels.height, d), style: st });
  }
  return React.createElement('div', { style: st });
};
const inert = new Proxy(function () {}, { get: (t, k) => (k === Symbol.toPrimitive ? () => '' : k === 'then' ? undefined : inert), apply: () => inert, construct: () => inert });
module.exports = { View, Text, Image, ScrollView: View, inert };
`;

// Every name the apps import from @lordos/sdk becomes an explicit export of the stand-in (esbuild
// copies only real properties, so a catch-all proxy is not enough).
function sdkNames() {
  const names = new Set();
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.(jsx?|mjs)$/.test(e.name)) {
        for (const m of fs.readFileSync(p, 'utf8').matchAll(/import\s*\{([^}]*)\}\s*from\s*['"]@lordos\/sdk['"]/g)) {
          for (const part of m[1].split(',')) { const n = part.trim().split(/\s+as\s+/)[0].trim(); if (/^[A-Za-z_$][\w$]*$/.test(n)) names.add(n); }
        }
      }
    }
  };
  walk(path.join(REPO, 'apps'));
  return [...names];
}
const SDK_ESM = `import S from 'sdk-cjs';\n` + sdkNames().map((n) => `export const ${n} = S.${n} || S.inert;`).join('\n') + '\nexport default S.inert;';

async function bundle(id, file) {
  const res = await esbuild.build({
    stdin: { contents: `export { Icon } from ${JSON.stringify(file)};`, resolveDir: REPO, loader: 'js' },
    bundle: true, write: false, format: 'cjs', platform: 'node', jsx: 'automatic', logLevel: 'silent',
    loader: { '.js': 'jsx', '.jsx': 'jsx' },
    plugins: [{
      name: 'stubs',
      setup(b) {
        b.onResolve({ filter: /^@lordos\/sdk/ }, () => ({ path: 'sdk', namespace: 'stub' }));
        b.onResolve({ filter: /^sdk-cjs$/ }, () => ({ path: 'sdk-cjs', namespace: 'stub' }));
        b.onResolve({ filter: /^react(\/.*)?$|^react-dom/ }, (a) => ({ path: dreq.resolve(a.path), external: true }));
        b.onResolve({ filter: /\.(css|png|svg|wasm|json)$/ }, (a) => ({ path: a.path, namespace: 'empty' }));
        b.onLoad({ filter: /^sdk$/, namespace: 'stub' }, () => ({ contents: SDK_ESM, loader: 'js', resolveDir: REPO }));
        b.onLoad({ filter: /^sdk-cjs$/, namespace: 'stub' }, () => ({ contents: SDK_STUB, loader: 'js', resolveDir: REPO }));
        b.onLoad({ filter: /.*/, namespace: 'empty' }, () => ({ contents: 'module.exports = {}', loader: 'js' }));
      },
    }],
  });
  const code = res.outputFiles[0].text;
  const mod = { exports: {} };
  // eslint-disable-next-line no-new-func
  new Function('module', 'exports', 'require', code)(mod, mod.exports, (p) => (p === 'sdk' ? null : dreq(p)));
  return mod.exports.Icon;
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: SIZE * 2, height: SIZE * 2 }, deviceScaleFactor: 1 });
let ok = 0;
for (const id of ids) {
  const file = iconFile(id);
  if (!file) { console.warn(`${id}: no Icon export, skipped`); continue; }
  try {
    const Icon = await bundle(id, file);
    if (typeof Icon !== 'function') throw new Error('Icon is not a component');
    const html = renderToStaticMarkup(React.createElement(Icon, { size: SIZE }));
    // the shadow is part of the icon, so leave a margin for it and crop to the icon box plus a little
    await page.setContent(`<!doctype html><html><body style="margin:0;background:transparent"><div id="i" style="position:absolute;left:${SIZE / 2}px;top:${SIZE / 2}px">${html}</div></body></html>`);
    const pad = Math.round(SIZE * 0.06);
    await page.screenshot({ path: path.join(OUT, `${id}.png`), omitBackground: true, clip: { x: SIZE / 2 - pad, y: SIZE / 2 - pad, width: SIZE + 2 * pad, height: SIZE + 2 * pad } });
    ok++;
    console.log(`${id}: ${path.relative(REPO, file)} -> web/src/app-icons/${id}.png`);
  } catch (e) {
    console.warn(`${id}: failed (${e.message.split('\n')[0]})`);
  }
}
await browser.close();
console.log(`${ok}/${ids.length} icons rendered`);
