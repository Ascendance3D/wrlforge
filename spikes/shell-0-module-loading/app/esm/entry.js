// Native ES module entry: static relative imports, a .mjs import, a dynamic
// import, module scope, import.meta, origin and stack-trace evidence.
import { add, fromA } from './lib/a.js';
import { label } from './lib/b.mjs';

window.__spike.order.push('module');
const topLevelThis = this;
const lazy = await import('./lib/lazy.js');
let stack = null;
try { throw new Error('stack-probe'); } catch (e) { stack = String(e.stack); }
window.__spikeFinish({
  sum: add(2, 3), fromA, label, lazy: lazy.value,
  importMetaUrl: import.meta.url,
  selfOrigin: self.origin, locationOrigin: location.origin, protocol: location.protocol,
  currentScriptIsNull: document.currentScript === null,
  topLevelThisUndefined: topLevelThis === undefined,
  moduleScoped: typeof window.add === 'undefined',
  readyStateAtRun: document.readyState,
  stack,
});
