# RUST-STAB-1 — crash recovery and source safety

Lane: `feature/rust-stab-1-recovery`, stacked on PR #136
(`feature/visual-4b-rotate-scale`, head `96aef4ff65d61efa3cd3b0d0a2142634618a604b`).
Scope: stability only. No modeling feature was added.

Supported platforms for this lane: Linux (tested) and macOS 15+ (same code
paths; not run in this lane). Windows and mobile are not supported targets.
WRL Forge is not ready for public release.

## 1. Behavior before this lane

| situation | behavior at `96aef4f` |
|---|---|
| dirty document | Rust held the only copy (`wrlforge_document::Document`, a UTF-8 `String` plus revision, saved text and undo stacks). Nothing was written anywhere until Save. |
| untitled document | New World had no path; its text existed only in memory. |
| Save / Save As | `files::safe_save` (temp, fsync, verify, backup, atomic rename). Save refused on an external change (`Conflict`). |
| Undo / Redo | In memory only; every undo or redo advances the revision. |
| window close | Closed at once, **without a question**, even with unsaved changes (only the native viewport could defer the close). |
| application shutdown | Same: unsaved work was lost. |
| unexpected exit | All unsaved work was lost. No recovery store existed (`recovery-store.js` was PENDING in the migration map). |
| outside change | A 3-second stat poll showed a banner; Save refused to overwrite. |

The canonical source text and its file format (`plain` / `gzip`) are owned
by Rust (`service::Session`). The WebView `<textarea>` is a projection and is
never a recovery source. X_ITE is never a source of document data.

## 2. Base defects found and fixed

The PR #136 head did not compile. Commit `5fb1db0` (VISUAL-4B) left
`Service::translate_target` with a duplicated `Err` arm and an unclosed `Ok(`,
and never declared `ui/src/smoke/rotscale.rs` as a module. The first commit of
this lane fixes only those two breaks (no behavior change). PR #136 needs the
same fix before it can merge.

## 3. Record format (version 1)

One file per document session: `rec-<32 hex>.wfr`.

```
offset size  field
0      8     magic  "WFRECOV\n"
8      2     version (u16 LE) = 1
10     4     header length (u32 LE, max 16 KiB)
14     4     header CRC-32
18     8     payload length (u64 LE, max 64 MiB)
26     4     payload CRC-32
30     n     header: JSON
30+n   m     payload: the EXACT canonical text, UTF-8
```

The header (`recovery::Header`, unknown keys refused): `record` id (must equal
the file name), owning `instance` id, change `sequence`, document `revision`,
`savedAtMs`, `format` (`plain`/`gzip`), `untitled`, a safe display `name` (a
file name only), `original` file identity (path, size, mtime, CRC-32, FNV-1a
64) or none for a new world, `textBytes` and `textFnv64`.

The payload is the source text itself, so a BOM, CRLF / LF / lone CR, mixed
endings, whitespace, comments, DEF/USE names, PROTO bodies, Unicode and
malformed source are kept byte for byte. A gzip document is recovered as text
plus `format: gzip`; Save As writes gzip again.

Reading rejects, each with its own message: short file, bad magic,
unsupported version, oversize header or text, truncation, trailing bytes,
header or payload checksum failure, invalid header JSON or fields, a name
that is not a plain file name, id/file-name mismatch, invalid UTF-8.

## 4. Storage location and safety

* Location: `<app data dir>/recovery/` (Tauri `app_data_dir`, for example
  `~/.local/share/<identifier>/recovery` on Linux). With `--config-dir DIR`
  (every automated test) it is `DIR/recovery/`.
* The folder is created with mode `0700`; a symlink or non-folder is refused
  (recovery is then unavailable, with a warning). Files are created with
  `create_new` and mode `0600`.
* Write: encode → temp file `.tmp-<instance>-<record>-<n>` → `fsync` → rename
  over the record → `fsync` the folder. A failed write removes only its own
  temp file; the older record stays. Leftover temp files of dead processes are
  removed at the next start.
* Ordering: each committed state gets a process-wide sequence number. A write
  commits only if its sequence is newer than the last applied one for that
  record, checked under the same lock as the rename. Save, undo-to-saved and
  Discard remove the record and advance the sequence in that same critical
  section, so a delayed write cannot recreate an obsolete record and an older
  snapshot cannot replace a newer one.
* Ownership: each process holds an exclusive lock on
  `instances/<instance>.lock` for its whole life; the kernel frees it on any
  exit, including SIGKILL. Only records whose owner lock is free are offered.
  A second running window never sees or deletes the first window's records.
* No record field selects a write path. A restored document has no path; only
  Save As (a native dialog) can give it one. The original path is used only to
  compare the original file's identity, read-only, at most 64 MiB.
* Schedule: a write runs 750 ms after the last change and at most 3 s after the
  first unwritten change, on a background thread. Typing does not cause a disk
  write per key.

**Limit:** changes made after the last completed recovery write are lost if
the process dies before the next write (up to about 3 s of work).

## 5. Lifecycle

* Every session access goes through `Service::with`. A new revision is a new
  recovery state: dirty → scheduled write; clean → the record is removed now.
  This covers typing, Inspector edits, Create, Move, Rotate, Scale, Extrusion
  and list edits, add-field, Undo, Redo and Reload with one hook.
* The recovery path is separate from Save. It never writes a document file,
  never changes the canonical text, the revision, or the undo history. A failed
  recovery write changes nothing in the document and shows a warning banner
  ("Crash recovery could not save a recovery copy …"); unavailable storage
  shows "Crash recovery is unavailable …".
* Save / Save As success → the document is clean → its record is removed at
  once. A failed Save, a Save conflict, or a canceled Save keeps the record.
* New World on unsaved changes → native confirmation → Discard removes that
  record. Cancel keeps it.
* Window close with unsaved changes (new in this lane) → native confirmation
  "Discard changes / Cancel". Discard removes those records and closes; Cancel
  keeps the window, the work and the records.
* A normal exit writes any scheduled recovery copy (`RunEvent::Exit`).
* Close of a dirty session without a decision keeps its record (conservative).

## 6. Startup choice

At start the UI asks Rust (`recovery_list`) for records of exited processes. If
there are any, a dialog lists each with a safe name, "file" or "new world",
format, size and recovery time (local `YYYY-MM-DD HH:MM`). No path is shown. A
note appears when the original file changed or is missing. Damaged records
are listed with their reason.

* **Restore** opens the exact text in a NEW untitled, dirty document, in the
  record's format. It is refused while the shown document has unsaved changes.
  The original file is not touched. Save goes through Save As. The record is
  re-owned by this process (same text) and stays until that save or an
  explicit discard. Normal editing and Undo work on the restored text.
* **Discard…** asks a native confirmation, then deletes only that record
  (never a live window's record).
* **Later** hides the dialog and keeps every record for the next start.

More than one record is offered at once; acting on one leaves the others.

## 7. Tests

Rust unit tests (`src-tauri/src/recovery.rs`, 8): exact round trip (BOM, CRLF,
lone CR, Unicode, malformed, empty); every rejection reason; private modes and
atomic writes; a simulated mid-write failure and a real read-only folder keep
the older record; late write after retire; older snapshot after newer; owner
locks (a live owner hides records, a released lock offers them); damaged,
foreign and symlinked files; refused symlinked folder; bounded schedule under
continuous typing; safe names.

Service tests (`service::recovery_tests`, 6): crash (session dropped without
any save path) then restore of plain, gzip, BOM+CRLF+lone CR+Unicode and an
untitled new world, byte for byte; double restore refused; plain Save refused
on restored work; originals unchanged; restored work edits, undoes, saves as
gzip, and completes its record; every change kind reaches recovery and Save
completes it; pending write after Save does not recreate the record;
undo-to-saved removes it; conflict, failed save and close keep it; Later;
Discard of one record leaves others; failed recovery write warns and changes
nothing; unavailable storage; damaged record reported, not restorable, deleted
only on request.

`wrlforge-document`: `new_unsaved` is exact, dirty until saved, undoable.

Source preservation (`src-tauri/tests/flux_regression.rs`, 6 tests, original
fixtures in `tests/fixtures/flux-regression/`, based on the corrected Flux
Studio research; no Flux output or assets): ten valid VRML97 fixtures (animation
without a startTime ROUTE, TouchSensor chain, Shape without Appearance,
explicit normals and normalIndex, quad IndexedFaceSet, high-precision
Background colors, DEF/USE shared Material, PROTO + Script + Inline, duplicate
DEF names, Cylinder and Cone fields) × {LF, CRLF, BOM+CRLF, gzip}: open → Save
→ Save As keeps the text byte for byte, gains no `Material`, `startTime` or
`creaseAngle`, keeps ROUTE order. No error diagnostic on any valid fixture
(duplicate DEF included). Inspector edit of `cycleInterval`, Inspector edit of
the shared Material, and a Move of the quad cube each change exactly one token
(quads stay quads) and Undo restores the exact text. Six malformed fixtures
open and save without a byte change; only the two syntax-malformed ones are
asserted to report diagnostics.

Headless smoke (`./smoke.sh --headless --recovery`, three processes):

1. `--smoke-recovery-crash`: edits plain, gzip, BOM+CRLF+CR+Unicode files and
   creates a Box in a new world; a clean file and an edited-then-undone file
   must make no record. It waits until the background worker has written all
   four records and each re-reads byte-identical, then the script kills the
   process with **SIGKILL** (exit 137).
2. `--smoke-recovery-restore`: the dialog shows four records and one damaged
   record (made by the script after the kill). Real clicks: Restore,
   type + Ctrl+Z, Save (as) for BOM/CRLF and gzip; Discard with confirmation; a
   canceled window close; Later. Rust compares every text byte for byte.
3. `--smoke-recovery-later`: the records kept by Later and by the canceled
   close are offered again; Restore the untitled world exactly; Discard the
   damaged and the other record.

## 8. Not in this lane (recorded for later)

Full ROUTE semantic validation (m2, m3, m6 report nothing today); a new
DEF/USE scope graph; Flux `.fxw` import; Spazz3D `.spz` import; mesh tools; a
materials workflow; X_ITE replacement (X_ITE stays the default renderer; the
Rust renderer stays off by default); a new source editor; macOS packaging;
a recovery view of a record's text before Restore; recovery of multiple
documents per window (the UI shows one document); Windows (not a target).
