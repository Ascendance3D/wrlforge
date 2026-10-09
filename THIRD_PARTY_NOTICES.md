# Third-Party Notices

WRL Forge (`Copyright © 2026 Ryan Bundy (BassMekanik2000) and contributors`, licensed
`GPL-3.0-or-later`) bundles and builds upon third-party components. **WRL Forge does not own
these components.** They remain the property of their respective authors and are governed by
their own licenses, reproduced or referenced below — the GPL applies to WRL Forge's own code
and does **not** relicense anything listed here. Nothing in this file grants any rights to the
WRL Forge code itself — see [`COPYRIGHT.md`](COPYRIGHT.md) and [`LICENSE`](LICENSE).

Every component below is distributed under a license compatible with conveying the combined
program under `GPL-3.0-or-later`. How third-party material is vetted and recorded is described
in [`OPEN_SOURCE_PROVENANCE.md`](OPEN_SOURCE_PROVENANCE.md).

## Why this file exists

The shipped native-editor bundle is produced with `esbuild` using
`--legal-comments=none`, which strips the inline attribution/license banners that would
otherwise be embedded in the bundled JavaScript. To preserve the required attribution for
those components, their notices are reproduced here instead.

(The Electron packaging pipeline additionally emits its own bundled license blobs for
Chromium, ffmpeg, and related runtime components; those are left in place within the
distributed application and are not restated here.)

**Where these notices ship.** The packaged application carries this file, the WRL Forge
`LICENSE` (GPLv3), and `OPEN_SOURCE_PROVENANCE.md` in a `licenses/` directory beside the
executable (`package.json` → `build.extraFiles`), so a recipient of a binary has the license
text without needing the repository.

---

## MIT-licensed components

The following components are distributed under the **MIT License**. They are bundled into
the shipped application (except where noted as build-time only):

Shipped in the application bundle:

- **x_ite** — the VRML/X3D rendering engine. © Holger Seelig and contributors.
- **@codemirror/state**
- **@codemirror/view**
- **@codemirror/language**
- **@codemirror/commands**
- **@codemirror/lint**
- **@codemirror/search**
- **@lezer/highlight**
- **@lezer/common** (transitive)
- **@marijn/find-cluster-break** (transitive, via `@codemirror/state`)
- **style-mod** (transitive, via `@codemirror/view`)
- **w3c-keyname** (transitive, via `@codemirror/view`)
- **crelt** (transitive, via `@codemirror/view` / `@codemirror/search`)
- **Electron**

Build-time tooling only (not shipped in the application):

- **esbuild** — bundles the native editor.
- **electron-builder** — packages the desktop application.

A single copy of the MIT License text applies to all of the MIT-licensed components listed
above. Copyright is held by the respective authors of each component (for x_ite, © Holger
Seelig and contributors).

```
MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Fonts bundled inside x_ite

The x_ite package ships bundled fonts, which are therefore included in the packaged WRL Forge
application (inside the app's `asar` archive). x_ite carries the corresponding license files
for these fonts. They are governed by their own licenses:

- **PT Sans** — licensed under the **SIL Open Font License, Version 1.1** (OFL-1.1).
  See <https://scripts.sil.org/OFL>.
- **Droid Serif** — licensed under the **Apache License, Version 2.0** (Apache-2.0).
  See <https://www.apache.org/licenses/LICENSE-2.0>.
- **Ubuntu Mono** — licensed under the **Ubuntu Font Licence, Version 1.0**.
  See <https://ubuntu.com/legal/font-licence>.

The full text of each font license is distributed with the x_ite package inside the
application bundle.

---

## Tokyo Night colour palette (Tauri desktop app)

- **Tokyo Night VS Code theme** — © 2018-present Enkia, **MIT License**. Its colour values
  are adapted into the built-in Tokyo Night, Tokyo Night Storm and Tokyo Night Light themes
  of the Rust/Tauri application (`apps/desktop-tauri/ui/static/themes.css`). No extension code
  is shipped. The license text ships as `LICENSE-tokyo-night.txt` beside the stylesheet.
  Provenance: [`OPEN_SOURCE_PROVENANCE.md`](OPEN_SOURCE_PROVENANCE.md) §4, entry TN-1.

## Build-time icon tooling

- **@resvg/resvg-js** — licensed under the **Mozilla Public License, Version 2.0**
  (MPL-2.0). Used at build time only, as a development dependency, to rasterize the
  application icons. It is **not** shipped in the application bundle. The full MPL-2.0 text is
  available at <https://www.mozilla.org/MPL/2.0/>.

---

## Compatibility with the project license

| component | license | ships? | GPL-3.0-or-later compatible |
|---|---|---|---|
| x_ite (+ transitive CodeMirror/@lezer deps, Electron) | MIT | yes | yes — permissive, absorbable |
| PT Sans (in x_ite) | OFL-1.1 | yes (font data) | separately licensed font data, aggregated |
| Droid Serif (in x_ite) | Apache-2.0 | yes (font data) | yes — Apache-2.0 is GPLv3-compatible |
| Ubuntu Mono (in x_ite) | Ubuntu Font Licence 1.0 | yes (font data) | separately licensed font data, aggregated |
| esbuild, electron-builder | MIT | build-only | n/a (not distributed) |
| @resvg/resvg-js | MPL-2.0 | build-only | n/a (not distributed) |
| Tokyo Night colour values (Tauri app) | MIT | yes (adapted values + license text) | yes — permissive, absorbable |

**On the bundled fonts.** The three font families travel inside the `x_ite` package's own
`dist/assets/fonts/**`, each accompanied by its own license file, and are redistributed
unmodified. They are font *data* consumed at runtime, not source combined into the WRL Forge
program, and each retains its own license. No font is relicensed by this project, and their
license texts must continue to ship with them.

## Nothing here is WRL Forge's own work

No component listed in this file was authored by WRL Forge, and none may be relabelled as
such. No third-party implementation code has been copied or adapted into WRL Forge's own
sources; the Tokyo Night colour values are the one adapted item — see [`OPEN_SOURCE_PROVENANCE.md`](OPEN_SOURCE_PROVENANCE.md) §4.
