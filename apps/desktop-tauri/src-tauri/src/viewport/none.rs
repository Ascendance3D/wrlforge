// SPDX-License-Identifier: GPL-3.0-or-later
//! NATIVE-RENDER-1 has no host on this platform: the native viewport never
//! starts and X_ITE stays the viewport.

use std::time::Duration;

use tauri::AppHandle;

pub(crate) struct Plat;

pub(crate) fn layout(_: &AppHandle) -> Result<Plat, String> {
    Err("the native viewport is not supported on this platform".into())
}
pub(crate) fn every(_: Duration, _: fn()) {}
pub(crate) fn add_viewport() {}
pub(crate) fn cancel_attach(_: &mut Plat) {}
pub(crate) fn park() {}
pub(crate) fn remove_viewport() {}
pub(crate) fn report() -> serde_json::Value {
    serde_json::Value::Null
}
