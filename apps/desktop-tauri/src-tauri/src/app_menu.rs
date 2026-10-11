// SPDX-License-Identifier: GPL-3.0-or-later
//! UI-MENU-1: the macOS application menu.
//!
//! The menus are drawn in the window on every platform (`ui/src/menu.rs`).
//! macOS also needs its standard application menu: WRL Forge → About, Hide,
//! Hide Others, Show All, Quit, and an Edit menu with the platform's own Cut
//! / Copy / Paste / Select All so that ⌘X / ⌘C / ⌘V / ⌘A reach the WebView.
//!
//! About and Quit are registry commands: the item sends the command key to
//! the UI (`menu-command` event) and the UI runs it through `command::run`,
//! the one dispatch path. Quit therefore ends in `app_quit` -> the window's
//! CloseGate, like every other Quit.
//!
//! [`build`] compiles on every platform (so Linux builds type-check it); it
//! is installed only on macOS. Its runtime behavior is NOT verified until it
//! runs on a Mac.

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};
use wrlforge_desktop_protocol::command::{CommandId, MAC_APP_MENU};

/// The event the UI listens to (`ui/src/menu.rs`).
pub const EVENT: &str = "menu-command";

/// The application menu (installed on macOS only, see `lib.rs`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let item = |id: CommandId, accel: Option<&str>| {
        MenuItem::with_id(app, id.key(), id.spec().label, true, accel)
    };
    let about = item(CommandId::HelpAbout, None)?;
    let quit = item(CommandId::AppQuit, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(
        app,
        "WRL Forge",
        true,
        &[
            &about,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit])
}

/// A menu item was chosen: send its registry key to the UI.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn on_event<R: Runtime>(app: &AppHandle<R>, ev: MenuEvent) {
    let key = ev.id().as_ref();
    if MAC_APP_MENU.iter().any(|c| c.key() == key) {
        let _ = app.emit(EVENT, key.to_string());
    }
}
