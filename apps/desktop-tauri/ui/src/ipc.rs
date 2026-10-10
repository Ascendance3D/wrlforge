// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed IPC to the Rust backend through Tauri's injected `invoke`, and the
//! narrow X_ITE preview adapter. These two `extern` blocks are the UI's ONLY
//! contact with JavaScript.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wrlforge_desktop_protocol as p;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

// TEMPORARY JavaScript dependency: X_ITE is a JavaScript renderer. The adapter
// (`static/preview-adapter.js`) is the only handwritten JS in the application.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = load, catch)]
    async fn preview_load_js(text: &str, meta: JsValue, base: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = loadState, catch)]
    fn preview_load_state_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = capture, catch)]
    async fn preview_capture_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = pick, catch)]
    fn preview_pick_js(client_x: f64, client_y: f64) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = retire, catch)]
    fn preview_retire_js(reason: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = pixel, catch)]
    async fn preview_pixel_js(client_x: f64, client_y: f64) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = pickStatus, catch)]
    fn preview_pick_status_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = probe, catch)]
    fn preview_probe_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = coverage, catch)]
    async fn preview_coverage_js() -> Result<JsValue, JsValue>;
    // VISUAL-3A translation gizmo (xite-gizmo-adapter.js through the adapter).
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoCamera, catch)]
    fn gizmo_camera_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoLocate, catch)]
    fn gizmo_locate_js(seq: f64, from: f64, to: f64) -> Result<JsValue, JsValue>;
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoBind, catch)]
    fn gizmo_bind_js(
        seq: f64,
        from: f64,
        to: f64,
        index: f64,
        name: &str,
        x: f64,
        y: f64,
        z: f64,
    ) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoSet, catch)]
    fn gizmo_set_js(x: f64, y: f64, z: f64) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoRestore, catch)]
    fn gizmo_restore_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoRelease, catch)]
    fn gizmo_release_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoUnbind, catch)]
    fn gizmo_unbind_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = gizmoRendered, catch)]
    fn gizmo_rendered_js() -> Result<JsValue, JsValue>;
    // VISUAL-3A1 camera carry.
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = cameraCapture, catch)]
    fn camera_capture_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = cameraResult, catch)]
    fn camera_result_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = cameraState, catch)]
    fn camera_state_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = cameraTraceStart, catch)]
    fn camera_trace_start_js() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = cameraTraceStop, catch)]
    fn camera_trace_stop_js() -> Result<JsValue, JsValue>;
}

/// The renderer's camera as last drawn, or `None` when it cannot be read.
pub fn gizmo_camera() -> Option<wrlforge_desktop_protocol::gizmo::Camera> {
    let s = gizmo_camera_js().ok()?.as_string()?;
    serde_json::from_str(&s).ok()
}

/// VISUAL-3A1: the plain-data runtime chain of the ONE runtime node preview
/// generation `seq` recorded for the preview span `[from, to)`, for Rust to
/// prove (`doc_translate_prove`).
pub fn gizmo_locate(seq: u64, from: u64, to: u64) -> Result<p::PickSnapshot, String> {
    let raw = gizmo_locate_js(seq as f64, from as f64, to as f64)
        .map_err(js_err)?
        .as_string()
        .ok_or("non-string span snapshot")?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// Bind the runtime node of `[from, to)` in generation `seq` -- the one Rust
/// proved. `index`, `name` and `t` are consistency assertions only (root
/// position, DEF name or "", source translation). `Err` is the reason.
pub fn gizmo_bind(
    seq: u64,
    span: (u64, u64),
    index: u32,
    name: &str,
    t: [f64; 3],
) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct R {
        ok: bool,
        reason: Option<String>,
    }
    let raw = gizmo_bind_js(
        seq as f64,
        span.0 as f64,
        span.1 as f64,
        index as f64,
        name,
        t[0],
        t[1],
        t[2],
    )
    .map_err(js_err)?
    .as_string()
    .unwrap_or_default();
    match serde_json::from_str::<R>(&raw) {
        Ok(R { ok: true, .. }) => Ok(()),
        Ok(R { reason, .. }) => Err(reason.unwrap_or_else(|| "unbound".into())),
        Err(e) => Err(e.to_string()),
    }
}

/// Temporary rendered translation of the bound node (never the document).
pub fn gizmo_set(t: [f64; 3]) -> bool {
    gizmo_set_js(t[0], t[1], t[2])
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Put back the bound node's rendered translation.
pub fn gizmo_restore() {
    let _ = gizmo_restore_js();
}

/// Forget the binding, keeping what is drawn (a committed move).
pub fn gizmo_release() {
    let _ = gizmo_release_js();
}

/// Restore and forget the binding.
pub fn gizmo_unbind() {
    let _ = gizmo_unbind_js();
}

/// The bound node's rendered translation (tests).
pub fn gizmo_rendered() -> Option<[f64; 3]> {
    let s = gizmo_rendered_js().ok()?.as_string()?;
    serde_json::from_str(&s).ok()
}

/// Fraction of viewport pixels that differ from the background after the
/// next frame (a render check for tests); -1 when unavailable.
pub async fn preview_coverage() -> f64 {
    preview_coverage_js()
        .await
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0)
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "unknown IPC error".into())
}

pub async fn call<T: DeserializeOwned>(cmd: &str, args: impl Serialize) -> Result<T, String> {
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    let a = args.serialize(&ser).map_err(|e| e.to_string())?;
    let v = tauri_invoke(cmd, a).await.map_err(js_err)?;
    serde_wasm_bindgen::from_value(v).map_err(|e| format!("{cmd}: bad reply: {e}"))
}

#[derive(Serialize)]
pub struct NoArgs {}

#[derive(Serialize)]
pub struct Session {
    pub session: u64,
}

/// VISUAL-3A1: which viewpoint the camera carry may restore onto in the new
/// scene: the layer default, or the authored viewpoint at this exact preview
/// span (mapped by Rust from the captured one).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CameraWant {
    Default,
    Authored { from: u64, to: u64 },
}

/// The identity the adapter captured when the last generation retired.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct CameraCapture {
    pub session: u64,
    pub revision: u64,
    /// `default` | `authored` | `unprovable`.
    pub kind: String,
    pub from: Option<u64>,
    pub to: Option<u64>,
}

/// The newest load's camera carry result.
#[derive(serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct CameraResult {
    pub seq: Option<u64>,
    pub session: Option<u64>,
    /// `restored` | `not-restored` | `not-requested`.
    pub status: String,
    pub reason: Option<String>,
    pub kind: Option<String>,
    pub ms: Option<f64>,
    /// Max |element| difference between the view matrix before the old world
    /// shut down and right after the restore.
    pub delta: Option<f64>,
}

/// Tests: the bound viewpoint's offsets and view matrix.
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CameraState {
    pub position_offset: [f64; 3],
    pub orientation_offset: [f64; 4],
    pub center_of_rotation_offset: [f64; 3],
    pub field_of_view_scale: f64,
    pub is_default: bool,
}

/// Tests: the per-frame view-matrix trace.
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CameraTrace {
    pub frames: u64,
    pub missing: u64,
    pub max_delta: f64,
    pub worst_frame: i64,
    pub had_ref: bool,
}

pub fn camera_capture() -> Option<CameraCapture> {
    let s = camera_capture_js().ok()?.as_string()?;
    serde_json::from_str(&s).ok()
}
pub fn camera_result() -> Option<CameraResult> {
    let s = camera_result_js().ok()?.as_string()?;
    serde_json::from_str(&s).ok()
}
pub fn camera_state() -> Option<CameraState> {
    let s = camera_state_js().ok()?.as_string()?;
    serde_json::from_str(&s).ok()
}
pub fn camera_trace_start() -> bool {
    camera_trace_start_js()
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
pub fn camera_trace_stop() -> Option<CameraTrace> {
    let s = camera_trace_stop_js().ok()?.as_string()?;
    serde_json::from_str(&s).ok()
}

/// Load `text` into X_ITE. `meta` names the preview generation (VISUAL-2
/// picking); `None` for a scene that is not a document. `camera`: what the
/// camera carry may restore onto (VISUAL-3A1). `base`: the Rust-issued
/// resource base for relative URLs (TEXTURE-LOCAL-1); "" = none.
pub async fn preview_load(
    text: &str,
    meta: Option<&crate::pick::Gen>,
    camera: Option<CameraWant>,
    base: &str,
) -> String {
    #[derive(Serialize)]
    struct Meta<'a> {
        #[serde(flatten)]
        gen: &'a crate::pick::Gen,
        camera: Option<CameraWant>,
    }
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    let meta = meta
        .and_then(|gen| Meta { gen, camera }.serialize(&ser).ok())
        .unwrap_or(JsValue::NULL);
    match preview_load_js(text, meta, base).await {
        Ok(v) => v
            .as_string()
            .unwrap_or_else(|| "error: non-string status".into()),
        Err(e) => format!("error: {}", js_err(e)),
    }
}

/// The adapter's plain-data snapshot of one click, as JSON.
pub fn preview_pick(client_x: f64, client_y: f64) -> Result<String, String> {
    preview_pick_js(client_x, client_y)
        .map_err(js_err)?
        .as_string()
        .ok_or_else(|| "non-string pick snapshot".into())
}

/// The viewport's RGB at a client point after the next frame (tests).
pub async fn preview_pixel(client_x: f64, client_y: f64) -> Option<Vec<u8>> {
    let v = preview_pixel_js(client_x, client_y).await.ok()?;
    serde_wasm_bindgen::from_value(v).ok()
}

/// The newest X_ITE load's phase, as JSON (TEXTURE-LOCAL-1 tests).
pub fn preview_load_state() -> String {
    preview_load_state_js()
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// The drawn frame as PNG bytes (TEXTURE-LOCAL-1 tests).
pub async fn preview_capture() -> Option<Vec<u8>> {
    let v = preview_capture_js().await.ok()?;
    v.dyn_into::<js_sys::Uint8Array>().ok().map(|a| a.to_vec())
}

/// Retire the generation on screen: it can no longer be picked.
pub fn preview_retire(reason: &str) {
    let _ = preview_retire_js(reason);
}

/// `{"ok":bool,"reason":..}`: whether viewport picking is available.
pub fn preview_pick_status() -> String {
    preview_pick_status_js()
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

pub fn preview_probe() -> String {
    match preview_probe_js() {
        Ok(v) => v.as_string().unwrap_or_default(),
        Err(e) => format!("unavailable: {}", js_err(e)),
    }
}

pub async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |res, _| {
        let _ = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&res, ms);
    });
    let _ = JsFuture::from(p).await;
}
