// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1 isolated Electron main for the renderer wasm proof. NOT main.js.
// Speaks the VisualQaRunner capture protocol (READY / OK <id> <json> /
// ERR <id> <msg>) over stdio so qa/visual-qa/runner.js drives it: one process,
// bounded, PID-tracked, leak-checked.
//
// Security posture mirrors main.js webPreferences exactly (contextIsolation
// true, nodeIntegration false) and grants LESS: no preload, no IPC handler, no
// protocol registration, no session/CSP header changes, window never shown.
'use strict';
const path = require('path');
const readline = require('readline');
const { app, BrowserWindow } = require('electron');

const PAGE_TIMEOUT_MS = 120000;

function runPage(page) {
  return new Promise((resolve, reject) => {
    const win = new BrowserWindow({
      show: false,
      width: 800,
      height: 600,
      webPreferences: { contextIsolation: true, nodeIntegration: false },
    });
    const prefs = win.webContents.getLastWebPreferences();
    const timer = setTimeout(() => { win.destroy(); reject(new Error(`timeout on ${page}`)); }, PAGE_TIMEOUT_MS);
    win.webContents.on('console-message', (e, level, msg) => {
      const message = typeof e.message === 'string' ? e.message : msg;
      if (!message.startsWith('RUST1_RESULT ')) return;
      clearTimeout(timer);
      const out = JSON.parse(message.slice('RUST1_RESULT '.length));
      out.webPreferences = {
        contextIsolation: prefs.contextIsolation,
        nodeIntegration: prefs.nodeIntegration,
        sandbox: prefs.sandbox,
        webSecurity: prefs.webSecurity,
        preload: prefs.preload || null,
      };
      out.electron = process.versions.electron;
      out.chrome = process.versions.chrome;
      win.destroy();
      resolve(out);
    });
    win.loadFile(path.join(__dirname, page)).catch((err) => { clearTimeout(timer); win.destroy(); reject(err); });
  });
}

app.whenReady().then(() => {
  process.stdout.write('WRL_FORGE_CAPTURE_READY\n');
  const rl = readline.createInterface({ input: process.stdin });
  let queue = Promise.resolve();
  rl.on('line', (line) => {
    const job = JSON.parse(line);
    if (job.cmd === 'shutdown') { queue.then(() => app.quit()); return; }
    queue = queue.then(() => runPage(job.page)
      .then((out) => process.stdout.write(`WRL_FORGE_CAPTURE_OK ${job.id} ${JSON.stringify(out)}\n`))
      .catch((err) => process.stdout.write(`WRL_FORGE_CAPTURE_ERR ${job.id} ${String(err.message).replace(/\n/g, ' ')}\n`)));
  });
});
app.on('window-all-closed', () => { /* keep alive until shutdown */ });
