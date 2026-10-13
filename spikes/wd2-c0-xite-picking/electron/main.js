'use strict';
// WD2-C0 Electron driver. SPIKE ONLY -- separate from the product main.js.
//
// Security posture is the product's: contextIsolation on, nodeIntegration off,
// no preload, no IPC. The page is driven with webContents.executeJavaScript and
// real pointer events via webContents.sendInputEvent. A spike-private
// `wd2c0:` scheme serves (read-only) the spike page, the installed X_ITE dist
// and the in-memory fixtures -- nothing else.
//
// Run through probe.js, which supplies an isolated --user-data-dir.
const { app, BrowserWindow, protocol } = require('electron');
const fs = require('fs');
const path = require('path');
const fixtures = require('../fixtures');

const SPIKE = path.join(__dirname, '..');
const REPO = path.join(SPIKE, '..', '..');
const XITE_DIST = path.join(REPO, 'node_modules', 'x_ite', 'dist');
const OUT = path.join(SPIKE, 'out');
const VARIANT = process.env.WD2C0_VARIANT || 'main';

protocol.registerSchemesAsPrivileged([{
  scheme: 'wd2c0',
  privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true },
}]);

const MEM = new Map(); // fixture name -> text (current run)
const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wrl': 'model/vrml', '.json': 'application/json', '.wasm': 'application/wasm' };

function confined(root, rel) {
  const abs = path.resolve(root, rel);
  return abs === root || abs.startsWith(root + path.sep) ? abs : null;
}

function serve(request) {
  const u = new URL(request.url);
  const p = decodeURIComponent(u.pathname).replace(/^\/+/, '');
  let abs = null;
  if (p.startsWith('fixtures/')) {
    const name = p.slice('fixtures/'.length);
    if (MEM.has(name)) return new Response(MEM.get(name), { headers: { 'content-type': 'model/vrml' } });
    return new Response('not found', { status: 404 });
  }
  if (p.startsWith('browser/')) abs = confined(path.join(SPIKE, 'browser'), p.slice(8));
  else if (p.startsWith('x_ite/')) abs = confined(XITE_DIST, p.slice(6));
  if (!abs || !fs.existsSync(abs) || !fs.statSync(abs).isFile()) return new Response('not found', { status: 404 });
  return new Response(fs.readFileSync(abs), { headers: { 'content-type': MIME[path.extname(abs)] || 'application/octet-stream' } });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function run(win) {
  const wc = win.webContents;
  const js = (expr) => wc.executeJavaScript(expr, true);
  const call = (fn, ...args) => js(`window.wd2c0.${fn}(${args.map((a) => JSON.stringify(a)).join(',')})`);
  const raw = { variant: VARIANT, env: {}, fixtures: [], coordinate: [], reload: null, edit: null, perf: [], sensors: [], anchor: null };

  raw.env.init = await call('init');
  raw.env.electron = process.versions.electron;
  raw.env.chrome = process.versions.chrome;

  const loadFixture = async (fx, opts = {}) => {
    MEM.clear();
    MEM.set(`${fx.id}.wrl`, fx.text);
    for (const [k, v] of Object.entries(fx.children || {})) MEM.set(k, v);
    const res = await call('load', fx.text, { baseURL: `wd2c0://app/fixtures/${fx.id}.wrl`, ...opts });
    if (fx.children) await sleep(1500); // let the Inline fetch + build
    await js('new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))');
    return res;
  };

  const clientPoint = async (click, cameras) => {
    const info = await call('canvasInfo');
    const cam = click.camera ? cameras[click.camera] : null;
    const p = fixtures.project(click.world, cam, info.rect.width, info.rect.height);
    return { x: info.rect.left + p.x, y: info.rect.top + p.y, info };
  };

  const pointerClick = async (x, y, zoom = 1) => {
    await call('armPointer');
    const X = Math.round(x * zoom), Y = Math.round(y * zoom);
    wc.sendInputEvent({ type: 'mouseMove', x: X, y: Y });
    wc.sendInputEvent({ type: 'mouseDown', x: X, y: Y, button: 'left', clickCount: 1 });
    wc.sendInputEvent({ type: 'mouseUp', x: X, y: Y, button: 'left', clickCount: 1 });
    await sleep(120);
    const r = await call('takePointer');
    return r[0] || null;
  };

  if (VARIANT === 'main') {
    // ---- fixture matrix -------------------------------------------------
    for (const fx of fixtures.build()) {
      const load = await loadFixture(fx);
      const rec = { id: fx.id, load, clicks: [] };
      if (fx.sensorProbe) await call('watchSensor', fx.sensorProbe);
      let boundCamera = null;
      for (const click of fx.clicks) {
        if (click.camera && click.camera !== boundCamera) { await call('bindViewpoint', click.camera); boundCamera = click.camera; }
        const pt = await clientPoint(click, fx.cameras || {});
        const hit = await call('pickClient', pt.x, pt.y);
        rec.clicks.push({ click: click.id, hit });
      }
      if (fx.sensorProbe) {
        // touch() alone vs. a real pointer click on the sensed geometry.
        const afterTouch = await call('sensorEvents');
        const sensed = fx.clicks.find((c) => c.id === 'sensed');
        const pt = await clientPoint(sensed, {});
        const viaPointer = await pointerClick(pt.x, pt.y);
        await sleep(300);
        raw.sensors.push({ fixture: fx.id, eventsAfterTouchOnly: afterTouch, eventsAfterRealClick: await call('sensorEvents'), pointerHit: viaPointer });
      }
      if (fx.id === 'P18-anchor') {
        const before = await call('activeViewpointDescription');
        const touchOnly = await call('activeViewpointDescription');
        const c = fx.clicks[0];
        const pt = await clientPoint(c, {});
        const viaPointer = await pointerClick(pt.x, pt.y);
        await sleep(2500);
        raw.anchor = { before, afterTouchOnly: touchOnly, afterRealClick: await call('activeViewpointDescription'), pointerHit: viaPointer };
      }
      raw.fixtures.push(rec);
    }

    // ---- reload identity -------------------------------------------------
    {
      const fx = fixtures.build().find((f) => f.id === 'P5-anonymous-twins');
      const g1 = await loadFixture(fx);
      await call('clearRetained');
      const picks1 = [];
      for (const c of fx.clicks.slice(0, 2)) {
        const pt = await clientPoint(c, {});
        picks1.push(await js(`window.wd2c0.pickClient(${pt.x},${pt.y},{retain:true})`));
      }
      const g2 = await loadFixture(fx);
      const picks2 = [];
      for (const c of fx.clicks.slice(0, 2)) {
        const pt = await clientPoint(c, {});
        picks2.push(await call('pickClient', pt.x, pt.y));
      }
      raw.reload = { fixture: fx.id, g1, g2, picks1, picks2, retained: await call('retainedReport'), text: fx.text };
    }
    // ---- source edit (text supplied by probe.js through WD2C0_EDIT) ------
    if (process.env.WD2C0_EDIT) {
      const edit = JSON.parse(fs.readFileSync(process.env.WD2C0_EDIT, 'utf8'));
      const before = { id: 'EDIT-before', text: edit.before };
      const after = { id: 'EDIT-after', text: edit.after };
      const g1 = await loadFixture(before);
      const picks1 = [];
      for (const w of edit.clicks) {
        const p = fixtures.project(w.before, null, 800, 800);
        picks1.push(await call('pickClient', p.x, p.y));
      }
      const g2 = await loadFixture(after);
      const picks2 = [];
      for (const w of edit.clicks) {
        const p = fixtures.project(w.after, null, 800, 800);
        picks2.push(await call('pickClient', p.x, p.y));
      }
      raw.edit = { g1, g2, picks1, picks2 };
    }

    // ---- performance sanity ------------------------------------------------
    for (const [n, stride] of [[1, 1], [100, 5], [1000, 25]]) {
      const fx = fixtures.gridFixture(n, stride);
      const off = await loadFixture(fx, { hook: false, withOccurrences: false });
      const on = await loadFixture(fx, { withOccurrences: true });
      const clicks = [];
      for (const c of fx.clicks) {
        const pt = await clientPoint(c, {});
        clicks.push({ click: c.id, hit: await call('pickClient', pt.x, pt.y) });
      }
      raw.perf.push({ id: fx.id, n, loadHookOff: off, loadHookOn: on, clicks });
    }
  }

  // ---- coordinates: layout, resize, rem scale, page zoom, DPR ------------
  {
    const fx = fixtures.build().find((f) => f.id === 'P6-many-siblings');
    await loadFixture(fx);
    const layouts = VARIANT === 'main'
      ? [
        { name: 'base-800', layout: { left: 0, top: 0, width: 800, height: 800 } },
        { name: 'offset-37-53', layout: { left: 37, top: 53, width: 800, height: 800 } },
        { name: 'resize-640x480', layout: { left: 0, top: 0, width: 640, height: 480 } },
        { name: 'resize-1000x700-offset', layout: { left: 21, top: 9, width: 1000, height: 700 } },
        { name: 'rem-ui-scale-150', layout: { left: 24, top: 24, width: 700, height: 600, rootFontPx: 24 } },
        { name: 'page-zoom-1.5', layout: { left: 10, top: 10, width: 600, height: 500 }, zoom: 1.5 },
      ]
      : [
        { name: `dpr-${VARIANT}`, layout: { left: 17, top: 11, width: 700, height: 600 } },
        { name: `dpr-${VARIANT}-contentScale-2`, layout: { left: 17, top: 11, width: 700, height: 600 }, contentScale: '2' },
      ];
    for (const L of layouts) {
      wc.setZoomFactor(L.zoom || 1);
      if (L.contentScale) await js(`document.getElementById('canvas').setAttribute('contentScale', ${JSON.stringify(L.contentScale)})`);
      await sleep(200);
      const info = await call('setLayout', L.layout);
      const rows = [];
      for (const c of [fx.clicks[0], fx.clicks[5], fx.clicks[11], fx.clicks[30]]) {
        const pt = await clientPoint(c, {});
        const viaTouch = await call('pickClient', pt.x, pt.y);
        const viaPointer = await pointerClick(pt.x, pt.y, L.zoom || 1);
        rows.push({ click: c.id, viaTouch, viaPointer });
      }
      raw.coordinate.push({ fixture: fx.id, name: L.name, zoom: L.zoom || 1, info, rows });
    }
    wc.setZoomFactor(1);
  }

  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(path.join(OUT, `raw-${VARIANT}.json`), JSON.stringify(raw, null, 1));
}

app.whenReady().then(async () => {
  protocol.handle('wd2c0', serve);
  const win = new BrowserWindow({
    width: 1100, height: 900, show: true, useContentSize: true,
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  win.webContents.on('console-message', (e) => {
    const m = e.message || '';
    if (/error|warn|Parser/i.test(m)) console.log('[page]', m.slice(0, 300));
  });
  await win.loadURL('wd2c0://app/browser/index.html');
  let code = 0;
  try { await run(win); } catch (err) { console.error('[wd2c0] FAILED', err && err.stack || err); code = 1; }
  app.exit(code);
});
