# Open Source Provenance Policy

**Status:** current project policy. Supersedes the WD0-era
`docs/white-dune-2026/GPL_PROVENANCE_BOUNDARY.md`, which was written for WRL Forge's
former MIT posture and has been removed from `main` (Git history retains it).

> This is engineering provenance guidance, not legal advice. It records how WRL Forge
> tracks the origin and licensing of the code it ships. It is not a legal opinion and
> does not substitute for one.

---

## 1. Project license

Original WRL Forge code and documentation are licensed **`GPL-3.0-or-later`** unless a
file says otherwise. See [`LICENSE`](LICENSE) for the canonical GNU GPL v3 text and
[`COPYRIGHT.md`](COPYRIGHT.md) for the copyright statement.

Third-party material incorporated into WRL Forge **retains its own copyright and
license**. The project license applies to WRL Forge's own work; it does not relicense
anyone else's.

The practical rule for anything you bring in:

> It must be license-compatible with distributing the combined program under
> `GPL-3.0-or-later`, and every required attribution, copyright line, and license
> notice must be preserved.

## 2. What is now permitted

Open-source implementation material — source, algorithms, data tables, fixtures — **may
be read, searched, studied, compared, adapted, translated, ported, and incorporated**
into WRL Forge when:

1. its license is compatible with `GPL-3.0-or-later` for the proposed combination; and
2. its provenance is recorded here (§4); and
3. its upstream copyright notices and license text are preserved in the tree and in the
   distributed artifact.

This is a genuine change of direction. The former clean-room prohibition existed only
because WRL Forge was MIT-licensed and could not absorb copyleft code. That constraint
is gone.

### 2.1 White Dune

**White Dune is permitted implementation material.** It is
[GPL-2.0-or-later](https://sourceforge.net/projects/white-dune/), which is compatible
with distributing a combined work under `GPL-3.0-or-later` (the "or later" election in
White Dune's own headers is what makes the upgrade lawful).

Two cautions that are engineering facts, not policy hedging:

- **Verify the license of the specific artifact you use.** Do not assume every White
  Dune file, version, or packaged binary carries identical license metadata. The
  1.930 source tree's headers say "version 2 of the License, or (at your option) any
  later version"; a different file or a different release may not, and a `.deb` of
  binaries proves nothing about implementation licensing at all.
- **Record what you took.** Anything copied, adapted, or closely translated gets a §4
  entry and preserved upstream copyright headers. "Conceptually informed only" — you
  read it, then wrote your own — should still be recorded, because a reviewer cannot
  tell the difference from the diff alone.

Reuse is a **separate approved lane** from the licensing transition itself. No White
Dune material was imported as part of adopting the GPL.

### 2.2 Other open-source projects

The rule is general, not White-Dune-specific. FreeWRL, X3D toolkits, and any other
open-source VRML/X3D implementation are usable on the same terms: verify the license,
confirm compatibility, preserve notices, record provenance.

## 3. What remains prohibited

Changing WRL Forge's license did **not** widen what may be copied. Still forbidden:

- **Proprietary source** without explicit written permission.
- **Leaked source**, or source obtained in violation of a licence agreement, terms of
  service, NDA, or access control.
- **Software with no usable license** — "found on the internet", "no LICENSE file",
  abandonware whose rights holder is unknown.
- Code whose license is **GPL-incompatible for the proposed combination**.
- **Snippets of unestablished provenance** — a Stack Overflow answer, a gist, a pasted
  fragment whose origin cannot be traced.
- **Removing or falsifying attribution**, upstream copyright lines, or license headers.

Reverse-engineering research artifacts of proprietary tools (`RE-ARTIFACTS`,
`blaxxun-cs-RE`, and similar) remain **implementation-prohibited**. They are not
open-source material and a GPL project license does nothing to change that.

**Reading is not incorporating, and the distinction is narrow.** Where the owner
has explicitly authorized a research tree for a named lane, that tree may be
**read and studied** in order to establish independent facts — what a historical
runtime did, what a vendor documented, what a name meant. Those *facts*, and a
**paraphrase** of them, may be recorded in WRL Forge. The **material itself** may
not: no source, no manual prose, no decompilation output, no image, asset or
binary is copied, adapted, translated, vendored or committed, and a verbatim
sentence of proprietary documentation is not committed to this public repository
either. §4.1 records each authorization, and every such record must state the
boundary explicitly. This paragraph authorizes nothing on its own — it describes
what an owner authorization may and may not permit, and it does not widen
`blaxxun-cs-RE` or `RE-ARTIFACTS` beyond the specific lanes §4.1 names.

The credential, member-data, licence-key and login/crypto exclusions are
**unaffected by any such authorization** and are never opened, recorded, quoted
or paraphrased.

> **Provenance before convenience.** If you cannot establish where something came from
> and under what license, do not copy it into WRL Forge. An unprovable origin is a
> defect, not a detail.

## 4. Third-party provenance register

Every piece of third-party implementation material incorporated into WRL Forge is
recorded here with:

| field | meaning |
|---|---|
| Upstream project | name |
| Source | repository or download URL |
| Version | tag, release, or commit, where practical |
| Component | the upstream file(s) or module used |
| Authors | upstream copyright holders |
| License | SPDX identifier |
| Destination | the WRL Forge file(s) |
| Mode | `copied` · `adapted` · `translated` · `conceptually informed only` |
| Local changes | substantial modifications made |
| Notices | what must ship, and where it ships |

### Current entries

**TN-1 — Tokyo Night colour values (UI-THEME-1)**

| field | value |
|---|---|
| Upstream project | Tokyo Night VS Code theme |
| Source | <https://github.com/tokyo-night/tokyo-night-vscode-theme> |
| Version | commit `7c0f11eaef322f293621ca7befe462214b7ea468` (package `1.1.2`) |
| Component | `themes/tokyo-night-color-theme.json`, `themes/tokyo-night-storm-color-theme.json`, `themes/tokyo-night-light-color-theme.json` (colour values only) |
| Authors | Copyright (c) 2018-present Enkia |
| License | `MIT` |
| Destination | `apps/desktop-tauri/ui/static/themes.css` |
| Mode | `adapted` — colour values, not code. No theme JSON, token-scope rule or extension file is copied or shipped. |
| Local changes | Upstream workbench roles are mapped onto WRL Forge's own `--wf-*` semantic tokens. Translucent upstream values are pre-blended onto their surface. Some text, selection, border, scrollbar and accent values are adjusted (lightened on dark themes, darkened on Light) to meet WCAG AA 4.5:1 / 3:1; the pairs are asserted by `apps/desktop-tauri/protocol/src/theme/tests.rs`. |
| Notices | The upstream copyright and commit are in the `themes.css` header. The full MIT text ships as `apps/desktop-tauri/ui/static/LICENSE-tokyo-night.txt`, copied into `dist/` by `build-ui.sh`. Listed in `THIRD_PARTY_NOTICES.md`. |

No third-party implementation **code** has been copied, adapted, or translated into WRL
Forge's own source. Everything under `src/`, `renderer/`, `main.js`, `preload.js`,
`validator.js`, and `scripts/` is original work.

Third-party components that WRL Forge **depends on and redistributes unmodified**
(x_ite, CodeMirror, Electron, and their bundled assets) are not implementation
derivations and are inventoried separately in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

### Standards data is not third-party code

`src/vrml/node-schema.js` is generated from two license-clean **fact** sources — the
ISO/IEC 14772-1 (VRML97) node reference and the MIT-licensed `x_ite.d.ts` type
declarations. It extracts names, types, access categories, and default literals; it
reproduces no standards prose and no implementation. That remains true and is asserted
by `test/vrml/node-schema.test.js`.

That test is a **provenance statement about that one generated file**, not a residue of
the old prohibition. If a future lane derives schema content from another source, the
test and this section change together.

### 4.1 Research & Reference

The register above records **incorporated implementation material**. This section records
something different and deliberately kept separate: external implementation sources that
were **deliberately studied** — for architecture, algorithms, UX/workflow, or
cross-checking — where **no source code has been copied, adapted, translated, or
incorporated**.

A record here is **not** a claim that any WRL Forge file became derivative. Reading a
GPL-compatible implementation does not make the reader's own independently written code a
derivative work, and this section must never be read as implying otherwise. It exists so
that a reviewer can see what was consulted and when, and so that a later port has an
honest starting point.

**Transition rule.** If code that was only studied later becomes **conceptually informed,
translated, or adapted** into WRL Forge, create or update the corresponding entry in the
§4 production register **at that time**, with the mode set accordingly. Moving between
the two sections is the normal, expected lifecycle — a Research & Reference record is a
provenance *lead*, not a permanent exemption from §4.

#### Current records

**WD-OSS-A1 — White Dune implementation & architecture audit**

| field | value |
|---|---|
| Project | **White Dune** (`wdune`) |
| Local source | 1.930 source evidence, `~/Projects/white-dune-archive/white_dune-1.930-source-evidence/` (outside every Git repository) |
| Upstream | `github.com/mufti11/white_dune`, commit **`62f9ab457004666143160909a7e348bf87c107e6`** (1.956, 2020-09-02) — last upstream commit; project dormant |
| License posture | White Dune-authored code is **`GPL-2.0-or-later`** (1,220 of 1,303 `src/` files), compatible with WRL Forge's `GPL-3.0-or-later` via the "or later" election. The tree also contains **separately licensed vendored third-party code** — BSD-3-Clause, MIT, LGPL, permissive — plus generated Bison/flex output and 17–18 files carrying **no grant at all**. Licensing is **not uniform**; verify per artifact. |
| Purpose | implementation and architecture audit, to decide the next WRL Forge lane |
| Resulting document | [`docs/white-dune-2026/WD_OSS_A1_IMPLEMENTATION_ARCHITECTURE_AUDIT.md`](docs/white-dune-2026/WD_OSS_A1_IMPLEMENTATION_ARCHITECTURE_AUDIT.md) |
| Code incorporated | **None.** No White Dune implementation code, fixture, example scene, or asset was copied, adapted, translated, vendored, submoduled, or committed. No file under `src/`, `renderer/`, `scripts/`, `main.js`, `preload.js` or `validator.js` was modified by the audit. |
| Status | research only; §4 register correctly still reads **None** |

**Reuse leads recorded by that audit** — none acted on, each requiring a §4 entry *before*
any code lands: White Dune's handle protocol and `Scene3DView::Handle3D` frame math
(adaptation candidates), and its command/undo model, `Proto` unified type descriptor, and
ROUTE/`IS` authoring UI (architecture/UX reference). Its **document core is rejected
outright** — a live object graph with full regeneration on write, incompatible with
WRL Forge's byte-preserving document invariant.

**If geometry algorithms are ever wanted**, take Poly2Tri (BSD-3-Clause), catmull-clark
(MIT) and FTGL (MIT) from **their own upstreams**, not through White Dune: better terms,
cleaner provenance, active maintenance. Those would be `THIRD_PARTY_NOTICES.md` entries.

**VISUAL-3A — White Dune handle architecture (translation gizmo)**

| field | value |
|---|---|
| Project | **White Dune** 1.956 |
| Local source | `~/Projects/cybertown/white_dune` (owner-supplied for VISUAL-3A; its own checkout, outside the WRL Forge repository; opened **read-only**, nothing modified or copied) |
| Files studied | `src/Node.h` (`getHandle()`, `setHandle()`, `drawHandles()` declarations), `src/TransformNode.cpp` (`getHandle()` / `setHandle()` / `drawHandles()` for `TRANSLATION_X/Y/Z`), `src/Scene3DView.cpp` (handle drag: project / unproject, `constrainLine`) |
| License verified | each studied file's own header: Copyright (C) 1999 Stephen F. White (`Node.h` also 2005 J. "MUFTI" Scheurich), GNU GPL "version 2 … or (at your option) any later version" — `GPL-2.0-or-later` |
| Mode | **conceptually informed only.** The idea taken is the split between a handle that REPORTS a position for one field (`getHandle`) and a drag that turns a constrained handle position into a new value for that field (`setHandle`), drawn per node (`drawHandles`). |
| Not taken | No code, data structure, constant, numeric scene path or source writer. White Dune's handles live in the Transform's rotated local frame and its drag unprojects at the handle's screen depth; WRL Forge's VISUAL-3A gizmo uses **world** axes for top-level Transforms and a closest-point-between-lines axis drag (`apps/desktop-tauri/protocol/src/gizmo.rs`), binds the rendered node by its proven top-level statement index (never a White Dune-style numeric scene path as identity), and writes one source token through the existing field-edit planner (`crates/wrlforge-vrml/src/manipulate.rs`). |
| Destination files | `crates/wrlforge-vrml/src/manipulate.rs`, `apps/desktop-tauri/protocol/src/gizmo.rs` (each names the design reference in its header) |
| Status | design reference; §4 register correctly still reads **None** for White Dune |

**WD1.7-E0 — blaxxun compatibility-evidence study**

| field | value |
|---|---|
| Vendor / product | **blaxxun interactive** — *blaxxun Contact 3D* (4.x–5.x) and the server products it shipped with |
| Reference root | `blaxxun-cs-RE`, in the workspace and **outside this repository**. Not redistributed, not vendored, not a dependency. |
| Artifacts studied | the vendor's own **shipped 3D authoring/administrator documentation**, in two product generations: *Virtual Worlds Platform 5.1* and *Community Server 7.0* |
| Authorization | Ryan, **2026-08-29**, for the WD1.7-E lane specifically. Recorded here because WD1.7-A deliberately did **not** have it, and its conclusions were correct only within that narrower boundary. |
| Purpose | to decide whether any **named** compatibility profile is defensible, and on what evidence — see [`docs/white-dune-2026/WD1_7_E_COMPATIBILITY_POLICY_DECISION.md`](docs/white-dune-2026/WD1_7_E_COMPATIBILITY_POLICY_DECISION.md) |
| Resulting implementation | `src/vrml/compatibility.js` (WD1.7-E1), documented in [`docs/white-dune-2026/WD1_7_E1_BLAXXUN_CONTACT_PROFILE.md`](docs/white-dune-2026/WD1_7_E1_BLAXXUN_CONTACT_PROFILE.md) |
| **Incorporated** | **None.** No vendor source, manual text, decompilation output, image, asset or binary was copied, adapted, translated, vendored or committed. |
| **What WRL Forge does hold** | independently established **facts** — that a behaviour is documented, in which product generation, at which reference-root-relative path — plus WRL Forge's **own paraphrase** of the rule, an evidence tier, and a classification. No proprietary sentence is committed. |
| Runtime access | **none.** `src/vrml/compatibility.js` is pure and browser-safe, requires no filesystem module, and reads nothing from the reference root at runtime. An end user is not expected to hold it. |
| Excluded throughout | credentials, member data, licence keys, login and crypto material — never opened, recorded, quoted or paraphrased. |
| Status | **research and reference only**; the §4 production register correctly still reads **None** for this vendor. |

**Not authorized by that record, and deliberately not done:** decompilation,
binary analysis, new reverse-engineering work, or execution of the preserved
`blaxxun Contact` builds. A black-box execution lane against an already-installed
copy is a **separate future authorization** (WD1.7-E0 §13), and nothing in
WRL Forge claims an executed (`a3`) evidence tier until it has run.

## 5. Source headers

**Do not mass-add GPL boilerplate headers to existing files.** The root `LICENSE` plus
`package.json` metadata are sufficient for WRL Forge's original files, and a sweeping
header commit would bury real history under churn.

For **new** files, a single SPDX line is encouraged where it is unobtrusive:

```js
// SPDX-License-Identifier: GPL-3.0-or-later
```

For **third-party-derived** files, preserve the upstream copyright and license header
verbatim and add WRL Forge's own notice **alongside** it. Never replace an upstream
header with a WRL Forge one — that is exactly the falsified-attribution case §3 forbids.

## 6. Corpus and research boundaries

The spike harnesses under `spikes/` guard their corpus roots with hard path assertions
that **throw** rather than silently skip. Those guards are **corpus hygiene**, not
license policy: a VRML97 semantics sweep measures authored Cybertown content, and a
modeling tool's C++ source tree or a reverse-engineering artifact directory is not that.
The guards stay, and their rationale is unchanged by the license transition.

**A dedicated oracle reader is not an exemption.** A future lane may want to read named
White Dune files as a **cross-check oracle** (for example, comparing field-constraint
tables against the ISO mirror). That is permitted and needs **no change to any guard**,
because the two mechanisms never meet: a corpus guard rejects a *root offered to generic
enumeration*, whereas an oracle reader opens **explicitly named files by explicit path**
and never enumerates a corpus at all.

Such a reader must be: read-only · addressed by explicit path, never by discovery ·
**non-normative**, with every discrepancy adjudicated against ISO/IEC 14772-1 (§7) ·
recorded under §4.1 Research & Reference.

**Do not add a corpus-guard exemption to enable one.** Needing to weaken a guard is a
sign the tool is enumerating when it should be addressing.

## 7. Standards remain the normative authority

WRL Forge is standards-first. Correctness questions about VRML97 are settled by
**ISO/IEC 14772-1** and the Web3D specifications — never by any single implementation's
behaviour, White Dune's included.

Another implementation can teach *technique*: how to structure a scene tree, what a
field inspector needs, how a ROUTE graph is usefully drawn. It cannot establish what the
language *means*. Where an implementation and the standard disagree, the standard wins
and the divergence is classified as a compatibility item (see [`WD.md`](WD.md) §9).

## 8. Featured artwork and content

Creative content — VRML scenes, models, textures — displayed *by* WRL Forge or featured
on the project website is **not** covered by the software license. See
[`FEATURED_CONTENT_POLICY.md`](FEATURED_CONTENT_POLICY.md).
