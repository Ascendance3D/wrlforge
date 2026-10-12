// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1: the wgpu viewport renderer (`gpu`) and the render thread
//! that owns it (`thread`).
//!
//! This crate draws a `wrlforge_scene::RenderScene` and answers pick
//! requests; it never sees the document text, a path, a file or the network.
//! Platform surface creation (the only `unsafe` step) belongs to the desktop
//! host, which passes a surface factory into `thread::spawn`.
//!
//! Rules carried over from Step 0 (evidence in `docs/architecture/NATIVE_RENDER_1.md`):
//! no `unwrap`/`expect`/panic on a GPU path; every GPU wait has a limit
//! (`GPU_WAIT`); the present mode is an explicit policy, never a silent FIFO
//! fallback (D8); a lost device is reported and never restarted (D9).
#![forbid(unsafe_code)]

pub mod gpu;
pub mod thread;

pub use gpu::{Frame, GpuInfo, PresentPolicy, Renderer, GPU_WAIT};
