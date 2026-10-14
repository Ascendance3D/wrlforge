# WRL Forge Releases

This is the index of WRL Forge releases. WRL Forge is a desktop tool for
classic VRML97 `.wrl` content, with builds for Linux, Windows, and macOS.

All releases are published on GitHub:
**https://github.com/Ascendance3D/wrlforge/releases**

## About these versions

- **Public beta / prerelease.** WRL Forge is in **beta**. Versions are marked
  *Beta / Prerelease* and are **not** stable or production releases. Expect rough
  edges and changes between builds.
- **Signing differs by platform.** Linux and Windows builds are **not
  code-signed** (unsigned by design for the beta), so SmartScreen/Defender may
  warn on first Windows launch — this is expected. macOS builds are **Developer
  ID Application signed, notarized, and stapled**, and are expected to pass
  Gatekeeper normally.
- **Architectures.** Linux x64, Windows x64, and macOS Apple Silicon (arm64).
  No Intel macOS build, no Linux/Windows ARM64, no Snap/Flatpak/Store package.
- **Free and open source (GPL-3.0-or-later).** WRL Forge is licensed under the GNU
  General Public License, version 3 or any later version. See [`LICENSE`](../LICENSE),
  [`COPYRIGHT.md`](../COPYRIGHT.md), and
  [`OPEN_SOURCE_PROVENANCE.md`](../OPEN_SOURCE_PROVENANCE.md). Third-party components
  keep their own licenses. Releases through `v1.3.0-beta.3` were distributed under the
  MIT License and those copies keep the permissions granted with them.

## Release lines

- **Legacy Electron line (through 1.4.0).** Built by the GitHub Actions
  workflow `.github/workflows/release.yml` for Linux x64, Windows x64 and macOS
  Apple Silicon. Since `v1.4.51-alpha.1` this workflow starts **only by manual
  dispatch**; a tag push does not start it. Its `legacy-guard` job refuses any
  Rust-line tag (an `-alpha` tag, or a version at or above 1.4.51) before a
  build job runs.
- **Rust/Tauri line (1.4.51 and later, Alpha).** Built from
  `apps/desktop-tauri` for **Linux x64 only** by the owner with
  `apps/desktop-tauri/packaging/package-linux.sh`, after the Linux gate
  (`apps/desktop-tauri/gate-linux.sh`). There is no Windows, macOS or mobile
  package for this line, and no workflow builds or publishes it. Pushing a
  Rust-line tag starts no release job.

---

## 1.4.0 (Beta)

**Status: Beta · Prerelease**

Machine version `1.4.0`; tag `v1.4.0`; human-facing release name **WRL Forge
1.4.0 (Beta)**. This release is marked as a GitHub prerelease. Making it
visible to the public is an owner-controlled action — the release workflow
assembles a **draft** prerelease, and the owner decides whether and when to
publish it.

Linux and Windows artifacts are unsigned. macOS artifacts are Developer ID
Application signed, notarized, and stapled.

Starting with this release, **the versioning convention changes**: the machine
version drops the `-beta.N` suffix (`1.4.0`, not `1.4.0-beta.1`) and **Beta
becomes a release-channel/status label** carried in the human-facing release
name (`WRL Forge 1.4.0 (Beta)`) and the GitHub prerelease flag, not in the
SemVer string itself. This is **not** a claim that the project has left beta —
see [CHANGELOG.md](../CHANGELOG.md) for the `[1.4.0]` entry
summarizing the source changes since `1.3.0-beta.3`.

Once published, `1.4.0` supersedes `1.3.0-beta.5` below as the current
release. Earlier releases listed further down this file remain available for
reference.

The authoritative SHA-256 values for every artifact in this release are in the
`SHA256SUMS-1.4.0.txt` file attached to the release. No hashes are duplicated
in this document; verify against the attached manifest.

### 1.4.0 release assets

**Linux x64** — unsigned

- `WRL-Forge-1.4.0-linux-x64.AppImage` (recommended)
- `WRL-Forge-1.4.0-linux-x64.tar.gz` (portable app directory)

**Windows x64** — unsigned

- `WRL-Forge-Setup-1.4.0-x64.exe` (recommended — NSIS installer)
- `WRL-Forge-1.4.0-x64.msi` (MSI installer)
- `WRL-Forge-Portable-1.4.0-x64.exe` (portable, no install)
- `WRL-Forge-1.4.0-windows-x64.zip` (portable unpacked app)

**macOS Apple Silicon (arm64)** — Developer ID Application signed, Hardened
Runtime, notarized, stapled

- `WRL-Forge-1.4.0-mac-arm64.dmg` (recommended)
- `WRL-Forge-1.4.0-mac-arm64.zip` (zipped app bundle)

**Checksums**

- `SHA256SUMS-1.4.0.txt`

That is **8 platform artifacts plus 1 checksum manifest — 9 release assets**.
Verify any download against the attached `SHA256SUMS-1.4.0.txt`.

---

## 1.3.0-beta.5 — Cross-Platform Beta (Private, Unsigned)

**Status: Beta · Prerelease · Unsigned — private candidate. Not a stable
release. Do not distribute publicly.**

This is the **cross-platform** refresh of the `1.3.0-beta` series — the same
source built for **Linux x64** and **native Windows 11 x64**, with no new
product features versus `1.3.0-beta.4`. The included work since the beta.3 era:

- **Phase Beta 2 — Crash Recovery** (already in `1.3.0-beta.4`): debounced
  recovery snapshot under `userData/editor-recovery.json`; Restore / Start Fresh
  prompt at next launch; source files on disk are NEVER mutated by recovery.
  See `docs/PHASE_BETA_2_CRASH_RECOVERY.md`.
- **Phase: Accessibility + Performance** (D1 + D2): keyboard accessibility for
  the Mall + World toolbars (toolbar semantics, `aria-keyshortcuts` on `Ctrl+R` /
  `Ctrl+E`), `Ctrl+R` Repack + `Ctrl+E` Open in Native Editor wired to the same
  named actions the buttons use, shortcut suppression in text inputs / modal
  backdrops / while the matching button is disabled, `role=list` semantics on
  the Inspector findings list, single-shot back-focus restoration. The perf gate
  is PASS (`MEDIAN_GATE`, all profile medians under the 250 ms ceiling), and
  **45.20 MB stable** renderer heap over 30 edit/reparse cycles. See
  `docs/ACCESSIBILITY_PERFORMANCE.md`.

> **Public-download checksums:** the authoritative SHA-256 manifest for the
> published `v1.3.0-beta.5` release is `SHA256SUMS-1.3.0-beta.5.txt` attached
> to the GitHub Release. The hashes in the tables below are the
> **pre-publication independently QA-tested build** and may differ from the
> later GitHub Actions rebuild — verify against `SHA256SUMS-1.3.0-beta.5.txt`
> for public downloads.

### Linux x64 — unsigned

| File | Size | SHA-256 |
|---|---|---|
| `WRL-Forge-1.3.0-beta.5-linux-x64.AppImage` | 134,527,292 B (~128 MB) | `45cb38735c814ffc14cb00b6002f7aa7505ac495d1b17b97a6bf99c554f47bec` |
| `WRL-Forge-1.3.0-beta.5-linux-x64.tar.gz` | 127,632,242 B (~122 MB) | `8cb1c5a2d7fd0524ec075a6d61830d24b328db581d0c41d9dd39cd2c94953df4` |

### Windows x64 — unsigned

| File | Size | SHA-256 |
|---|---|---|
| `WRL-Forge-Setup-1.3.0-beta.5-x64.exe` (NSIS installer) | 111,019,007 B (~106 MB) | `35f0839816e294af3934e26afc318e74116c24846439b932c685acda186771` |
| `WRL-Forge-Portable-1.3.0-beta.5-x64.exe` (portable) | 110,799,995 B (~106 MB) | `c8dca0ec6a6b0a3c4ddd87a9e8f46005bb98c87a938f1c0f06cd5527b5768e30` |
| `WRL-Forge-1.3.0-beta.5-x64.msi` (MSI installer) | 126,521,344 B (~121 MB) | `a521e530887ca3582d703fbb8c507b8e6e0efa970010ca7b6f0997efef1ecf03` |

Checksums in `release/SHA256SUMS-1.3.0-beta.5.txt` (git-ignored, like the
artifacts). The SHA-256 values in that manifest are the **public-download
authority**; the values in the tables above are the **pre-publication
independently QA-tested build** and may differ from the GitHub Actions
rebuild. All five are **unsigned** (PE certificate table empty — verified at
build time) and **x64 only** (no ARM64). No auto-update, no store packaging,
no public release.

**Verified lifecycle on Windows 11** (libvirt/QEMU `win11` guest at
`192.168.122.170`, local NTFS `C:\Projects\wrlforge`, driven headlessly over SSH):
NSIS install → installed exe launches with `WRL_FORGE_CAPTURE_SERVER=1` and
emits `WRL_FORGE_CAPTURE_READY` + `WRL_FORGE_CAPTURE_OK` → Start-menu shortcut
present → install dir + shortcuts + registry entry removed on uninstall;
**userData `editor-session.json` SHA-256 unchanged across install/uninstall**
(`window-state.json` may change when the launched EXE writes its own window
state — not a regression). Portable EXE launches cleanly via auto-extract in a
scratch directory. Source SHA: **`2f3591b3c56bee1de5ed38de609f22b11b4f5997`**.

Cross-Platform-Beta Tier-3 capture-server smoke against the installed EXE was
emitted end-to-end (full event stream: `ready`, `capture:start`, `capture:done`,
`shutdown:`,`exit` graceful, `leak:check` alive:false, `survivors` empty) — see
`docs/CROSS_PLATFORM_BETA.md` §9.

See `docs/CROSS_PLATFORM_BETA.md` for the full cross-platform evidence record.

### Known limitations (inherited from beta.4 + beta.3)

- **Unsigned** — SmartScreen/Defender may warn on first Windows launch
  (*More info → Run anyway*). **Not** eliminated by design. See
  `docs/SIGNING_READINESS.md`.
- **x64 only** — no Linux ARM64, no Windows ARM64, no macOS, no Snap/Flatpak/
  Store package.
- **Review + manual upload** — the WRL Forge World Project Bundle is a review
  artifact you upload through the Cybertown website / Mall workflow by hand.
  WRL Forge does no direct upload (by design) and makes no CTR server-format
  claim.
- No auto-update, telemetry, ads, Microsoft Store packaging, or public release.

---

## 1.3.0-beta.4 — Crash Recovery

**Status: Beta · Prerelease · Unsigned — not a stable release.**

This refresh ships **Phase Beta 2 — Crash Recovery** so unsaved native-editor
work survives an abnormal exit. A Restore / Start Fresh prompt at next
launch offers the user a deliberate decision before any recovered text
is restored or discarded. Source files on disk are NEVER mutated by
recovery — the recovered buffer opens as unsaved work, and the user
decides whether to keep it.

The exact release commit is shown on the GitHub release page:
**https://github.com/DJAscendance/wrlforge/releases/tag/v1.3.0-beta.4**

### What changed since beta.3

- **Phase Beta 2 crash recovery**. Debounced recovery snapshot under
  `userData/editor-recovery.json` (schemaVersion 2; carries
  `sourceStat { mtimeMs, size, sha1 }` of the real source bytes as the
  conflict anchor — gzip-aware). The three renderer pages (Mall, Editor,
  World) all invoke the same shared `WRLForgeRecoveryPrompt.maybePrompt`
  on init, idempotent per session via `sessionStorage`.
- **Recovery lifecycle rule**: the recovery file is cleared ONLY by
  successful Save / explicit Discard / explicit Start Fresh. Restore,
  Save failure, missing source, renderer reload, app restart — all keep
  it on disk so the next launch can offer it again. Independent final
  re-QA: `PHASE_BETA_2_FINAL_REQA_PASS`.
- **B1 plain + gzip external-change protection**. Save after Restore
  detects a post-snapshot external source change through the existing
  `EEXTERNAL` conflict path (no second conflict system). For gzip
  sources, the conflict anchor is the sha1 of the gzip bytes, NOT the
  decompressed text — the recovery snapshot carries the real disk
  identity at snapshot time.
- **B2 missing-source viewer**. When the original source is missing
  (or the recovery lacks a `sourceStat` anchor), the renderer shows a
  dedicated viewer with the recovered text in a selectable `<pre>` and
  a Copy-to-clipboard action. Recovery stays on disk; the viewer is
  non-destructive.
- **B4 / M1 prompt pages**. `recovery-prompt.js` script tag now loads
  BEFORE `editor.js` and `world.js` so the page init can call
  `WRLForgeRecoveryPrompt.maybePrompt` immediately. Deferred scripts
  run in document order.
- **Third-party bundle unchanged**: x_ite 15.1.10 (the only runtime
  dependency) and CodeMirror 6 (dev-time editor bundle).

### Downloads

Start at **https://wrlforge.com**, or download directly from the
[GitHub release](https://github.com/DJAscendance/wrlforge/releases/tag/v1.3.0-beta.4).

**Linux x64** — built locally on this machine
- `WRL-Forge-1.3.0-beta.4-linux-x64.AppImage` — recommended
- `WRL-Forge-1.3.0-beta.4-linux-x64.tar.gz` — portable app directory

**Windows x64** — owner-routed (rebuild on the Win11 guest and update
the release page when the next lane lands)
- `WRL-Forge-Setup-1.3.0-beta.4-x64.exe` — recommended
- `WRL-Forge-1.3.0-beta.4-x64.msi`
- `WRL-Forge-Portable-1.3.0-beta.4-x64.exe`
- `WRL-Forge-1.3.0-beta.4-windows-x64.zip`

**Checksums**

- `SHA256SUMS-1.3.0-beta.4.txt`

This release is still **beta, prerelease, unsigned, and x64-only**. There is
no auto-update, telemetry, direct Cybertown upload, or automatic
submission.

### Known limitations

- **Beta limits inherited from beta.3**: unsigned (SmartScreen prompt on
  Windows); x64 only (no ARM64); no auto-update; not server-certified
  for direct Cybertown upload.
- **Phase Beta 2 accepted limit**: the renderer throttles dirty-buffer
  recovery pings at 1.5 s and main debounces at 5 s. In the 0–1.5 s
  window after a keystroke, the latest text can exist only in renderer
  memory. A renderer crash in that window may lose those characters.
  Crash survives — the user is still offered Restore on the next
  launch (possibly without the last typing burst).
- **Phase 7D0 deferred**: no automatic last-Mall-item restore, no
  automatic last-World-Project restore. Mall and World pages open
  empty after a fresh launch.

# 1.3.0-beta.3 — Desktop Open With Update

**Status: Beta · Prerelease · Unsigned — not a stable release.**

This is a small maintenance update for the public beta. It makes WRL Forge work
properly when a `.wrl` or `.wrz` is opened from the Linux desktop, a file
manager, or the command line. It also packages the shortcut installer and cyan
icon so extracted Linux builds can register themselves without a hardcoded
install location.

The exact release commit and tag are shown on the GitHub release page:
**https://github.com/DJAscendance/wrlforge/releases/tag/v1.3.0-beta.3**

### What changed

- WRL Forge can open `.wrl` and `.wrz` arguments at startup and forward a later
  **Open With** request to the already-running app.
- Startup, second-instance, and desktop file-open requests are serialized so an
  older request cannot win a race and replace the file you opened most recently.
- Linux packages include a path-neutral, per-user shortcut installer. It adds
  WRL Forge to the application menu, installs the approved cyan SVG icon, and
  registers VRML files as an **Open With** choice.
- The source launcher now forwards command-line arguments correctly.
- File-URL parsing and tests are portable across Linux and Windows.
- Public package metadata now points to **https://wrlforge.com**.

Files opened from the desktop enter the **Mall Item** lane. World Projects still
open through their own workspace because WRL Forge needs the project root to
scan and authorize dependencies. The Linux helper does not force WRL Forge to
become your default VRML application.

### Downloads

Start at **https://wrlforge.com**, or download directly from the
[GitHub release](https://github.com/DJAscendance/wrlforge/releases/tag/v1.3.0-beta.3).

**Linux x64**

- `WRL-Forge-1.3.0-beta.3-linux-x64.AppImage` — recommended
- `WRL-Forge-1.3.0-beta.3-linux-x64.tar.gz` — portable app directory

**Windows x64**

- `WRL-Forge-Setup-1.3.0-beta.3-x64.exe` — recommended
- `WRL-Forge-1.3.0-beta.3-x64.msi`
- `WRL-Forge-Portable-1.3.0-beta.3-x64.exe`
- `WRL-Forge-1.3.0-beta.3-windows-x64.zip`

**Checksums**

- `SHA256SUMS-1.3.0-beta.3.txt`

This release is still **beta, prerelease, unsigned, and x64-only**. Windows may
show an unknown-publisher warning. There is no auto-update, telemetry, direct
Cybertown upload, or automatic submission.

---

## 1.3.0-beta.2 — First Public Beta

**Status: Beta · Prerelease · Unsigned — not a stable release.**

The first **public** beta of WRL Forge. (The earlier `1.3.0-beta.1` was a
**private** beta.) The exact release commit and tag **`v1.3.0-beta.2`** are shown
on the GitHub release page (which displays the tagged commit SHA):
**https://github.com/DJAscendance/wrlforge/releases/tag/v1.3.0-beta.2**

### Highlights

- **Native editor** (CodeMirror 6): syntax highlighting, document outline, live
  syntax **diagnostics + advisories**, line/column position, **five themes**
  including **High Contrast**, and zoom (`Ctrl` `+` / `-` / `0`).
- **Backups and conflict handling:** saves are **backup-first** (a timestamped
  backup is written before the source is replaced), and WRL Forge **detects
  external changes** to a file so you don't silently clobber edits made outside
  the app.
- **Unsaved live preview** (X_ITE) for **both** the Mall and World lanes: a split
  view of your in-memory editor buffer with **no temp file**, ~700 ms debounce.
  It keeps the **last valid scene** during a temporary syntax error and recovers
  when you fix it.
- **Mall Item** inspection with **Cybertown placement preview**: **Original** mode
  and **Fit** mode, with placement guides and scale info. Preview transforms
  never modify your source.
- **World Project** lane: scans a multi-file world (nested **Inline** files,
  textures), reports **missing / unsafe / case-mismatched** assets, gives an
  embedded X_ITE **world preview** with a **viewpoint selector**, **navigation
  mode**, and **reset view**, and lets you **edit a nested WRL** with a full-world
  preview via an unsaved nested override.
- **Nested WRL** support and **plain + gzip-compressed** `.wrl` files, opened
  transparently.
- **World Project Bundle:** builds a portable ZIP for **manual review and
  hand-off** (deterministic in-repo ZIP writer). You upload it by hand through the
  Cybertown website — it is review + manual handoff only, **not** a direct upload
  and **not** a server-certified format.
- **Optional VSCodium / VS Code** integration, launched **only** by an explicit
  external-editor action (ordinary file opening never launches it).
- **New cyan WRL Forge app icon.**

### Downloads

From the release page
(**https://github.com/DJAscendance/wrlforge/releases/tag/v1.3.0-beta.2**):

**Linux x64**
- `WRL-Forge-1.3.0-beta.2-linux-x64.AppImage` — recommended
- `WRL-Forge-1.3.0-beta.2-linux-x64.tar.gz` — portable app directory

**Windows x64**
- `WRL-Forge-Setup-1.3.0-beta.2-x64.exe` — recommended (NSIS installer)
- `WRL-Forge-1.3.0-beta.2-x64.msi` — MSI installer
- `WRL-Forge-Portable-1.3.0-beta.2-x64.exe` — portable, no install
- `WRL-Forge-1.3.0-beta.2-windows-x64.zip` — portable unpacked app

**Checksums**
- `SHA256SUMS-1.3.0-beta.2.txt`

Install instructions for each: [INSTALLATION.md](INSTALLATION.md).

### Known limitations

- **Unsigned Windows builds** → SmartScreen/Defender may warn (*More info → Run
  anyway*). Unsigned by design for this beta; signing would not by itself remove
  the warning.
- The **portable EXE** has a documented **stdout-handshake** limitation that
  affects **automated visual capture only**, not normal use.
- **Historical nonstandard VRML extensions** may not behave identically to the
  original platform.
- Parser **advisories are advisory-only**; the **X_ITE runtime is authoritative**
  for what actually renders.
- **x64-only** public beta (no ARM64, no macOS).
- **No direct upload** — the World Project Bundle is a manual hand-off package.
- No telemetry, analytics, ads, auto-update, code signing, or crash upload.

### Verifying your download (SHA-256)

Every artifact's SHA-256 hash is in `SHA256SUMS-1.3.0-beta.2.txt` on the release
page. Verify before running.

**Linux:**
```bash
sha256sum -c SHA256SUMS-1.3.0-beta.2.txt
```
Run it from the folder holding both the checksum file and the artifacts; each
line should print `OK`.

**Windows (PowerShell):**
```powershell
Get-FileHash .\WRL-Forge-Setup-1.3.0-beta.2-x64.exe -Algorithm SHA256
```
Compare the printed hash to the matching line in `SHA256SUMS-1.3.0-beta.2.txt`.

### Reporting problems

- Troubleshooting guide: [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
- Bug reports (and what screenshots/evidence to attach): open an issue at
  **https://github.com/Ascendance3D/wrlforge/issues** — see
  [What to attach to a bug report](TROUBLESHOOTING.md#what-to-attach-to-a-bug-report).

### Notes

- **No direct upload / no Cybertown authentication / no automatic submission** —
  these will not be built (a locked product decision).
- WRL Forge is an **independent community project**. It is **not** affiliated
  with, endorsed by, sponsored by, or officially connected to **Cybertown** or its
  current/former operators. "Cybertown" and related names belong to their
  respective owners.

---

## 1.3.0-beta.1 — Private beta

A **private**, unsigned beta that preceded the first public release. Not
distributed publicly; superseded by **1.3.0-beta.2**.
