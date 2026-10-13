'use strict';
// SHELL-0 module-loading spike -- a minimal Electron main that loads spike
// pages with the SAME renderer security settings as WRL Forge's window
// (contextIsolation true, nodeIntegration false, Electron's default sandbox, a
// strict <meta> CSP) and reports what happened. Evidence only; never shipped.
//
// Speaks the capture-server protocol (WRL_FORGE_CAPTURE_READY / _OK / _ERR over
// stdout, newline-JSON jobs on stdin) so qa/visual-qa/runner.js drives it with
// its launch cap, PID accounting and leak check. Job:
//   { id, file, timeoutMs? }  -> { result: window.__spike, console: [...], loadError? }

const path = require('path');
const { app, BrowserWindow } = require('electron');

const emit = (line) => process.stdout.write(line + '\n');

app.whenReady().then(() => {
  const win = new BrowserWindow({
    show: false, width: 900, height: 700,
    webPreferences: { contextIsolation: true, nodeIntegration: false },
  });
  let consoleLog = [];
  win.webContents.on('console-message', (ev, level, message, line, sourceId) => {
    const e = ev && ev.message !== undefined ? ev : { level, message, lineNumber: line, sourceId };
    consoleLog.push({ level: e.level, message: String(e.message), line: e.lineNumber, source: e.sourceId ? path.basename(String(e.sourceId)) : null });
  });

  async function runJob(job) {
    consoleLog = [];
    let loadError = null;
    try { await win.loadFile(job.file); } catch (err) { loadError = String(err && err.message || err); }
    const deadline = Date.now() + (job.timeoutMs || 3000);
    let result = null;
    while (Date.now() < deadline) {
      result = await win.webContents.executeJavaScript('window.__spike ? JSON.stringify(window.__spike) : null').catch(() => null);
      if (result && JSON.parse(result).done) break;
      await new Promise((r) => setTimeout(r, 20));
    }
    await new Promise((r) => setTimeout(r, 50)); // let late console/error events land
    result = await win.webContents.executeJavaScript('window.__spike ? JSON.stringify(window.__spike) : null').catch(() => result);
    const nav = await win.webContents.executeJavaScript(
      "(function(){var n=performance.getEntriesByType('navigation')[0]||{};return {dclEnd:n.domContentLoadedEventEnd,loadEnd:n.loadEventEnd};})()",
    ).catch(() => null);
    return { result: result ? JSON.parse(result) : null, nav, console: consoleLog, loadError };
  }

  let chain = Promise.resolve();
  const rl = require('readline').createInterface({ input: process.stdin });
  rl.on('line', (raw) => {
    const line = raw.trim();
    if (!line) return;
    let job;
    try { job = JSON.parse(line); } catch { emit('WRL_FORGE_CAPTURE_ERR - bad-json'); return; }
    chain = chain.then(async () => {
      if (job.cmd === 'shutdown') { app.quit(); return; }
      try { emit('WRL_FORGE_CAPTURE_OK ' + job.id + ' ' + JSON.stringify(await runJob(job))); } catch (err) {
        emit('WRL_FORGE_CAPTURE_ERR ' + job.id + ' ' + String(err && err.message || err));
      }
    });
  });
  rl.on('close', () => app.quit());
  emit('WRL_FORGE_CAPTURE_READY');
});

app.on('window-all-closed', () => app.quit());
