// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed IPC to the Rust backend through Tauri's injected `invoke`, and the
//! narrow X_ITE preview adapter. These two `extern` blocks are the UI's ONLY
//! contact with JavaScript.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

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
    async fn preview_load_js(text: &str, meta: JsValue) -> Result<JsValue, JsValue>;
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

/// Load `text` into X_ITE. `meta` names the preview generation (VISUAL-2
/// picking); `None` for a scene that is not a document.
pub async fn preview_load(text: &str, meta: Option<&crate::pick::Gen>) -> String {
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    let meta = meta
        .and_then(|m| m.serialize(&ser).ok())
        .unwrap_or(JsValue::NULL);
    match preview_load_js(text, meta).await {
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
