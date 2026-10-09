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
    async fn preview_load_js(text: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "wrlforgePreview"], js_name = probe, catch)]
    fn preview_probe_js() -> Result<JsValue, JsValue>;
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

pub async fn preview_load(text: &str) -> String {
    match preview_load_js(text).await {
        Ok(v) => v
            .as_string()
            .unwrap_or_else(|| "error: non-string status".into()),
        Err(e) => format!("error: {}", js_err(e)),
    }
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
