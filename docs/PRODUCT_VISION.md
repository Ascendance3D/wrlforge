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

A future custom Rust VRML97/X3D renderer is the approved direction. X_ITE stays
as the temporary renderer until that lane is approved and built. Renderer
implementation has not started.

## Platforms

Linux, Windows, and macOS.

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

The first visual creation workflow (VISUAL-1) is being built in the Rust/Tauri
application: New World, Create Box/Sphere/Cylinder/Cone as
Transform/Shape/Appearance/Material, Scene Tree selection, Inspector editing of
transform and diffuse color, undo/redo, Save As, and reopen. Viewport picking
and 3D transform gizmos are the next planned visual features.

None of the groups above should be read as shipped unless a dated status
document says so.
