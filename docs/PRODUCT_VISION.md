# WRL Forge product vision

This is a **product direction document**. It records the owner's approved
goal. It is **not** a claim that any feature listed here is complete.

## Goal

WRL Forge is a complete VRML97/X3D creation studio. It combines direct visual
3D authoring (inspired by Spazz3D) with professional VRML source authoring
(inspired by VrmlPad) in one application.

## Clean-room independence

WRL Forge is an independent, clean-room implementation built from public
standards (ISO/IEC 14772-1 VRML97, X3D) and documented behavior. No proprietary
Spazz3D or VrmlPad code, assets, manuals, artwork, or interface designs are
copied. WRL Forge is not the separate FreeWRL project and is unrelated to it.

## One document, many views

The exact source text is the single canonical document (see `WD.md`). Every
user interface is a view over that document: the 3D viewport, the Scene Tree,
the Inspector, and the source editor. All of them edit through the same shared
Rust commands, so no view owns a private copy of the scene.

PhotoCraft (https://github.com/storytold/photocraft) is cited only as an
architecture reference for the idea "one document engine, shared commands,
multiple UIs". No code is copied from it, and WRL Forge keeps its own
framework.

## Principles

- The 3D viewport is not a secondary preview.
- The source editor is not the only way to create objects.
- A user can create objects and worlds without writing source.
- A user always has direct access to the exact source.

## Renderer direction

X_ITE remains the default renderer. The Rust (wgpu) renderer remains
experimental: it exists behind an opt-in test switch (NATIVE_RENDER_1) and is
disabled by default. No X_ITE removal is planned; X_ITE compatibility code is
kept.

## Platforms

Linux is the primary platform, followed by macOS 15+. Windows 11 is a planned target for a future phase. Mobile remains out of scope.

## Planned feature groups

- **Modeling and geometry**: create and edit primitives and richer geometry.
- **Materials and textures**: appearance, color, and texture assignment.
- **Object transforms and hierarchy**: move, rotate, scale, and organize nodes.
- **Viewpoints, lights, and world settings**: cameras, lighting, and environment.
- **Animation and routing**: interpolators, sensors, and ROUTE editing.
- **PROTO and advanced VRML authoring**: prototypes and reusable components.
- **Source editing and diagnostics**: syntax-aware editing with standards-based feedback.
- **Cybertown Mall Item and World Project workflows**: profile-specific validation and packaging.

## Current status

The first visual creation workflow (VISUAL-1) is built in the Rust/Tauri
application: New World, Create Box/Sphere/Cylinder/Cone as
Transform/Shape/Appearance/Material, Scene Tree selection, Inspector editing of
transform and diffuse color, undo/redo, Save As, and reopen.

Source-proven 3D viewport selection (WD2-D, issue #28 closed) is built: in the
Model workspace, clicking an object in the viewport selects the exact authored
node (or its simple-object Transform) via shared selection authority, with
fail-closed refusal when source identity cannot be proven. Windows/macOS
real-pointer picking QA (#119) and Inline content selection (#118) remain open.
Direct Translate, Rotate, and Scale transform tools are implemented in the
Rust/Tauri beta. Shift-drag snaps rotation to 15 degree steps. Translation and
scale snapping, multi-selection, and independent QA remain open under the parent
WD2-E workstream (#50), which is not yet accepted.

Crash recovery (RUST-STAB-1, 2026-10-11) is built in the Rust/Tauri
application: unsaved work is kept in private recovery records and offered at
the next start (Restore, Discard, Later). See
`docs/architecture/RUST_STAB_1_RECOVERY.md`. WRL Forge is not ready for public
release.

None of the groups above should be read as shipped unless a dated status
document says so.
