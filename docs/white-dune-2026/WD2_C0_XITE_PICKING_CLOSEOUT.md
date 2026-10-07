# WD2-C0 — X_ITE picking provenance: closeout

Lane: [#24](https://github.com/Ascendance3D/wrlforge/issues/24) ·
closeout [#27](https://github.com/Ascendance3D/wrlforge/issues/27).
Research record: [`WD2_C0_XITE_PICKING_SPIKE.md`](WD2_C0_XITE_PICKING_SPIKE.md)
and `spikes/wd2-c0-xite-picking/`. Both are preserved byte-for-byte as the
artifact independent QA evaluated.

## Status

**ACCEPTED WITH REQUIRED COMPATIBILITY GUARDS**

| | |
|---|---|
| Spike verdict | `WRLFORGE_WD2_C0_XITE_PICKING_SPIKE_PASS_WITH_LIMITATIONS` |
| Independent QA verdict | `WRLFORGE_WD2_C0_INDEPENDENT_QA_PASS_WITH_CONDITIONS` |
| Identity result | `WRONG_SOURCE_SELECTIONS = 0` |

## Proven safe cases

A click either proves the exact authored source occurrence or refuses.

- **WD2-C Box** — the First Object Box is PROVEN and promoted to its Transform.
- **WD2-C Sphere** — in Box + Sphere, each object is PROVEN to its own Transform.
- **Anonymous twins** — identical twins and identical nested structures are
  PROVEN by runtime identity plus exact-span parse provenance, never by geometry,
  order or index.
- **Repeated siblings** — 12 identical siblings in 3 orders: 36/36 PROVEN, 0 wrong.
- **Reload / stale generation** — reload mints new runtime objects. Hits from a
  previous generation resolve to REFUSED_STALE.
- **Inline refusal** — Inline geometry belongs to the Inline's own scene.
  It is REFUSED_EXTERNAL.
- **PROTO refusal** — PROTO-instance geometry is UNSUPPORTED.
- **Sensor conflict refusal** — geometry under a pointing-device sensor or
  Anchor is REFUSED_SENSOR_CONFLICT.

## USE limitation

**The exact authored USE occurrence cannot be recovered from current X_ITE hit
data.** Every instance returns the same runtime Shape. The only per-instance
datum is a model-view matrix, and matching on it is rejected.

A shared DEF/USE runtime identity **must therefore refuse** (REFUSED_AMBIGUOUS).
This includes the DEF original. A DEF with no USE is provable.

## Private API dependency

The research basis depends on X_ITE 15.1.10 surfaces that are absent from
`x_ite.d.ts`:

- `X3D.VRMLParser.prototype.nodeStatement`, for parse-time source provenance
- `browser.touch(x, y)`
- `browser.getHit()`
- `browser.getPointerFromEvent(...)`, used only for a cross-check
- `X3DChildObject.getParents()`
- `browser.getWorld().getLayer0()` and `layer.groupNode` / `groupNodes`
- `browser.getViewport()`

Line references are in §2 of the spike record.

## Known research-harness findings

Independent QA found three limitations in the harness. They are **NOT approved
production behavior** and are deliberately left unrepaired here, so the
committed artifact stays the one QA evaluated.

1. **High: global parser prototype hook leak.** The harness wraps
   `X3D.VRMLParser.prototype.nodeStatement` globally and never restores it.
2. **Medium: global capture concurrency race.** One global mutable capture slot
   holds parse state. Overlapping parses are unsafe.
3. **Medium: caret X_ITE dependency range.** `package.json` declares
   `"x_ite": "^15.1.10"`. Private-API use needs an exact pin.

## Mandatory WD2-D conditions

1. Pin X_ITE exactly to `15.1.10`.
2. Put all private X_ITE access behind one narrow adapter.
3. Install the parser hook only for the active parse.
4. Restore the original parser method in `finally`.
5. Do not use one global mutable capture slot.
6. Bind provenance to one preview generation and exact preview text.
7. Discard the complete runtime mapping after every preview reload.
8. Disable viewport picking if any required private API contract fails.
9. Never fall back to structural, DEF, matrix, sibling, nearest-offset, or fingerprint matching.
10. Keep USE, Inline, PROTO, sensor, stale-preview, and other refusal states as first-class results.
11. Add compatibility tests that fail loudly if X_ITE private internals change.
12. Keep scene-tree and Inspector selection working when viewport picking is disabled.

## Owner disposition

**WD2-C0 ACCEPTED WITH REQUIRED COMPATIBILITY GUARDS.**

**PRIVATE X_ITE PICKING BASIS APPROVED FOR WD2-D WITH REQUIRED COMPATIBILITY GUARDS**

## Scope

WD2-C0 implemented **no production viewport selection**. It changed no
production X_ITE integration and did not pin X_ITE. WD2-D
([#28](https://github.com/Ascendance3D/wrlforge/issues/28)) remains Backlog and
needs separate owner authorization.
