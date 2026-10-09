// SPDX-License-Identifier: GPL-3.0-or-later
// Prevents an extra console window on Windows release builds (Windows is not a
// supported target; kept inert for the standard Tauri template).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wrl_forge_desktop::run()
}
