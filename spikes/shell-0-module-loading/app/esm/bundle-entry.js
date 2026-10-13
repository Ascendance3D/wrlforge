// The same graph as entry.js, written for an esbuild IIFE entry bundle
// (IIFE output has no top-level await and no import.meta).
import { add, fromA } from './lib/a.js';
import { label } from './lib/b.mjs';

window.__spike.order.push('bundle');
import('./lib/lazy.js').then((lazy) => {
  let stack = null;
  try { throw new Error('stack-probe'); } catch (e) { stack = String(e.stack); }
  window.__spikeFinish({
    sum: add(2, 3), fromA, label, lazy: lazy.value,
    currentScriptIsNull: document.currentScript === null,
    moduleScoped: typeof window.add === 'undefined',
    readyStateAtRun: document.readyState,
    stack,
  });
});
