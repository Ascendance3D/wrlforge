'use strict';
// WD2-D QA-only: a minimal Chrome DevTools Protocol client for the Electron
// window the capture server already drives.
//
// Why: real pointer input must travel Chromium's own input pipeline (trusted
// events -> X_ITE's handlers -> WD2-D's listener), and the contract forbids
// any main-process, preload or IPC change to get it. Electron honours the
// standard Chromium `--remote-debugging-port` switch, so the QA launcher adds
// that flag (port 0 = OS-assigned, loopback only) and this client sends
// `Input.dispatchMouseEvent` -- the same browser-side path DevTools uses.
// Nothing here is loaded by the product.

const fs = require('fs');
const path = require('path');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Chromium writes "<port>\n<browser ws path>" here once the endpoint is up.
async function waitForPort(userDataDir, timeoutMs = 20000) {
  const file = path.join(userDataDir, 'DevToolsActivePort');
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const port = Number(fs.readFileSync(file, 'utf8').split('\n')[0]);
      if (port > 0) return port;
    } catch { /* not yet */ }
    await sleep(100);
  }
  throw new Error('cdp: DevToolsActivePort never appeared');
}

async function pageTarget(port, match, timeoutMs = 20000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/list`);
      const list = await res.json();
      const t = list.find((x) => x.type === 'page' && (!match || match.test(x.url)));
      if (t) return t;
    } catch { /* endpoint not ready */ }
    await sleep(150);
  }
  throw new Error('cdp: no page target');
}

function connect(wsUrl) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    let nextId = 1;
    const pending = new Map();
    const listeners = new Map();
    ws.onopen = () => resolve(api);
    ws.onerror = (e) => reject(new Error(`cdp: websocket error ${e && e.message}`));
    ws.onclose = () => { for (const p of pending.values()) p.reject(new Error('cdp: closed')); pending.clear(); };
    ws.onmessage = (ev) => {
      const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : Buffer.from(ev.data).toString('utf8'));
      if (msg.id && pending.has(msg.id)) {
        const p = pending.get(msg.id);
        pending.delete(msg.id);
        if (msg.error) p.reject(new Error(`cdp ${p.method}: ${msg.error.message}`));
        else p.resolve(msg.result);
      } else if (msg.method) {
        for (const fn of listeners.get(msg.method) || []) fn(msg.params);
      }
    };
    const api = {
      send(method, params = {}) {
        const id = nextId++;
        ws.send(JSON.stringify({ id, method, params }));
        return new Promise((res, rej) => pending.set(id, { resolve: res, reject: rej, method }));
      },
      on(method, fn) {
        if (!listeners.has(method)) listeners.set(method, []);
        listeners.get(method).push(fn);
      },
      close() { try { ws.close(); } catch { /* already closed */ } },
    };
  });
}

// One QA session on the editor window: evaluation, real mouse input and an
// exception/console log with FULL stack traces.
async function attach(userDataDir) {
  const port = await waitForPort(userDataDir);
  const target = await pageTarget(port);
  const c = await connect(target.webSocketDebuggerUrl);
  const exceptions = [];
  const consoleErrors = [];
  const frames = (st) => (st && st.callFrames ? st.callFrames.map((f) => `${f.functionName || '<anon>'} ${path.basename(f.url || '')}:${f.lineNumber + 1}:${f.columnNumber + 1}`) : []);
  c.on('Runtime.exceptionThrown', (p) => {
    const d = p.exceptionDetails || {};
    exceptions.push({
      text: d.text,
      description: d.exception && d.exception.description ? d.exception.description : null,
      url: d.url || null, line: d.lineNumber, column: d.columnNumber, stack: frames(d.stackTrace),
    });
  });
  c.on('Runtime.consoleAPICalled', (p) => {
    if (p.type !== 'error' && p.type !== 'warning' && p.type !== 'assert') return;
    consoleErrors.push({
      level: p.type,
      message: (p.args || []).map((a) => (a.value !== undefined ? String(a.value) : a.description || a.type)).join(' '),
      stack: frames(p.stackTrace),
    });
  });
  await c.send('Runtime.enable');

  async function evaluate(expression) {
    const r = await c.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: false });
    if (r.exceptionDetails) {
      const d = r.exceptionDetails;
      throw new Error(`evaluate: ${d.exception && d.exception.description ? d.exception.description : d.text}`);
    }
    return r.result ? r.result.value : undefined;
  }

  // A hook on the page's existing read-only QA surface, by name.
  const hook = (name, ...args) => evaluate(`(async () => {
    const h = window.__wrlEditor;
    if (!h || typeof h[${JSON.stringify(name)}] !== 'function') throw new Error('no __wrlEditor hook ${name}');
    const v = await h[${JSON.stringify(name)}](...${JSON.stringify(args)});
    return v === undefined ? null : JSON.parse(JSON.stringify(v));
  })()`);

  // Real Chromium mouse input at CSS (DIP) client coordinates.
  let buttons = 0;
  async function mouse(type, x, y) {
    const t = { move: 'mouseMoved', down: 'mousePressed', up: 'mouseReleased' }[type];
    if (type === 'down') buttons = 1;
    const params = { type: t, x, y, modifiers: 0, buttons: type === 'up' ? 0 : buttons };
    if (type !== 'move') { params.button = 'left'; params.clickCount = 1; }
    else if (buttons) params.button = 'left';
    await c.send('Input.dispatchMouseEvent', params);
    if (type === 'up') buttons = 0;
  }

  async function screenshot(file) {
    const r = await c.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(file, Buffer.from(r.data, 'base64'));
  }

  return { evaluate, hook, mouse, screenshot, exceptions, consoleErrors, close: () => c.close(), send: c.send };
}

module.exports = { attach, waitForPort };
