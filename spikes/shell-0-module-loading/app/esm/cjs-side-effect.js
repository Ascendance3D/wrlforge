// A dual-format shared src/ module (CommonJS in Node, window.* in a classic
// script) imported as a module for its side effect.
import '../../../../src/editor/command-registry.js';
const api = window.WrlCommandRegistry;
window.__spikeFinish({
  globalPublished: typeof api,
  registryWorks: !!(api && api.createCommandRegistry && api.createCommandRegistry().list().length === 0),
});
