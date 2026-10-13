'use strict';
// SHELL-0 spike probe (classic script, loaded first on every spike page).
// Records execution order, errors (window AND element-level, via capture),
// unhandled rejections and CSP violations into window.__spike for main to read.
window.__spike = { order: [], errors: [], violations: [], done: false, data: {}, t: null };
window.addEventListener('error', function (e) {
  if (e.target && e.target !== window) {
    window.__spike.errors.push({ kind: 'element', tag: e.target.tagName, src: e.target.src || null });
    return;
  }
  window.__spike.errors.push({ kind: 'window', message: e.message, filename: e.filename, lineno: e.lineno, colno: e.colno,
    stack: e.error && e.error.stack ? String(e.error.stack) : null });
}, true);
window.addEventListener('unhandledrejection', function (e) {
  var r = e.reason;
  window.__spike.errors.push({ kind: 'rejection', message: String(r && r.message || r), stack: r && r.stack ? String(r.stack) : null });
});
document.addEventListener('securitypolicyviolation', function (e) {
  window.__spike.violations.push({ directive: e.violatedDirective, blockedURI: e.blockedURI, sample: e.sample || null });
});
window.__spikeFinish = function (data) {
  Object.assign(window.__spike.data, data || {});
  window.__spike.t = performance.now();
  window.__spike.done = true;
};
