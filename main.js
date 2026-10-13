'use strict';
const { app, BrowserWindow, dialog, ipcMain, shell, screen, session, protocol } = require('electron');
const path = require('path');
const fs = require('fs');
const zlib = require('zlib');
const { spawn } = require('child_process');
const { validate } = require('./validator');
const { isGzip, editPathFor } = require('./src/files/vrml-file');
const { readWrlSource } = require('./src/preview/wrl-source');
const { measureArtifact, mallPayload } = require('./src/mall/artifact-size');
const { repackMall } = require('./src/mall/repack');
const { activeRepackPaths } = require('./src/mall/repack-paths');
const { fileDirUrl } = require('./src/preview/texture-base');
const { isBlockedPreviewUrl, scanRemoteUrls } = require('./src/preview/url-policy');
const { detectPrimaries } = require('./src/world-project/project-loader');
const { summarize } = require('./src/world-project/project-stats');
const { ProjectSession } = require('./src/world-project/session');
const {
  WORLD_PREVIEW_SCHEME,
  buildAuthorizedSet,
  buildPreviewPayload,
  resolveWorldRequest,
} = require('./src/world-project/preview-source');
const { buildPackagePlan, buildManifest } = require('./src/world-project/package-plan');
const { writeReviewBundle } = require('./src/world-project/bundle-builder');
const { resolveEditor, buildLaunch } = require('./src/editor/editor-locator');
const { loadSettings } = require('./src/settings/app-settings');
const { EditorController } = require('./src/editor/editor-controller');
const { RecoveryController } = require('./src/editor/recovery-controller');
const { openMallItem, openExternalEditor } = require('./src/editor/mall-edit-flow');
const { createMallPreviewBridge } = require('./src/preview/mall-preview-bridge');
const { createWorldPreviewBridge } = require('./src/preview/world-preview-bridge');
const { findVrmlFileArgument } = require('./src/app/file-open');

// The World Project preview scheme (Phase 4B) is a privileged, standard, LOCAL
// scheme. This MUST run before app 'ready'. It does NOT bypass CSP -- world.html
// still lists the scheme explicitly. Its handler (installed in whenReady) serves
// ONLY asset-graph-authorized files confined to the open project's root.
protocol.registerSchemesAsPrivileged([{
  scheme: WORLD_PREVIEW_SCHEME,
  privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true },
}]);

// The currently-authorized World Project preview: { projectRoot, authorized:Map }
// or null when no world preview is active. Set only by world:previewLoad (which
// derives the allow-list from the production asset graph). The scheme handler
// reads this; when null it refuses every request. READ-ONLY -- serving reads
// files, never writes.
let worldPreview = null;

// Which file the embedded preview is allowed to read. Set only by openMallFile
// (which itself is reached only via the user's Open dialog or an explicit path),
// so the read-only preview:load channel can never be steered at an arbitrary
// path by the renderer -- it may read only the currently-open item or its
// .edit.wrl sibling, never a renderer-supplied path.
let currentSession = null; // { mallPath, editFile }

// The single open World Project (Phase 4A). The main process owns the project
// root and detected primary candidates; the renderer never supplies an arbitrary
// scan path -- it can only pick among candidates the main process detected. The
// world surface is strictly READ-ONLY: there is no write-capable world IPC.
const worldSession = new ProjectSession();

// The main window, used for Mall<->World page navigation (main keeps control of
// which local page loads; the renderer cannot navigate to an arbitrary URL).
let mainWindow = null;

// A desktop/file-manager launch may arrive before Electron is ready, while a
// second launch is forwarded after the window already exists. Keep only the
// newest explicit file request; this app has one active Mall Item session.
let pendingOpenFile = findVrmlFileArgument(process.argv);
let desktopOpenSequence = Promise.resolve();
const hasSingleInstanceLock = app.requestSingleInstanceLock();
if (!hasSingleInstanceLock) app.quit();

// The native editor controller (Phase 7B). Instantiated in whenReady (it needs
// app.getPath('userData') for session restore). It owns the one open editor
// document and every path decision; the renderer supplies only text + intent (and
// a World reference to OPEN, which is authorized against the scan graph below).
let editorController = null;

// The Phase Beta 2 recovery controller. Owns the recovery lifecycle for the
// one open editor document: debounced dirty-state snapshots to userData, a
// startup-time read for the Restore / Start Fresh prompt, and a one-shot
// adopt-from-snapshot helper. The renderer pings it after every keystroke
// (throttled) and at Save / Close points; main remains the only writer to
// userData. Instantiated alongside editorController.
let recoveryController = null;

// The Mall live-preview bridge (Phase 7C2). Turns the native editor's UNSAVED
// buffer into an authorized X_ITE preview payload without writing any temp file.
// It owns the 7C1 BufferOverlay; the renderer sends only { sessionId, text,
// bufferVersion } and never a path. Instantiated in whenReady alongside the
// editor controller (it reads the live currentSession / editor describe).
let mallPreviewBridge = null;

// The World live-preview bridge (Phase 7C3). The World twin of the Mall bridge:
// it authorizes an UNSAVED editor buffer (the primary or an authorized nested
// WRL) against the CURRENT World scan graph, byte-substitutes it through its own
// 7C1 overlay, and installs the wrlworld:// serving context while active. The
// renderer sends only { sessionId, text, bufferVersion } -- never a path, root,
// or URL. Instantiated in whenReady alongside the editor controller.
let worldPreviewBridge = null;

// Dispatch a live-preview request to the bridge that owns the OPEN editor
// document's profile. Both bridges independently re-verify the session and
// context, so this is routing, not authorization.
function activePreviewBridge() {
  const d = editorController ? editorController.describe() : { open: false };
  return d.open && d.context === 'world' ? worldPreviewBridge : mallPreviewBridge;
}

// Combined leak-assertion view across both preview bridges (QA gate: zero
// overlays / zero active generations after close, on both profiles).
function combinedPreviewLeak() {
  const m = mallPreviewBridge ? mallPreviewBridge.leak() : { size: 0, activeGenerations: 0 };
  const w = worldPreviewBridge ? worldPreviewBridge.leak() : { size: 0, activeGenerations: 0 };
  return {
    size: m.size + w.size,
    activeGenerations: m.activeGenerations + w.activeGenerations,
    mall: m,
    world: w,
  };
}

// Renderer pages the app may navigate between (whitelist -- never a path from
// the renderer). Both share the one BrowserWindow, preload, and security config.
const APP_PAGES = { mall: 'index.html', world: 'world.html', editor: 'editor.html' };
let currentPage = 'mall';

// Navigate the one window to a whitelisted local page and await its load. Used
// by both the interactive nav buttons and the capture-server QA harness so the
// same reused window can screenshot either workspace.
async function gotoPage(page) {
  const file = APP_PAGES[page];
  if (!file || !mainWindow) return;
  if (currentPage === page) return;
  await mainWindow.loadFile(path.join(__dirname, 'renderer', file));
  currentPage = page;
}

function waitForCurrentPageLoad() {
  if (!mainWindow || !mainWindow.webContents.isLoadingMainFrame()) return Promise.resolve();
  return new Promise((resolve) => mainWindow.webContents.once('did-finish-load', resolve));
}

async function openMallFileFromDesktop(mallPath) {
  if (!mainWindow || !mallPath) return;
  if (currentPage !== 'mall') await gotoPage('mall');
  await waitForCurrentPageLoad();
  const data = openMallFile(mallPath);
  await mainWindow.webContents.executeJavaScript(
    `window.__wrlForgeApplyOpen(${JSON.stringify(data)})`,
  );
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.show();
  mainWindow.focus();
}

function queueDesktopOpen(mallPath) {
  if (!mallPath) return;
  pendingOpenFile = mallPath;
  if (!app.isReady() || !mainWindow) return;
  const requested = pendingOpenFile;
  pendingOpenFile = null;
  // Serialize startup, second-instance, and macOS open-file events. The app has
  // one active Mall session, so overlapping page loads must never race and
  // leave an older request displayed after a newer one.
  desktopOpenSequence = desktopOpenSequence
    .then(() => openMallFileFromDesktop(requested))
    .catch((err) => {
      console.error('[wrl-forge] could not open desktop file:', err);
      dialog.showErrorBox('Could not open VRML file', String(err && err.message || err));
    });
}

if (hasSingleInstanceLock) {
  app.on('second-instance', (_event, argv) => {
    queueDesktopOpen(findVrmlFileArgument(argv));
  });
  app.on('open-file', (event, filePath) => {
    event.preventDefault();
    queueDesktopOpen(findVrmlFileArgument([filePath]));
  });
}

// Flatten a session scan result into a JSON-safe payload for the renderer. The
// graph is already free of Sets (referencedBy arrays); this trims wrlNodes to
// display fields and attaches the derived summary. Read-only data only.
function serializeScan(scan) {
  const g = scan.graph || { references: [], wrlNodes: [], assets: [], missing: [], caseMismatches: [], remoteRefs: [], unsafe: [], cycles: [] };
  return {
    ok: scan.status === 'ok',
    status: scan.status,
    error: scan.error || null,
    stale: !!scan.stale,
    superseded: !!scan.superseded,
    root: scan.root,
    primary: scan.primary,
    primaryGzip: !!scan.primaryGzip,
    scanMs: scan.scanMs,
    summary: summarize(scan),
    references: g.references,
    wrlNodes: g.wrlNodes.map((n) => ({ path: n.path, depth: n.depth, parent: n.parent || null, bytes: n.bytes, unreadable: n.unreadable || null, refs: n.refs || [] })),
    assets: g.assets,
    missing: g.missing,
    caseMismatches: g.caseMismatches,
    remoteRefs: g.remoteRefs,
    unsafe: g.unsafe,
    cycles: g.cycles,
    truncated: !!g.truncated,
    depthCapped: !!g.depthCapped,
  };
}

// Hard network gate for the embedded X_ITE preview: cancel every request whose
// scheme is remote/network-capable (http/https/ws/ftp/protocol-relative), so an
// authored VRML `url` can never reach the network. Local schemes (file/data/
// blob) pass through. This is enforced in addition to the renderer CSP -- a
// layered control, not a single point of failure.
function installPreviewNetworkGuard() {
  session.defaultSession.webRequest.onBeforeRequest({ urls: ['<all_urls>'] }, (details, callback) => {
    callback({ cancel: isBlockedPreviewUrl(details.url) });
  });
}

// Install the World Project preview scheme handler (Phase 4B). X_ITE resolves the
// world's nested Inline / textures against wrlworld:// URLs, so every dependency
// read comes back through here -- authorized against the production asset graph
// and confined to the project root. WRL nodes are served gzip-decompressed (X_ITE
// only ever sees plain text); assets are served as raw bytes. Nothing is written.
function installWorldPreviewProtocol() {
  protocol.handle(WORLD_PREVIEW_SCHEME, async (request) => {
    // Phase 7C3: while the editor's live World preview is active, its serving
    // context (the SAME projectRoot + graph allow-list the disk preview would
    // install, held by the world bridge) takes precedence, and an unsaved-buffer
    // overlay may substitute bytes for an ALREADY-authorized WRL node -- the
    // lookup runs inside resolveWorldRequest only AFTER root confinement and the
    // allow-list check, so overlay presence can never make a request valid. When
    // the editor preview is not active, the workspace disk preview (worldPreview,
    // set only by world:previewLoad) serves exactly as in Phase 4B.
    const live = worldPreviewBridge ? worldPreviewBridge.servingContext() : null;
    const res = resolveWorldRequest(
      live || worldPreview,
      request.url,
      live ? { overlayLookup: (abs) => worldPreviewBridge.overlayTextFor(abs) } : {},
    );
    if (res.status !== 200) {
      return new Response(res.error || 'unavailable', { status: res.status });
    }
    return new Response(res.body, { headers: { 'content-type': res.mimeType } });
  });
}
const {
  DEFAULT_WINDOW_STATE,
  windowStatePath,
  legacyWindowStatePath,
  isVisibleOnAnyDisplay,
} = require('./src/settings/window-state');

function loadWindowState() {
  try {
    const saved = JSON.parse(fs.readFileSync(windowStatePath(app.getPath('userData')), 'utf8'));
    if (saved.x != null && saved.y != null && isVisibleOnAnyDisplay(saved, screen.getAllDisplays())) return saved;
  } catch {
    // No saved state at the new (wrl-forge) path -- check the pre-rename
    // vrmlpad location before falling back to defaults.
    try {
      const legacy = JSON.parse(fs.readFileSync(legacyWindowStatePath(app.getPath('userData')), 'utf8'));
      if (legacy.x != null && legacy.y != null && isVisibleOnAnyDisplay(legacy, screen.getAllDisplays())) return legacy;
    } catch {
      // No legacy state either -- fall through to default.
    }
  }
  return { ...DEFAULT_WINDOW_STATE };
}

function saveWindowState(win) {
  const isMaximized = win.isMaximized();
  const bounds = isMaximized ? win.getNormalBounds() : win.getBounds();
  const state = { ...bounds, isMaximized };
  try {
    const statePath = windowStatePath(app.getPath('userData'));
    fs.mkdirSync(path.dirname(statePath), { recursive: true });
    fs.writeFileSync(statePath, JSON.stringify(state));
  } catch {
    // Best-effort -- losing saved window position isn't fatal.
  }
}

function createWindow() {
  const state = loadWindowState();
  const win = new BrowserWindow({
    x: state.x,
    y: state.y,
    width: state.width,
    height: state.height,
    // Cyan opaque is the primary WRL Forge identity (assets/wrl-forge-cyan.svg,
    // rasterized deterministically by `npm run build:icons`). Sets the Linux
    // window icon and the Windows dev-mode window/taskbar icon; packaged Windows
    // builds embed the .ico via electron-builder (build.win.icon).
    icon: path.join(__dirname, 'assets', 'generated', 'icons', 'runtime', 'icon.png'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  if (state.isMaximized) win.maximize();
  mainWindow = win;

  // Phase Beta 2 -- render-process-gone handler. Single-shot reload, with a
  // burst guard so a crash loop never tears the GPU down further.
  win.webContents.on('render-process-gone', (_event, details) => {
    handleRenderProcessGone(details);
  });
  // Best-effort recovery flush when the renderer navigates away. The editor
  // page already forces a final flush on Close and Back; this catches Mall /
  // World page navigations too.
  win.webContents.on('will-prevent-unload', (event) => {
    flushRecoveryBeforeUnload();
  });

  let saveTimer = null;
  const scheduleSave = () => {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveWindowState(win), 500);
  };
  win.on('resize', scheduleSave);
  win.on('move', scheduleSave);
  win.on('close', () => {
    clearTimeout(saveTimer);
    saveWindowState(win);
    flushRecoveryBeforeUnload();
  });

  win.loadFile(path.join(__dirname, 'renderer', 'index.html'));

  // Non-interactive smoke-test hook (test/electron-smoke.test.js): report a
  // single JSON line describing security-relevant window state, then quit.
  // Only active when explicitly requested -- never runs during normal use.
  if (process.env.WRL_FORGE_SMOKE_TEST) {
    win.webContents.once('did-finish-load', async () => {
      // Probe actual renderer-side behavior rather than reading back the options
      // we passed to BrowserWindow -- this verifies what's really in effect.
      const hasVrmlpadBridge = await win.webContents.executeJavaScript(
        'typeof window.vrmlpad === "object" && window.vrmlpad !== null'
      );
      // contextIsolation:true means the renderer's main world has no access to the
      // preload script's Node-derived `require`; nodeIntegration:false means the
      // renderer has no global `process`/`require` at all.
      const rendererHasRequire = await win.webContents.executeJavaScript('typeof require !== "undefined"');
      const rendererHasProcess = await win.webContents.executeJavaScript('typeof process !== "undefined"');
      // Preview-surface presence (Phase 2B1): container, X_ITE engine global,
      // Original/Fit controls, and the CSP meta tag -- all probed in the real
      // renderer, not asserted from config.
      const preview = await win.webContents.executeJavaScript(`(() => ({
        hasPreviewCanvas: !!document.getElementById('preview'),
        xiteLoaded: typeof X3D === 'function',
        hasModeControls: !!document.getElementById('modeOriginal') && !!document.getElementById('modeFit'),
        hasCspMeta: !!document.querySelector('meta[http-equiv="Content-Security-Policy"]'),
      }))()`);
      const report = {
        title: win.getTitle(),
        hasVrmlpadBridge,
        contextIsolation: !rendererHasRequire,
        nodeIntegration: rendererHasProcess,
        ...preview,
      };
      console.log('WRL_FORGE_SMOKE_TEST_RESULT ' + JSON.stringify(report));
      app.quit();
    });
  }

  // Persistent capture-server mode: ONE long-lived Electron process that drives
  // many fixtures/screenshots through a single reused window, one command at a
  // time, instead of relaunching the app per capture. This is the ONLY sanctioned
  // path for visual QA batches (see qa/visual-qa/ and docs/VISUAL_QA_SAFETY.md) --
  // it is what prevents an Electron launch storm from ever taking down the
  // compositor again. Commands arrive as newline-delimited JSON on stdin:
  //   { id, fixture, mode?:'fit'|'original', size?:'WxH', out?:pngPath, json?:bool }
  //   { cmd:'shutdown' }
  // and each produces exactly one response line (READY once, then OK/ERR per id).
  // Security posture is unchanged: same window, same preload, same IPC surface.
  if (process.env.WRL_FORGE_CAPTURE_SERVER) {
    const SETTLE_MS = Number(process.env.WRL_FORGE_SETTLE_MS || 1200);
    const emit = (line) => process.stdout.write(line + '\n');
    // Process one job end-to-end against the reused window.
    async function runJob(job) {
      if (job.size) {
        const [w, h] = String(job.size).split('x').map(Number);
        if (w && h) win.setSize(w, h);
      }
      const payload = {};

      // Native editor job (Phase 7B): drive the real editor lane in the SAME
      // reused window. Main points its context (currentSession / worldSession) at
      // a SCRATCH fixture confined to the OS temp dir, opens it through the real
      // editorController, navigates to the editor page, optionally runs scripted
      // steps (type/save/click-diagnostic/outline/external), stages an external
      // change to raise the conflict dialog, then screenshots. Reachable only
      // under WRL_FORGE_CAPTURE_SERVER; never touches a real project file.
      if ('editor' in job) {
        const e = job.editor || {};
        const os = require('os');
        const inTemp = (p) => path.resolve(String(p || '')).startsWith(path.resolve(os.tmpdir()));
        // WD2-B: optionally record every renderer console message for this job
        // (the runtime gate is "no unexpected console error/warning").
        let consoleLog = null;
        const onConsole = (ev, level, message) => {
          const lv = ev && ev.level !== undefined ? ev.level : level;
          const msg = ev && ev.message !== undefined ? ev.message : message;
          const name = typeof lv === 'number' ? ['verbose', 'info', 'warning', 'error'][lv] || String(lv) : String(lv);
          consoleLog.push({ level: name, message: String(msg) });
        };
        if (e.captureConsole) {
          consoleLog = [];
          win.webContents.on('console-message', onConsole);
        }

        // Establish the lane context so editorController resolves the source.
        if (e.context === 'world') {
          if (!inTemp(e.root)) throw new Error('editor world root must be under the OS temp dir');
          if (typeof e.writeSource === 'string') {
            if (!inTemp(e.primary)) throw new Error('editor writeSource refused outside the OS temp dir');
            fs.writeFileSync(e.primary, e.writeSource, 'utf8');
          }
          worldSession.open({ root: e.root, primary: e.primary, candidates: [{ path: e.primary }] });
          await worldSession.scan();
          if (e.ref) editorController.openWorldReference(e.ref);
          else editorController.openWorldPrimary();
        } else {
          if (!inTemp(e.mallPath)) throw new Error('editor mallPath must be under the OS temp dir');
          if (typeof e.writeSource === 'string') {
            const buf = e.gzip ? zlib.gzipSync(Buffer.from(e.writeSource, 'utf8'), { level: 9 }) : Buffer.from(e.writeSource, 'utf8');
            fs.writeFileSync(e.mallPath, buf);
          }
          currentSession = { mallPath: e.mallPath, editFile: e.mallPath };
          editorController.openMall();
        }

        // Force a FRESH load of the editor page every job (gotoPage would no-op
        // when already on 'editor', leaving the previous job's document shown).
        // Production never needs this -- a user always leaves the editor page
        // before returning to it -- but the batch harness reopens it back-to-back.
        await mainWindow.loadFile(path.join(__dirname, 'renderer', 'editor.html'));
        currentPage = 'editor';
        // Wait for the editor page to mount its CodeMirror surface (or show the
        // no-document message), bounded so a stuck load can't hang the run.
        await win.webContents.executeJavaScript(`(async () => {
          for (let i = 0; i < 60; i++) {
            if (window.__wrlEditor && window.__wrlEditor.ready()) return true;
            await new Promise((r) => setTimeout(r, 100));
          }
          return false;
        })()`);
        await new Promise((r) => setTimeout(r, SETTLE_MS));

        // Optional theme selection (visual-QA showcase of the built-in themes).
        if (e.theme) {
          await win.webContents.executeJavaScript(`window.__wrlEditor.setTheme(${JSON.stringify(e.theme)})`);
          await new Promise((r) => setTimeout(r, 400));
        }

        // Optional zoom level (visual-QA of the vision-accommodation scaling).
        if (e.zoom != null) {
          await win.webContents.executeJavaScript(`window.__wrlEditor.setZoom(${JSON.stringify(e.zoom)})`);
          await new Promise((r) => setTimeout(r, 400));
        }

        // Optional scripted step, driven through the page's own QA hook.
        if (e.step === 'type') {
          await win.webContents.executeJavaScript(`window.__wrlEditor.setText(${JSON.stringify(e.text || '')})`);
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        } else if (e.step === 'save') {
          await win.webContents.executeJavaScript(`window.__wrlEditor.setText(${JSON.stringify(e.text || '')})`);
          await new Promise((r) => setTimeout(r, 400));
          await win.webContents.executeJavaScript(`window.__wrlEditor.click('saveBtn')`);
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        } else if (e.step === 'diagnostic') {
          await win.webContents.executeJavaScript(`window.__wrlEditor.clickFirst('#diagList .row-item')`);
          await new Promise((r) => setTimeout(r, 400));
        } else if (e.step === 'outline') {
          await win.webContents.executeJavaScript(`window.__wrlEditor.clickFirst('#outlineList .row-item')`);
          await new Promise((r) => setTimeout(r, 400));
        } else if (e.step === 'external') {
          await win.webContents.executeJavaScript(`window.__wrlEditor.click('externalBtn')`);
          await new Promise((r) => setTimeout(r, 400));
        } else if (e.step === 'modal') {
          // Open the in-DOM go-to-line modal so it can be captured (e.g. scaled).
          await win.webContents.executeJavaScript(`window.__wrlEditor.click('gotoBtn')`);
          await new Promise((r) => setTimeout(r, 400));
        } else if (e.step === 'conflict') {
          // Type an edit, stage an EXTERNAL change to the same scratch source,
          // then attempt Save -> the renderer raises the Reload/Save As/Cancel dialog.
          const target = e.context === 'world' ? (e.ref || e.primary) : e.mallPath;
          await win.webContents.executeJavaScript(`window.__wrlEditor.setText(${JSON.stringify(e.text || '# mine\\n')})`);
          await new Promise((r) => setTimeout(r, 300));
          if (inTemp(target)) {
            const raw = fs.readFileSync(target);
            const isGz = raw[0] === 0x1f && raw[1] === 0x8b;
            const bytes = Buffer.from('#VRML V2.0 utf8\n# externally changed\n', 'utf8');
            fs.writeFileSync(target, isGz ? zlib.gzipSync(bytes, { level: 9 }) : bytes);
          }
          await win.webContents.executeJavaScript(`window.__wrlEditor.click('saveBtn')`);
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        }

        // Phase 7C2 live-preview steps (Mall editor only). These drive the SAME
        // preview controls the user uses, through the page's own __wrlEditor hooks
        // -- no new capability. The preview authorizes against currentSession
        // (the scratch mallPath set above), so it renders the unsaved buffer.
        if (e.previewLayout) {
          // Full settle: entering a visible layout from a persisted hidden one
          // triggers the first render, which must complete before later steps.
          await win.webContents.executeJavaScript(`window.__wrlEditor.previewLayout(${JSON.stringify(e.previewLayout)})`);
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        }
        if (e.fitMode) {
          await win.webContents.executeJavaScript(`window.__wrlEditor.fitMode(${JSON.stringify(e.fitMode)})`);
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        }
        if (e.previewStepSplit) {
          await win.webContents.executeJavaScript(`window.__wrlEditor.previewStepSplit(${JSON.stringify(e.previewStepSplit)})`);
          await new Promise((r) => setTimeout(r, 400));
        }
        if (e.previewStep === 'update') {
          await win.webContents.executeJavaScript('window.__wrlEditor.previewUpdate()');
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        } else if (e.previewStep === 'saved') {
          await win.webContents.executeJavaScript('window.__wrlEditor.previewSaved()');
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        } else if (e.previewStep === 'maximize') {
          await win.webContents.executeJavaScript('window.__wrlEditor.previewMaximize()');
          await new Promise((r) => setTimeout(r, 500));
        } else if (e.previewStep === 'findnew') {
          // World only: the explicit Find-new-files action (rescan + fresh
          // Update). Longer settle -- a rescan plus an X_ITE render follows.
          await win.webContents.executeJavaScript('window.__wrlEditor.previewFindNew()');
          await new Promise((r) => setTimeout(r, SETTLE_MS * 2));
        }
        // Phase 7C3 World live-preview view steps: select a viewpoint / set the
        // navigation mode through the page's own controls (after the initial
        // render, before any text2 edit triggers a preserving refresh).
        if (e.previewViewpoint != null) {
          await win.webContents.executeJavaScript(`window.__wrlEditor.previewViewpoint(${JSON.stringify(e.previewViewpoint)})`);
          await new Promise((r) => setTimeout(r, 600));
        }
        if (e.previewNav) {
          await win.webContents.executeJavaScript(`window.__wrlEditor.previewNav(${JSON.stringify(e.previewNav)})`);
          await new Promise((r) => setTimeout(r, 400));
        }
        // A second edit (used for the temporary-syntax-error / recovery captures).
        if (typeof e.text2 === 'string') {
          await win.webContents.executeJavaScript(`window.__wrlEditor.setText(${JSON.stringify(e.text2)})`);
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        }

        // WD2-B scripted hook sequence: each step calls ONE existing
        // window.__wrlEditor hook by name (identifier-validated, must already be
        // a function on the page) with JSON args, records its JSON result, and
        // optionally captures the window. No arbitrary script is evaluated.
        if (Array.isArray(e.steps)) {
          payload.steps = [];
          for (const st of e.steps) {
            if (st && typeof st.capture === 'string') {
              const shot = await win.webContents.capturePage();
              fs.writeFileSync(st.capture, shot.toPNG());
              payload.steps.push({ capture: st.capture });
            } else if (st && typeof st.call === 'string' && /^[A-Za-z][A-Za-z0-9]*$/.test(st.call)) {
              const args = JSON.stringify(Array.isArray(st.args) ? st.args : []);
              const raw = await win.webContents.executeJavaScript(`(async () => {
                const h = window.__wrlEditor;
                if (!h || typeof h[${JSON.stringify(st.call)}] !== 'function') throw new Error('no __wrlEditor hook ' + ${JSON.stringify(st.call)});
                const v = await h[${JSON.stringify(st.call)}](...${args});
                return JSON.stringify(v === undefined ? null : v);
              })()`);
              payload.steps.push({ call: st.call, args: st.args || [], label: st.label || null, result: JSON.parse(raw) });
            } else {
              throw new Error('editor steps: each step needs a hook name (call) or a capture path');
            }
            if (st.wait) await new Promise((r) => setTimeout(r, Number(st.wait) || 0));
          }
        }

        const status = await win.webContents.executeJavaScript(
          '(window.__wrlEditor && window.__wrlEditor.status) ? JSON.stringify(window.__wrlEditor.status()) : "null"'
        );
        payload.editor = JSON.parse(status);
        const pv = await win.webContents.executeJavaScript(
          '(window.__wrlEditor && window.__wrlEditor.previewState) ? JSON.stringify(window.__wrlEditor.previewState()) : "null"'
        );
        payload.preview = JSON.parse(pv);
        if (job.out) {
          const img = await win.webContents.capturePage();
          fs.writeFileSync(job.out, img.toPNG());
          payload.out = job.out;
        }
        // Leak assertion: after closing the editor session, no overlay/generation
        // may remain for it on EITHER preview bridge (7C2 + 7C3 cleanup gate).
        if (e.leakAfterClose) {
          const sid = editorController.describe().sessionId;
          if (sid != null) {
            mallPreviewBridge.invalidateSession(sid);
            worldPreviewBridge.invalidateSession(sid);
          }
          payload.leak = combinedPreviewLeak();
        }
        if (consoleLog) {
          win.webContents.removeListener('console-message', onConsole);
          payload.console = consoleLog;
        }
        return payload;
      }

      // World Project job (Phase 4A): drive the read-only World workspace across
      // a sequence of states in the SAME reused window. `world` may be null (the
      // empty-project state) or { root, primary } (a scratch project the trusted
      // main process points the confined scan at -- the renderer never supplies
      // this). No mutation: scanning only reads.
      if ('world' in job) {
        await gotoPage('world');
        if (job.world) {
          // QA-only: swap the SCRATCH primary's bytes before scanning, so the
          // parse-fail->recover sequence can be driven inside ONE reused process
          // without mid-run file edits from the orchestrator. Confined to the OS
          // temp dir -- this never touches a real project file, and is reachable
          // only under WRL_FORGE_CAPTURE_SERVER (never in normal use).
          if (typeof job.world.writePrimary === 'string') {
            const os = require('os');
            const scratch = path.resolve(job.world.primary);
            if (scratch.startsWith(path.resolve(os.tmpdir()))) {
              fs.writeFileSync(scratch, job.world.writePrimary, 'utf8');
            } else {
              throw new Error('writePrimary refused outside the OS temp dir');
            }
          }
          worldSession.open({ root: job.world.root, primary: job.world.primary, candidates: [{ path: job.world.primary }] });
          const scan = await worldSession.scan();
          const ser = serializeScan(scan);
          await win.webContents.executeJavaScript(`window.__wrlForgeApplyWorld(${JSON.stringify(ser)})`);
          payload.summary = ser.summary;
          // Drive the embedded X_ITE world preview through the real read-only
          // world:previewLoad path + confined wrlworld:// handler.
          if (job.preview) {
            const opts = JSON.stringify({ viewpoint: job.viewpoint, reset: !!job.reset });
            const dbg = await win.webContents.executeJavaScript(
              `(async () => JSON.stringify(await window.__wrlForgeWorldPreview(${opts})))()`
            );
            payload.preview = JSON.parse(dbg);
          }
          // Drive the read-only PACKAGE AUDIT display (Phase 5A).
          if (job.packageAudit || job.buildBundle) {
            const plan = buildPackagePlan(scan);
            const audit = {
              ok: plan.ok, status: plan.status, projectName: plan.projectName,
              primaryRel: plan.primaryRel, totals: plan.totals, blocking: plan.blocking,
              findings: plan.findings, unusedFiles: plan.unusedFiles, files: plan.files,
              manifest: buildManifest(plan), label: plan.label, disclaimer: plan.disclaimer,
            };
            await win.webContents.executeJavaScript(
              `window.__wrlForgeApplyPackageAudit(${JSON.stringify(audit)});` +
              `(document.getElementById('pkgStatusBadge')||{}).scrollIntoView&&document.getElementById('pkgStatusBadge').scrollIntoView({block:'start'});`
            );
            payload.packageAudit = { status: plan.status, totals: plan.totals,
              blocking: plan.blocking.map((b) => b.code), unused: plan.unusedFiles.length };
            // QA-only bundle build: writes ONLY under the OS temp dir (never a
            // real destination, never inside the project), mirroring writePrimary's
            // confinement. Reachable only under WRL_FORGE_CAPTURE_SERVER.
            if (job.buildBundle && plan.status !== 'blocked') {
              const os = require('os');
              const dest = path.resolve(String(job.buildBundle));
              if (!dest.startsWith(path.resolve(os.tmpdir()))) {
                throw new Error('buildBundle refused outside the OS temp dir');
              }
              try {
                payload.bundle = writeReviewBundle(scan, dest, { plan });
                await win.webContents.executeJavaScript(
                  `window.__wrlForgeApplyBundleResult(${JSON.stringify({ ok: true, outPath: payload.bundle.outPath, bytes: payload.bundle.bytes, entryCount: payload.bundle.entryCount })})`
                );
              } catch (err) {
                payload.bundleError = String(err.message || err);
                await win.webContents.executeJavaScript(
                  `window.__wrlForgeApplyBundleResult(${JSON.stringify({ ok: false, error: payload.bundleError })})`
                );
              }
            }
          }
        } else {
          worldSession.open({ root: null, primary: null, candidates: [], empty: true });
          worldPreview = null;
          await win.webContents.executeJavaScript('window.__wrlForgeResetWorld && window.__wrlForgeResetWorld()');
        }
        await new Promise((r) => setTimeout(r, SETTLE_MS));
        if (job.out) {
          const img = await win.webContents.capturePage();
          fs.writeFileSync(job.out, img.toPNG());
          payload.out = job.out;
        }
        return payload;
      }

      // JSON-only job: authoritative bounds/fit debug via the READ-ONLY preview
      // channel -- identical to the WRL_FORGE_PREVIEW_FIXTURE harness (points
      // currentSession at the fixture, never writes a .edit.wrl sibling), just
      // reused across many fixtures in one process instead of one spawn each.
      if (job.json && !job.out) {
        currentSession = { mallPath: job.fixture, editFile: job.fixture };
        const dbg = await win.webContents.executeJavaScript(
          '(async () => { await window.wrlPreview.load(); return JSON.stringify(window.wrlPreview._debug()); })()'
        );
        payload.debug = JSON.parse(dbg);
        return payload;
      }

      // Screenshot job: the REAL open->validate->preview flow (as in the app),
      // which produces a .edit.wrl -- callers point this at scratch copies.
      if (job.fixture) {
        await win.webContents.executeJavaScript(
          `(async () => { const d = await window.vrmlpad.openMallPath(${JSON.stringify(job.fixture)}); window.__wrlForgeApplyOpen(d); })()`
        );
        await new Promise((r) => setTimeout(r, SETTLE_MS));
        if (job.mode === 'fit') {
          await win.webContents.executeJavaScript("document.getElementById('modeFit').click()");
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        } else if (job.mode === 'original') {
          await win.webContents.executeJavaScript("document.getElementById('modeOriginal').click()");
          await new Promise((r) => setTimeout(r, SETTLE_MS));
        }
      }
      if (job.out) {
        const img = await win.webContents.capturePage();
        fs.writeFileSync(job.out, img.toPNG());
        payload.out = job.out;
      }
      // Keyboard interaction job (final QA enablement pass): dispatch real
      // KeyboardEvents to the packaged Electron window. Two paths:
      //   - `press` / `keydown` / `keyup` use Electron's native
      //     webContents.sendInputEvent (this drives OS-level focus traversal
      //     for Tab, Escape, etc. AND fires keydown listeners on the
      //     renderer side via the Chromium input pipeline).
      //   - `inspectFocus` / `goto` use document-level executeJavaScript.
      // Both are reachable only under WRL_FORGE_CAPTURE_SERVER.
      if (job.keyboard) {
        const kb = job.keyboard || {};
        if (kb.kind === 'inspectFocus') {
          await new Promise((r) => setTimeout(r, kb.delayMs || 60));
          const focus = await win.webContents.executeJavaScript(
            `(function(){var a=document.activeElement;if(!a||a===document.body)return{tag:'BODY',focusable:false};var r=a.getBoundingClientRect?a.getBoundingClientRect():null;return{tag:a.tagName,id:a.id||null,cls:a.className||null,role:a.getAttribute&&a.getAttribute('role'),ariaLabel:a.getAttribute&&a.getAttribute('aria-label'),text:(a.innerText||'').slice(0,80),rect:r?{x:r.x,y:r.y,w:r.width,h:r.height}:null,focusable:true};})()`
          );
          payload.focus = focus;
          return payload;
        }
        if (kb.kind === 'goto') {
          if (kb.page === 'editor') {
            await mainWindow.loadFile(path.join(__dirname, 'renderer', 'editor.html'));
            currentPage = 'editor';
          } else if (kb.page === 'world') {
            await mainWindow.loadFile(path.join(__dirname, 'renderer', 'world.html'));
            currentPage = 'world';
          } else {
            await mainWindow.loadFile(path.join(__dirname, 'renderer', 'index.html'));
            currentPage = 'mall';
          }
          await new Promise((r) => setTimeout(r, kb.delayMs || 600));
          return payload;
        }
        if (kb.kind === 'focus') {
          // Programmatic focus: simulates where Tab navigation would arrive
          // without depending on OS-level focus traversal.
          const focusRes = await win.webContents.executeJavaScript(
            `(function(){var s=${JSON.stringify(kb.selector || '')};if(!s)return{ok:false,err:'no selector'};var el=document.querySelector(s);if(!el)return{ok:false,err:'not found',sel:s};el.focus();var a=document.activeElement;return{ok:true,focusedId:a&&a.id,focusedTag:a&&a.tagName,sel:s};})()`
          );
          payload.focusRes = focusRes;
          await new Promise((r) => setTimeout(r, kb.delayMs || 120));
          return payload;
        }
        if (kb.kind === 'inspect-recovery' || kb.kind === 'inspect-recovery-prompt') {
          const r = await win.webContents.executeJavaScript(
            `(async function(){var a=document.activeElement;var dlg=document.getElementById('wrlforgeRecoveryRoot');var visible=!!(dlg&&(dlg.offsetWidth>0||dlg.offsetHeight>0));var found={ok:false,found:false};try{var bridge=window.vrmlpad&&window.vrmlpad.editor;if(bridge&&bridge.recoveryRead){var r=await bridge.recoveryRead();if(r&&r.found){found={ok:true,found:true,activeWorkspace:(r.record&&r.record.activeWorkspace),sourcePath:(r.record&&r.record.sourcePath)}else{found={ok:true,found:false,reason:r&&r.reason};}else{found={ok:false,err:'no bridge'};}}catch(e){found={ok:false,err:String(e)};}return{hasDialog:!!dlg,visible,activeId:a&&a.id,activeTag:a&&a.tagName,recoveryBridge:found};})()`
          );
          payload.recovery = r;
          return payload;
        }
        if (kb.kind === 'executeJS') {
          const r = await win.webContents.executeJavaScript(kb.code || 'null');
          payload.executeJS = r;
          return payload;
        }
        if (kb.kind === 'fire-keydown') {
          // Direct render-side dispatch (browser-only KeyboardEvent). Bypasses
          // Electron's sendInputEvent validation. Used for testing renderer-side
          // keyboard handlers; does NOT drive system-level Tab/Escape focus.
          const r = await win.webContents.executeJavaScript(
            `(function(){var e=new KeyboardEvent('keydown',{key:${JSON.stringify(kb.key || 'Tab')},code:${JSON.stringify(kb.code || kb.key || 'Tab')},ctrlKey:${JSON.stringify((kb.modifiers||[]).includes('control'))},shiftKey:${JSON.stringify((kb.modifiers||[]).includes('shift'))},altKey:${JSON.stringify((kb.modifiers||[]).includes('alt'))},metaKey:${JSON.stringify((kb.modifiers||[]).includes('meta'))},bubbles:true,cancelable:true});var t=document.activeElement||document.body;var d=t.dispatchEvent?t.dispatchEvent(e):false;return{dispatched:!!d,target:t.tagName||'BODY',activeId:t.id||null};})()`
          );
          payload.fireKeydown = r;
          return payload;
        }
        // press / keydown / keyup -- Electron native sendInputEvent.
        // Per Electron docs, `keyCode` is a STRING (Electron Accelerator key
        // code), NOT a Chromium virtual-key integer. Examples: 'Tab',
        // 'R', 'E', 'Escape', 'Return', 'Space'. The Accelerator parser
        // accepts single-letter strings for A-Z and 0-9, and the names for
        // special keys. The BrowserWindow must be focused before the event
        // is delivered or it is silently dropped.
        const FOCUSABLE_NAMES = new Set([
          'Tab','Return','Enter','Escape','Backspace','Space','Delete',
          'Insert','Home','End','PageUp','PageDown','ArrowUp','ArrowDown',
          'ArrowLeft','ArrowRight','F1','F2','F3','F4','F5','F6','F7','F8','F9','F10','F11','F12',
          'Plus','Minus','Multiply','Divide','Decimal','Separator'
        ]);
        let accel = kb.code || kb.key;
        if (accel && accel.length === 1) accel = accel.toUpperCase();
        if (!FOCUSABLE_NAMES.has(accel) && !/^[A-Z]$|^Digit[0-9]$/.test(accel)) {
          throw new Error('unsupported keyCode for sendInputEvent: ' + accel);
        }
        // Establish application focus (BrowserWindow.show + focus), which is
        // required for OS-level input to be delivered to the renderer.
        try { win.show(); win.focus(); } catch {}
        const modifiers = Array.isArray(kb.modifiers) ? kb.modifiers.map((m) => String(m).toLowerCase()) : [];
        if (kb.kind !== 'keyup') win.webContents.sendInputEvent({ type: 'keyDown', keyCode: accel, modifiers });
        if (kb.kind !== 'keydown') win.webContents.sendInputEvent({ type: 'keyUp', keyCode: accel, modifiers });
        await new Promise((r) => setTimeout(r, kb.delayMs || 120));
      }
      return payload;
    }
    // Serialize commands: exactly one job runs at a time inside the process too,
    // so a burst of stdin lines can never open overlapping renders.
    let chain = Promise.resolve();
    const enqueue = (job) => {
      chain = chain.then(async () => {
        if (job && job.cmd === 'shutdown') { app.quit(); return; }
        try {
          const payload = await runJob(job);
          emit('WRL_FORGE_CAPTURE_OK ' + job.id + ' ' + JSON.stringify(payload));
        } catch (err) {
          emit('WRL_FORGE_CAPTURE_ERR ' + (job && job.id) + ' ' + String((err && err.message) || err));
        }
      });
    };
    win.webContents.once('did-finish-load', async () => {
      // Windows transport (Phase 7C5): a GUI-subsystem electron.exe gets an
      // immediately-ended process.stdin, so stdin-delivered jobs never arrive and
      // a readline 'close' would quit the server at once. When a jobs FILE is
      // provided, read the whole batch from it, run each through the SAME runJob,
      // emit one result line per job on stdout (stdout works fine), then quit.
      // stdout/capturePage/WebGL are unaffected -- only stdin is broken on Windows.
      // See docs/WINDOWS_NATIVE_QA_PLAN.md Sec.5.
      const jobsFile = process.env.WRL_FORGE_CAPTURE_JOBS_FILE;
      if (jobsFile) {
        emit('WRL_FORGE_CAPTURE_READY');
        let batch = [];
        try {
          // BOM tolerance: PowerShell `Out-File -Encoding utf8` (PS 5.1 default)
          // writes UTF-8 with BOM. Strip a single leading U+FEFF before parsing
          // so the QA-tier-3 jobs file round-trips. Mirrors qa/visual-qa/json-file.js.
          let text = fs.readFileSync(jobsFile, 'utf8');
          if (text.charCodeAt(0) === 0xFEFF) text = text.slice(1);
          batch = JSON.parse(text);
        } catch { emit('WRL_FORGE_CAPTURE_ERR - bad-jobs-file'); }
        for (const job of batch) {
          if (job && job.cmd === 'shutdown') break;
          try {
            const payload = await runJob(job);
            emit('WRL_FORGE_CAPTURE_OK ' + job.id + ' ' + JSON.stringify(payload));
          } catch (err) {
            emit('WRL_FORGE_CAPTURE_ERR ' + (job && job.id) + ' ' + String((err && err.message) || err));
          }
        }
        app.quit();
        return;
      }
      // POSIX transport: newline-delimited JSON jobs over stdin.
      const rl = require('readline').createInterface({ input: process.stdin });
      rl.on('line', (raw) => {
        const line = raw.trim();
        if (!line) return;
        let job;
        try { job = JSON.parse(line); } catch { emit('WRL_FORGE_CAPTURE_ERR - bad-json'); return; }
        enqueue(job);
      });
      // If the orchestrator's stdin closes (it died/was killed), never linger as
      // an orphan holding the GPU -- exit promptly.
      rl.on('close', () => app.quit());
      emit('WRL_FORGE_CAPTURE_READY');
    });
    return;
  }

  // Non-interactive QA screenshot harness: drive the REAL open->validate->preview
  // path against a (scratch) fixture, optionally switch to Fit mode, capture the
  // page to a PNG, then quit. Takes precedence over the JSON harness below.
  // NOTE: this single-shot path is retained for ad-hoc one-off captures only;
  // batch/visual-QA runs MUST use the capture-server above via qa/visual-qa/.
  if (process.env.WRL_FORGE_PREVIEW_CAPTURE) {
    const fixture = process.env.WRL_FORGE_PREVIEW_FIXTURE;
    const outPath = process.env.WRL_FORGE_PREVIEW_CAPTURE;
    const captureMode = process.env.WRL_FORGE_PREVIEW_MODE;
    win.webContents.once('did-finish-load', async () => {
      try {
        if (process.env.WRL_FORGE_WIN_SIZE) {
          const [w, h] = process.env.WRL_FORGE_WIN_SIZE.split('x').map(Number);
          if (w && h) win.setSize(w, h);
        }
        if (fixture) {
          await win.webContents.executeJavaScript(
            `(async () => { const d = await window.vrmlpad.openMallPath(${JSON.stringify(fixture)}); window.__wrlForgeApplyOpen(d); })()`
          );
          await new Promise((r) => setTimeout(r, 1400));
          if (captureMode === 'fit') {
            await win.webContents.executeJavaScript("document.getElementById('modeFit').click()");
            await new Promise((r) => setTimeout(r, 1600));
          }
        }
        const img = await win.webContents.capturePage();
        fs.writeFileSync(outPath, img.toPNG());
        console.log('WRL_FORGE_PREVIEW_CAPTURE_DONE ' + outPath);
      } catch (err) {
        console.log('WRL_FORGE_PREVIEW_CAPTURE_ERROR ' + String(err && err.stack || err));
      }
      app.quit();
    });
    return;
  }

  // Non-interactive preview harness (test/electron-preview.test.js): point the
  // read-only preview channel at a specific fixture, drive a real X_ITE load +
  // authoritative bounds/fit computation in the renderer, print the result as
  // one JSON line, then quit. Only active when explicitly requested.
  if (process.env.WRL_FORGE_PREVIEW_FIXTURE) {
    const fixture = process.env.WRL_FORGE_PREVIEW_FIXTURE;
    currentSession = { mallPath: fixture, editFile: fixture };
    win.webContents.once('did-finish-load', async () => {
      let line;
      try {
        const json = await win.webContents.executeJavaScript(
          '(async () => { await window.wrlPreview.load(); return JSON.stringify(window.wrlPreview._debug()); })()'
        );
        line = 'WRL_FORGE_PREVIEW_RESULT ' + json;
      } catch (err) {
        line = 'WRL_FORGE_PREVIEW_RESULT ' + JSON.stringify({ error: String(err && err.stack || err) });
      }
      console.log(line);
      app.quit();
    });
  }
}

// The set of absolute WRL-node paths the current world scan discovered (readable
// only). This is the editor's open allow-list for "Open a referenced WRL": only a
// real dependency node may be opened, never an arbitrary in-root file.
function worldWrlSet() {
  const set = new Set();
  const scan = worldSession.last;
  if (scan && scan.graph) {
    for (const n of scan.graph.wrlNodes || []) {
      if (!n.unreadable) set.add(path.resolve(n.path));
    }
  }
  return set;
}

// Build the native editor controller, injecting the live context providers and
// the main-owned dialog/launcher. Keeps EditorController free of Electron so its
// authorization/stale/conflict/restore logic stays unit-tested.
function createEditorController() {
  return new EditorController({
    userDataPath: app.getPath('userData'),
    getMallSource: () => (currentSession ? currentSession.mallPath : null),
    getWorldContext: () => {
      if (!worldSession.primary) return null;
      return { root: worldSession.root, primary: worldSession.primary, allowedWrl: worldWrlSet() };
    },
    launchExternal: (filePath) => launchEditor(filePath),
    promptSaveAs: async ({ defaultPath }) => {
      const res = await dialog.showSaveDialog(mainWindow, {
        title: 'Save WRL As',
        defaultPath: defaultPath || undefined,
        filters: [
          { name: 'VRML files', extensions: ['wrl', 'wrz'] },
          { name: 'All files', extensions: ['*'] },
        ],
        properties: ['createDirectory', 'showOverwriteConfirmation'],
      });
      return (res.canceled || !res.filePath) ? null : res.filePath;
    },
  });
}

// Phase Beta 2 -- the recovery controller. Receives the editor controller
// through `openSession` so adopting a snapshot reuses the regular open path
// (which already handles gzip/format/stat). It only handles the recovery
// file; session persistence lives in session-store, which is unchanged.
//
// editorController is created FIRST; then we patch its recoveryController
// reference back in so EditorController.save() can call recordClear() on
// success through the standard controller surface (no extra IPC-aware
// wrapper code).
//
// Phase Beta 2 QA pass 2 (B1): `getSourceStat` is the live on-disk stat of
// the held session. Capturing this at snapshot time lets the recovery
// record carry the actual hash/size of the file (gzip or plain), which is
// what the existing safe-save conflict path compares against. Synthesizing
// a stat from decompressed text is unsafe for gzip sources.
function createRecoveryController() {
  const rc = new RecoveryController({
    userDataPath: app.getPath('userData'),
    openSession: (recovered) => editorController.openFromRecovery(recovered),
    getSourceStat: () => {
      const ecSession = editorController && editorController.session;
      if (!ecSession || !ecSession.doc) return null;
      const stat = ecSession.doc.stat;
      if (!stat || typeof stat !== 'object') return null;
      return stat;
    },
    log: (entry) => {
      try { console.warn('[wrl-forge][recovery]', JSON.stringify(entry)); } catch { /* noop */ }
    },
  });
  if (editorController) editorController.recoveryController = rc;
  return rc;
}

if (hasSingleInstanceLock) app.whenReady().then(() => {
  installPreviewNetworkGuard();
  installWorldPreviewProtocol();
  editorController = createEditorController();
  recoveryController = createRecoveryController();
  mallPreviewBridge = createMallPreviewBridge({
    // The renderer never supplies a path: the bridge resolves the editor session
    // itself and confirms the held source equals the authorized Mall item.
    describeSession: () => editorController.describe(),
    getAuthorizedMallSource: () => (currentSession ? currentSession.mallPath : null),
    scanRemoteUrls,
    readSaved: (absPath) => readWrlSource(absPath),
  });
  worldPreviewBridge = createWorldPreviewBridge({
    // The renderer never supplies a path: the bridge resolves the editor session
    // itself and authorizes the held document against the CURRENT scan graph
    // (root match, graph membership, exact case, realpath confinement).
    describeSession: () => editorController.describe(),
    getWorldRoot: () => worldSession.root,
    getWorldPrimary: () => worldSession.primary,
    getScan: () => worldSession.last,
    rescan: () => worldSession.scan(),
    readSaved: (absPath) => readWrlSource(absPath),
  });
  createWindow();
  if (pendingOpenFile) queueDesktopOpen(pendingOpenFile);
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

// Phase Beta 2 -- render-process-gone. The renderer killed itself (a segfault,
// an OOM, a debugger hard-quit). Phase 7D0 evidence notes main logged the event
// but did nothing; Phase Beta 2 makes it actionable without creating a crash
// loop:
//   * Single-shot reload -- the renderer's page is reloaded once via
//     webContents.reload. A second within a short window is treated as
//     repeated failure and we DO NOT keep restarting the renderer.
//   * Recovery state survives -- main did not lose anything; the renderer
//     re-mounts and re-reads its state from the same session. Any pending
//     debounced recovery flush is force-flushed BEFORE the reload so the
//     new renderer can find the freshest snapshot on boot.
//   * The session is NEVER silently restored -- recovery remains a
//     user-decided action; we only make sure the new renderer can ask.
//
// A second event in the same "burst window" (default 10s) is logged and the
// reload is suppressed -- a hard crash loop is NEVER papered over; the user
// gets the rest of the app and can quit.
let lastRpgReload = 0;
function handleRenderProcessGone(details) {
  console.warn('[wrl-forge] renderer gone:', details.reason, details.exitCode);
  if (!mainWindow || !mainWindow.webContents) return;
  // Always force-flush any pending recovery so the new renderer can read the
  // freshest possible snapshot on its next init.
  if (recoveryController) {
    try { recoveryController.forceFlush(); } catch (e) { /* noop */ }
  }
  const now = Date.now();
  if (now - lastRpgReload < 10000) {
    console.error('[wrl-forge] renderer crashed again within 10s; not reloading to avoid a loop. Quit and restart the app.');
    return;
  }
  lastRpgReload = now;
  try { mainWindow.webContents.reload(); } catch (e) { /* webContents may already be torn down */ }
}

// Phase Beta 2 -- beforeunload safety net. The renderer's `beforeunload` is
// what currently fires a "discard / keep" modal for unsaved work; we add a
// synchronous best-effort flush of any pending recovery snapshot so a fast
// navigation away doesn't lose the last keystrokes. The renderer has its own
// debounce, so the typical case is "nothing to flush" -- this is the tail
// guard.
async function flushRecoveryBeforeUnload() {
  if (!recoveryController) return;
  try { recoveryController.forceFlush(); } catch (e) { /* noop */ }
}

// I/O + launch dependency bag for the Mall edit flow (src/editor/mall-edit-flow.js).
// Kept here so the flow itself stays pure and unit-testable without Electron.
const mallEditDeps = {
  readSource: (p) => readWrlSource(p),
  editPathFor,
  writeWorkingCopy: (editFile, text) => fs.writeFileSync(editFile, text, 'utf8'),
  workingCopyExists: (editFile) => fs.existsSync(editFile),
  launch: (editFile) => launchEditor(editFile),
};

// Open a mall .wrl (gzip or plain) and prepare its plain .edit.wrl working copy.
// Opening a file is PASSIVE (Phase 7B1): it never launches an external editor.
ipcMain.handle('mall:open', async () => {
  const res = await dialog.showOpenDialog({
    title: 'Open Cybertown mall .wrl (gzip or plain)',
    properties: ['openFile'],
    filters: [
      { name: 'VRML files', extensions: ['wrl', 'wrz'] },
      { name: 'All files', extensions: ['*'] },
    ],
  });
  if (res.canceled || res.filePaths.length === 0) return null;
  return openMallFile(res.filePaths[0]);
});

ipcMain.handle('mall:openPath', async (_evt, mallPath) => openMallFile(mallPath));

function openMallFile(mallPath) {
  // Passive open: write the plain working copy, but do NOT launch any external
  // editor. The external editor starts only via the explicit mall:openInEditor
  // action below.
  const info = openMallItem(mallPath, mallEditDeps);
  currentSession = { mallPath, editFile: info.editFile };
  // `info.text` came out of THIS artifact, so identity is already proven here:
  // the artifact's own byte count is the authoritative upload size, and
  // `info.rawBytes` is that count measured off disk -- not recomputed from text.
  const sizeContext = info.wasGzipped
    ? { artifactBytes: info.rawBytes, artifactIsGzip: true, artifactMatchesText: true }
    : { artifactBytes: null, artifactIsGzip: false, artifactMatchesText: null };
  // mallPayload refuses to let validator output shadow a measured file fact --
  // the exact defect that used to drop `rawBytes` on the floor here.
  return mallPayload({
    mallPath: info.mallPath,
    editFile: info.editFile,
    wasGzipped: info.wasGzipped,
    // Bytes of the file actually on disk (gzip bytes for a packed item), kept
    // distinct from the validator's textBytes/artifactBytes.
    sourceFileBytes: info.rawBytes,
  }, validate(info.text, sizeContext));
}

// Resolve the external editor per platform (VSCodium → VS Code), honouring a
// user override from settings.json (`editorCommand`) or the WRL_FORGE_EDITOR env
// var. Cross-platform: Linux `codium` on PATH; Windows install locations + PATH
// shims (see src/editor/editor-locator.js).
function resolveConfiguredEditor() {
  let override = null;
  try {
    override = loadSettings(app.getPath('userData')).editorCommand;
  } catch { /* settings are best-effort */ }
  return resolveEditor({ override });
}

// Launch the editor on editFile. Returns { launched:true } or a structured
// { launched:false, reason } so callers can surface a clear "editor not found"
// message instead of failing silently. Never throws.
function launchEditor(editFile) {
  // WRL_FORGE_NO_EDITOR lets the non-interactive QA/screenshot harness drive the
  // real open flow without spawning a VSCodium window per run.
  if (process.env.WRL_FORGE_NO_EDITOR) return { launched: false, reason: 'disabled' };
  const resolution = resolveConfiguredEditor();
  if (!resolution.found) {
    console.warn('[wrl-forge] editor not found:', resolution.hint);
    return { launched: false, reason: 'not-found', hint: resolution.hint, tried: resolution.tried };
  }
  try {
    const spec = buildLaunch(resolution, editFile);
    const child = spawn(spec.command, spec.args, spec.options);
    // A spawn error (e.g. a stale path) arrives asynchronously; surface it in the
    // log rather than crashing the app. The user can still edit via the OS.
    child.on('error', (err) => console.warn('[wrl-forge] editor launch failed:', String(err && err.message || err)));
    child.unref();
    return { launched: true, command: resolution.command, source: resolution.source };
  } catch (err) {
    return { launched: false, reason: 'spawn-failed', error: String((err && err.message) || err) };
  }
}

// Explicit "Open in External Editor" action. This is the ONLY Mall-lane path that
// starts an external-editor process: it ensures the .edit.wrl working copy exists
// (recreating it from the source if it was removed) and launches the editor on it.
ipcMain.handle('mall:openInEditor', async () => {
  if (!currentSession) throw new Error('No file is open.');
  const { editFile, editorStatus } = openExternalEditor(currentSession, mallEditDeps);
  return { editFile, editorStatus };
});

// Read-only preview source loader. `role` selects the currently-open item
// ('source') or its plain working copy ('edit') -- NOT a renderer-supplied path.
// Detects gzip by magic bytes and decompresses in the main process, so X_ITE
// only ever receives plain UTF-8 text (it never fetches/parses gzip bytes).
// Returns the source directory as a file:// baseURL so relative textures resolve
// against the item's own folder, plus any remote url references found (advisory).
// There is no write path here -- this channel cannot mutate any file.
ipcMain.handle('preview:load', async (_evt, role) => {
  if (!currentSession) throw new Error('No file is open to preview.');
  if (role !== 'source' && role !== 'edit') throw new Error(`Unknown preview role '${role}'.`);
  const target = role === 'source' ? currentSession.mallPath : currentSession.editFile;
  const { text, wasGzipped, rawBytes } = readWrlSource(target);
  return {
    role,
    text,
    wasGzipped,
    rawBytes,
    baseURL: fileDirUrl(target),
    sourcePath: target,
    remoteUrls: scanRemoteUrls(text),
  };
});

// Re-read the plain .edit.wrl working copy, validate it, and report status
// without repacking -- used for a live "Check" button while editing.
ipcMain.handle('mall:check', async (_evt, editFile) => {
  const text = fs.readFileSync(editFile, 'utf8');
  // Measure the REAL mall artifact and prove whether it still decompresses to
  // the working copy being checked. Without this the check would report the
  // size of a file WRLForge has not written yet.
  const mallPath = currentSession ? currentSession.mallPath : null;
  return validate(text, measureArtifact(mallPath, text));
});

// Repack the edited plain text back into the mall .wrl, gzip by default
// (matching the mall upload convention).
//
// Lane B B2: this handler is now THIN. It resolves the paths it owns and hands
// the write to `repackMall`, which runs the shared safe-write discipline --
// unchanged-gzip preservation, verified in-memory candidate, the 81,920 B
// pre-write ceiling, then temp + fsync + read-back + backup + atomic rename.
// The old direct `fs.writeFileSync(mallPath, out)` (and its own
// backup-before-write) are gone: nothing outside file-io.js writes a real Mall
// artifact any more.
//
// Paths are main-owned, with no fallback. Repack REQUIRES an active
// `currentSession`, and reads/writes only the paths that session holds. The
// renderer still sends `mallPath`/`editFile` for wire compatibility (no preload
// change), and they are deliberately not destructured here: only `asGzip` may
// influence this operation. With no session open the request is rejected before
// anything is read, backed up, encoded or written -- see
// src/mall/repack-paths.js for why the old `? :` fallback was a defect.
ipcMain.handle('mall:repack', async (_evt, { asGzip }) => {
  const { mallPath, editFile } = activeRepackPaths(currentSession);
  const text = fs.readFileSync(editFile, 'utf8');

  return repackMall({ mallPath, text, asGzip });
});

ipcMain.handle('shell:revealInFolder', async (_evt, filePath) => {
  shell.showItemInFolder(filePath);
});

// ---------------------------------------------------------------------------
// World Project lane (Phase 4A) -- READ-ONLY. No handler writes, repairs, copies,
// renames, deletes, uploads, or fetches. The main process owns every project
// path; the renderer can only pick among detected candidates and read results.
// ---------------------------------------------------------------------------

// Switch the one window between the Mall and World workspaces (whitelisted page).
ipcMain.handle('app:goto', async (_evt, page) => {
  if (!APP_PAGES[page]) throw new Error(`Unknown page '${page}'.`);
  await gotoPage(page);
  return { page };
});

// Open a project FOLDER: detect the primary world file(s) within it. Ambiguity is
// reported (candidates returned) rather than silently guessed.
ipcMain.handle('world:openFolder', async () => {
  const res = await dialog.showOpenDialog(mainWindow, {
    title: 'Open a World Project folder',
    properties: ['openDirectory'],
  });
  if (res.canceled || res.filePaths.length === 0) return null;
  const root = res.filePaths[0];
  const detection = detectPrimaries(root);
  return worldSession.open({ root, ...detection });
});

// Open a single primary WORLD FILE directly; its folder becomes the project root.
ipcMain.handle('world:openPrimaryFile', async () => {
  const res = await dialog.showOpenDialog(mainWindow, {
    title: 'Open a primary world .wrl / .wrz',
    properties: ['openFile'],
    filters: [
      { name: 'VRML worlds', extensions: ['wrl', 'wrz'] },
      { name: 'All files', extensions: ['*'] },
    ],
  });
  if (res.canceled || res.filePaths.length === 0) return null;
  const primary = res.filePaths[0];
  const root = path.dirname(primary);
  return worldSession.open({
    root,
    primary,
    candidates: [{ path: primary, relative: path.basename(primary), bytes: safeSize(primary), depth: 0, referenced: false, preferred: true }],
    ambiguous: false,
    empty: false,
  });
});

// Resolve an ambiguous project: choose one detected candidate as the primary.
ipcMain.handle('world:choosePrimary', async (_evt, primaryPath) => {
  return worldSession.choosePrimary(primaryPath);
});

// Scan the currently-open project (held root + selected primary). Overlapping
// scans are refused by the session; a transient failure keeps the last result.
ipcMain.handle('world:scan', async () => {
  try {
    const scan = await worldSession.scan();
    return serializeScan(scan);
  } catch (err) {
    return { ok: false, status: err.code === 'EBUSY' ? 'busy' : 'error', error: String(err.message || err) };
  }
});

// Refresh == rescan the same primary (external edits picked up on demand only).
ipcMain.handle('world:refresh', async () => {
  try {
    const scan = await worldSession.scan();
    return serializeScan(scan);
  } catch (err) {
    return { ok: false, status: err.code === 'EBUSY' ? 'busy' : 'error', error: String(err.message || err) };
  }
});

// Read-only World Project PREVIEW loader (Phase 4B). Ensures a scan exists,
// installs the asset-graph-derived read-authorization set into the scheme
// handler, and returns ONLY controlled content + metadata to the renderer: the
// decompressed primary text, the wrlworld base URL, advisory counts, and the
// warning lists. It takes NO renderer-supplied path and never writes -- the main
// process owns every project path (held in worldSession). X_ITE then resolves the
// world's nested deps through the confined wrlworld:// handler.
ipcMain.handle('world:previewLoad', async () => {
  if (!worldSession.primary) throw new Error('No primary world is selected.');
  let scan = worldSession.last;
  if (!scan) scan = await worldSession.scan();
  worldPreview = {
    projectRoot: path.resolve(scan.root),
    authorized: buildAuthorizedSet(scan.graph),
  };
  return buildPreviewPayload(scan);
});

ipcMain.handle('world:describe', async () => worldSession.describe());

// Reveal a path in the OS file manager, confined to the open project's root and
// only if it actually exists on disk (never a renderer-chosen arbitrary path).
ipcMain.handle('world:reveal', async (_evt, targetPath) => {
  if (!worldSession.root) throw new Error('No World Project is open.');
  const root = path.resolve(worldSession.root);
  const target = path.resolve(String(targetPath || ''));
  const rel = path.relative(root, target);
  const inside = target === root || (!rel.startsWith('..') && !path.isAbsolute(rel));
  if (!inside) throw new Error('Refusing to reveal a path outside the project root.');
  if (!fs.existsSync(target)) throw new Error('That path no longer exists.');
  shell.showItemInFolder(target);
  return { revealed: target };
});

ipcMain.handle('world:revealRoot', async () => {
  if (!worldSession.root || !fs.existsSync(worldSession.root)) throw new Error('No World Project root available.');
  shell.showItemInFolder(worldSession.root);
  return { revealed: worldSession.root };
});

// Explicit-only: launch VSCodium on the primary world file. Opening a World
// Project never auto-launches the editor.
ipcMain.handle('world:openPrimaryInEditor', async () => {
  if (!worldSession.primary) throw new Error('No primary world is selected.');
  const editorStatus = launchEditor(worldSession.primary);
  return { primary: worldSession.primary, editorStatus };
});

// Read-only PACKAGE AUDIT (Phase 5A). Ensures a scan exists, derives the
// deterministic package plan from the production asset graph, and returns the
// renderer-facing audit: status (ready/blocked/needs-review), totals, blocking
// findings, unused files, findings, and a manifest preview. It NEVER writes and
// takes no renderer-supplied path (main owns every project path via worldSession).
ipcMain.handle('world:packageAudit', async () => {
  if (!worldSession.primary) throw new Error('No primary world is selected.');
  let scan = worldSession.last;
  if (!scan) scan = await worldSession.scan();
  const plan = buildPackagePlan(scan);
  return {
    ok: plan.ok,
    status: plan.status,
    projectName: plan.projectName,
    primaryRel: plan.primaryRel,
    totals: plan.totals,
    blocking: plan.blocking,
    findings: plan.findings,
    unusedFiles: plan.unusedFiles,
    files: plan.files,
    manifest: buildManifest(plan),
    label: plan.label,
    disclaimer: plan.disclaimer,
  };
});

// Explicit-user-action BUILD REVIEW BUNDLE (Phase 5A). Requires an explicit
// destination (a Save dialog owned by the main process — the renderer never
// supplies a path). Writes a deterministic ZIP ONLY to a new location OUTSIDE the
// project (bundle-builder refuses in-project writes, overwrites, and blocked
// projects). The source project is never mutated. NOT an upload: nothing is sent
// anywhere and no server compatibility is claimed.
ipcMain.handle('world:buildReviewBundle', async () => {
  if (!worldSession.primary) throw new Error('No primary world is selected.');
  let scan = worldSession.last;
  if (!scan) scan = await worldSession.scan();

  // Pre-flight the plan so we can refuse a blocked project BEFORE prompting.
  const plan = buildPackagePlan(scan);
  if (plan.status === 'blocked' || plan.blocking.length) {
    return { ok: false, code: 'EBLOCKED', status: 'blocked', blocking: plan.blocking,
      error: 'Packaging is blocked — resolve the blocking findings first.' };
  }

  // Default the bundle name + a location OUTSIDE the project (its parent folder),
  // so the default never lands inside the source project.
  const defaultName = `${plan.projectName || 'world'}-world-project-bundle.zip`;
  const defaultDir = path.dirname(path.resolve(scan.root));
  const res = await dialog.showSaveDialog(mainWindow, {
    title: 'Save WRL Forge World Project Bundle',
    defaultPath: path.join(defaultDir, defaultName),
    filters: [{ name: 'ZIP archive', extensions: ['zip'] }],
    properties: ['createDirectory', 'showOverwriteConfirmation'],
  });
  if (res.canceled || !res.filePath) return null;

  try {
    const summary = writeReviewBundle(scan, res.filePath, { plan });
    return { ok: true, ...summary };
  } catch (err) {
    return { ok: false, code: err.code || 'EBUNDLE', error: String(err.message || err),
      blocking: (err.plan && err.plan.blocking) || undefined };
  }
});

// ---------------------------------------------------------------------------
// Native editor lane (Phase 7B). The main process (editorController) owns the one
// open document and every path decision. The renderer supplies text + intent and,
// for a World "open referenced WRL", a reference that is authorized against the
// scan graph. There is NO channel by which the renderer names a write path: save
// writes only to the held source; saveAs writes only to a path from main's own
// Save dialog. Conflict (external change) is returned structured so the renderer
// can raise its Reload / Save As / Cancel choice.
// ---------------------------------------------------------------------------
ipcMain.handle('editor:openMall', async () => editorController.openMall());
ipcMain.handle('editor:openWorldPrimary', async () => editorController.openWorldPrimary());
ipcMain.handle('editor:openWorldReference', async (_evt, ref) => editorController.openWorldReference(ref));
ipcMain.handle('editor:describe', async (_evt, opts) => editorController.describe(opts));
ipcMain.handle('editor:setText', async (_evt, { sessionId, text } = {}) => editorController.setText(sessionId, text));

ipcMain.handle('editor:save', async (_evt, { sessionId, text, allowOverwrite } = {}) => {
  try {
    // EditorController.save() now invokes the recovery controller's
    // recordClear on a successful Save (injected in createRecoveryController).
    // Failures leave the recovery untouched so the next launch can offer it
    // again.
    return editorController.save(sessionId, text, { allowOverwrite });
  } catch (err) {
    // A conflict or failed verification is an expected, user-actionable outcome:
    // return it structured (source untouched, buffer stays dirty) so the renderer
    // can offer Reload / Save As / Cancel. Programming/renderer-state errors
    // (stale id, bad arg, nothing open) reject as usual.
    if (err.code === 'EEXTERNAL' || err.code === 'EVERIFY') {
      return { ok: false, code: err.code, error: String(err.message || err), current: err.current || null };
    }
    throw err;
  }
});

ipcMain.handle('editor:saveAs', async (_evt, { sessionId, text, format } = {}) =>
  editorController.saveAs(sessionId, text, { format }));
ipcMain.handle('editor:reload', async (_evt, { sessionId } = {}) => editorController.reload(sessionId));
ipcMain.handle('editor:checkConflict', async (_evt, { sessionId } = {}) => editorController.checkConflict(sessionId));
ipcMain.handle('editor:openInExternal', async (_evt, { sessionId } = {}) => editorController.openInExternal(sessionId));
ipcMain.handle('editor:close', async (_evt, { sessionId } = {}) => {
  // Closing an editor session must leave no live preview overlay behind (either
  // profile -- invalidation on a bridge that never held the session is a no-op).
  if (sessionId != null) {
    if (mallPreviewBridge) mallPreviewBridge.invalidateSession(sessionId);
    if (worldPreviewBridge) worldPreviewBridge.invalidateSession(sessionId);
  }
  const res = editorController.close(sessionId);
  // Phase Beta 2 -- an explicit Close forgets the recovery snapshot. The
  // user chose "Close" deliberately; the next launch must NOT prompt them
  // to restore work they already decided to throw away. Any Save action
  // before this would have already cleared the snapshot (a Save before
  // Close is the canonical happy path).
  if (recoveryController) recoveryController.recordClear();
  return res;
});
ipcMain.handle('editor:restore', async () => editorController.restore());

// ---------------------------------------------------------------------------
// Phase Beta 2 -- Crash recovery IPC.
// The renderer pings `recoveryRecordDirty` on every keystroke (throttled on
// its end); main debounces the writes through the recovery controller's timer.
// `recoveryRead` is called once at startup by whichever page mounts first --
// the renderer converts a `found: true` reply into the Restore / Start Fresh
// prompt. `recoveryAdopt` performs the Restore action; `recoveryClear` is the
// Start Fresh action (or the manual forget path). No path is renderer-supplied
// -- the recovery file lives at userData exclusively.
// ---------------------------------------------------------------------------
function activeWorkspaceName() {
  // The renderer mirrors this in its workspace state, but main is the
  // authority here: when the page is `editor`, we report 'editor'; otherwise
  // we report the lane. Used only for the recovery snapshot's
  // activeWorkspace field -- telling the restore prompt where to drop the
  // user on Restore.
  return currentPage || 'mall';
}
ipcMain.handle('editor:recoveryRead', async () => recoveryController ? recoveryController.readRecovery() : { found: false, reason: 'no-controller' });
ipcMain.handle('editor:recoveryRecordDirty', async (_evt, payload) => recoveryController ? recoveryController.recordDirtyState(payload || {}) : { ok: false, reason: 'no-controller' });
ipcMain.handle('editor:recoveryClear', async () => recoveryController ? recoveryController.recordClear() : { ok: false, reason: 'no-controller' });
ipcMain.handle('editor:recoveryAdopt', async () => {
  if (!recoveryController) return { ok: false, reason: 'no-controller' };
  const r = recoveryController.readRecovery();
  if (!r.found) return { ok: false, reason: r.reason || 'no-recovery' };
  const adopted = recoveryController.adoptRecovery(r.record);
  // adopted.buffer is the recovered text for unsaved-no-source cases; the
  // renderer needs it to seed a fresh unsaved session.
  return adopted;
});
ipcMain.handle('editor:recoveryActiveWorkspace', async () => activeWorkspaceName());

// ---------------------------------------------------------------------------
// Live-preview IPC (Phase 7C2 Mall + Phase 7C3 World). The renderer sends ONLY
// { sessionId, text, bufferVersion } -- never a path/base/root/url/scheme. The
// request is routed to the bridge owning the OPEN document's profile; each
// bridge independently resolves the editor session and its own authorization
// authority (the held Mall source, or the World scan graph), byte-substitutes
// the unsaved buffer through the 7C1 overlay, and returns the render payload.
// No file is written; there is no channel by which the renderer names a path.
// ---------------------------------------------------------------------------
ipcMain.handle('editor:previewLoad', async (_evt, { sessionId, text, bufferVersion } = {}) =>
  activePreviewBridge().load({ sessionId, text, bufferVersion }));
ipcMain.handle('editor:previewSaved', async (_evt, { sessionId } = {}) =>
  activePreviewBridge().saved({ sessionId }));
ipcMain.handle('editor:previewAccept', async (_evt, { sessionId, generation } = {}) =>
  activePreviewBridge().accept({ sessionId, generation }));
// Explicit "Find new files" (World only): run the NORMAL World rescan on the
// held project session, so a newly added file can become graph-authorized. The
// Mall profile has no rescan; the world bridge refuses non-World sessions.
ipcMain.handle('editor:previewRescan', async (_evt, { sessionId } = {}) =>
  worldPreviewBridge.rescanForNewFiles({ sessionId }));
ipcMain.handle('editor:previewClose', async (_evt, { sessionId } = {}) => {
  if (sessionId != null) {
    if (mallPreviewBridge) mallPreviewBridge.invalidateSession(sessionId);
    if (worldPreviewBridge) worldPreviewBridge.invalidateSession(sessionId);
  }
  return { ok: true };
});
ipcMain.handle('editor:previewLeak', async () => combinedPreviewLeak());

function safeSize(p) {
  try { return fs.statSync(p).size; } catch { return null; }
}
