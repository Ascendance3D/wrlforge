# WD2-D Source-Proven Viewport Selection Closeout

Lane: [#28 WD2-D](https://github.com/Ascendance3D/wrlforge/issues/28) ·
contract [#29 WD2-D-R](https://github.com/Ascendance3D/wrlforge/issues/29) ·
implementation [#30 WD2-D-I](https://github.com/Ascendance3D/wrlforge/issues/30) ·
independent QA [#31 WD2-D-Q](https://github.com/Ascendance3D/wrlforge/issues/31) ·
closeout [#32 WD2-D-C](https://github.com/Ascendance3D/wrlforge/issues/32).
Architecture specification:
[`WD2_D_XITE_PICKING_CONTRACT.md`](WD2_D_XITE_PICKING_CONTRACT.md).
Research basis: [`WD2_C0_XITE_PICKING_CLOSEOUT.md`](WD2_C0_XITE_PICKING_CLOSEOUT.md).

## Status

**COMPLETED**

| | |
|---|---|
| Implementation | [PR #117](https://github.com/Ascendance3D/wrlforge/pull/117) `feat(editor): add source-proven viewport picking` |
| Merge SHA | `f39c62612d79d217c795562787d6d92764508e48` |
| Independent QA verdict | `WRLFORGE_WD2_D_INDEPENDENT_QA_RETEST_PASS` |
| Identity result | `WRONG_SOURCE_SELECTIONS = 0` |

## Delivered capability

In the Model workspace, a click in the X_ITE preview can select the exact
authored node through the existing Scene Tree / Inspector selection authority
when source identity is proven.

When identity cannot be proven, WRL Forge refuses the selection instead of
guessing. A refusal selects nothing and leaves the existing selection unchanged.

## Architecture result

- The exact source text remains canonical (`WD.md` §2). X_ITE receives the
  unmodified preview string; no source instrumentation.
- One selection authority: the existing `sceneSelection`. No second selection
  store.
- X_ITE remains the only renderer, pinned exactly to `15.1.10`.
- One private picking adapter (`src/preview/xite-pick-adapter.js`) owns every
  private X_ITE access.
- One module-private parser-hook coordinator, installed only for the active
  parse and restored in `finally`.
- Parse-local provenance; no global mutable capture slot.
- Provenance is bound to one preview generation and its exact source text, and
  discarded on every preview reload.
- No heuristic identity fallback (DEF name, type, matrix, sibling index,
  fingerprint, nearest offset).
- Picking is armed only in the Model workspace. The Code workspace is inert.
- Viewport selection does not move the source caret.

## Merge record

| | |
|---|---|
| PR | [#117](https://github.com/Ascendance3D/wrlforge/pull/117), closes #30 |
| Merge commit | `f39c62612d79d217c795562787d6d92764508e48` |
| Merge parents | `351bbf556c7feeeadba6d814f301870d36cc137c`, `a42128e945a6da80f35143d19bb85b43768dfa89` |
| Implementation commit | `23cd626974f70bea62920f122c19d922de579928` |
| Windows guard follow-up | `a42128e945a6da80f35143d19bb85b43768dfa89` `test(wd2-d): make guard scans newline-safe` |

The Windows follow-up was test-only: the G8 guard scan now normalizes CRLF
before scanning. It changed no product bytes.

## Independent QA

Verdict: `WRLFORGE_WD2_D_INDEPENDENT_QA_RETEST_PASS`

- 12/12 mandatory WD2-C0 compatibility conditions verified
- P1–P22 real-pointer fixture matrix complete
- P6 repeated siblings: 36/36
- coordinate matrix: 80/80
- Electron run 1: 333/333 assertions; run 2: 333/333 assertions
- 218 picks per run
- `WRONG = 0`
- focused tests: 56/56

Implementation-side Electron evidence is committed under `qa/wd2-d-picking/`.

## CI result

| run | event | head | ubuntu | windows | macos |
|---|---|---|---|---|---|
| [`37609129945`](https://github.com/Ascendance3D/wrlforge/actions/runs/37609129945) | PR #117 (final) | `a42128e` | PASS | PASS | PASS |
| [`37609704839`](https://github.com/Ascendance3D/wrlforge/actions/runs/37609704839) | push to `main` | `f39c626` | PASS | PASS | PASS |

The main-push Windows job confirmed the CRLF-safe G8 guard on `main`.

## Supported proven selection

- Exact Shape identity.
- The approved WD2-C simple-object promotion: Shape → its Transform.
- Anonymous twins, nested twins and repeated identical siblings, proven by
  runtime identity plus exact-span parse provenance.
- A DEF with no USE.
- LF, CRLF and Unicode source.

## Fail-closed refusal cases

| case | result |
|---|---|
| ambiguous DEF/USE (shared runtime identity, including the DEF original) | `REFUSED_AMBIGUOUS` |
| Inline / external document | `REFUSED_EXTERNAL` |
| nested external World (editing a non-primary World file) | `REFUSED_EXTERNAL` `preview-root-is-another-document` |
| PROTO instance | `UNSUPPORTED` `proto-instance` |
| authored pointing-device sensor or Anchor | `REFUSED_SENSOR_CONFLICT` |
| stale preview | `REFUSED_STALE` |
| unsupported preview mode (e.g. Mall Cybertown Fit) | `UNSUPPORTED` `preview-is-not-the-document` |
| private X_ITE API contract not proven | `COMPATIBILITY_DISABLED`; Scene Tree / Inspector selection keeps working |
| nothing under the pointer | `NO_HIT` |

`WRONG_SOURCE_SELECTIONS = 0`. A refusal is the designed result when identity
cannot be proven, not a product failure.

## Security result

WD2-D made no change to the main process, preload, IPC, CSP or network policy.
`contextIsolation` and `nodeIntegration` are unchanged. Real-input automation is
QA-only, under `qa/wd2-d-picking/`.

## Known limitations

- **BOM.** Input with a byte-order mark can fail closed with no provenance
  generation. The source text is not stripped or normalized.
- **Mall Fit.** The Fit view can refuse with `preview-is-not-the-document` even
  when the bounds-unavailable fallback visually renders the original document.
  The refusal is unnecessary but safe.
- **QA harness.** The CDP real-pointer QA harness is Linux-only. This is a QA
  harness limitation, not a WRL Forge product-platform limitation.
- **USE occurrences.** X_ITE 15.1.10 hit data carries no per-instance path, so
  the exact authored USE occurrence cannot be proven. Refusal is permanent by
  design under the current pin.

## Deferred scope

| item | tracker |
|---|---|
| multi-selection | [#57 WD2-E-G](https://github.com/Ascendance3D/wrlforge/issues/57) (under [#50 WD2-E](https://github.com/Ascendance3D/wrlforge/issues/50)) |
| transform manipulation / gizmos | [#50 WD2-E](https://github.com/Ascendance3D/wrlforge/issues/50) |
| Model/Play architecture, sensor-dispatch suppression | [#33 UI-0](https://github.com/Ascendance3D/wrlforge/issues/33) |
| PROTO-instance selection and editing | [#86 WD2-J](https://github.com/Ascendance3D/wrlforge/issues/86) |
| Inline-content selection and opening | [#118 WD2-D-INLINE](https://github.com/Ascendance3D/wrlforge/issues/118) |
| Windows/macOS real-pointer picking measurement | [#119 WD2-D-XPLAT](https://github.com/Ascendance3D/wrlforge/issues/119) |
| any X_ITE upgrade | not planned; the exact `15.1.10` pin is a WD2-D condition |

## Final lane state

| issue | state |
|---|---|
| #29 WD2-D-R | Done |
| #30 WD2-D-I | Done · QA Pass |
| #31 WD2-D-Q | Done · QA Pass |
| #32 WD2-D-C | closes with this record |
| #28 WD2-D | closes after #32 merges and final reconciliation |

No product lane is started by this closeout.
