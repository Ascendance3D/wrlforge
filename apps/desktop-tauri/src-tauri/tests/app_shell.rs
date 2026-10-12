// SPDX-License-Identifier: GPL-3.0-or-later
//! APP-SHELL-0: version policy and command-registry wiring checks.

use wrlforge_desktop_protocol::command::{self as c, Action, COMMANDS};

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const LIB_RS: &str = include_str!("../src/lib.rs");
const ELECTRON_PACKAGE: &str = include_str!("../../../../package.json");

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("valid JSON")
}

#[test]
fn one_version_everywhere() {
    let conf = json(TAURI_CONF);
    assert_eq!(c::APP_VERSION, "1.4.5");
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        c::APP_VERSION,
        "src-tauri vs protocol"
    );
    assert_eq!(conf["version"], c::APP_VERSION, "tauri.conf.json");
    assert_eq!(c::app_display_name(), "WRL Forge 1.4.5 (Beta)");
}

#[test]
fn beta_until_the_complete_rust_release() {
    let mut it = c::APP_VERSION.split('.').map(|n| n.parse::<u32>().unwrap());
    let (major, minor) = (it.next().unwrap(), it.next().unwrap());
    // 1.5.0 is reserved for the complete Rust release on all three platforms.
    assert!((major, minor) < (1, 5), "1.5.0 needs its own release lane");
    assert_eq!(c::APP_CHANNEL, "Beta");
}

#[test]
fn identifier_and_electron_version_are_unchanged() {
    assert_eq!(
        json(TAURI_CONF)["identifier"],
        "org.ascendance3d.wrlforge.tauri"
    );
    assert_eq!(
        json(ELECTRON_PACKAGE)["version"],
        "1.4.0",
        "historical Electron version"
    );
}

#[test]
fn xite_16_5_0_is_the_default_renderer() {
    assert_eq!(wrl_forge_desktop::xite_version(), "16.5.0");
}

/// Every Rust command a registry action names is a registered Tauri
/// command. A registry entry cannot reach Rust any other way.
#[test]
fn registry_backend_commands_are_registered() {
    let handler = LIB_RS
        .split("generate_handler![")
        .nth(1)
        .and_then(|s| s.split("])").next())
        .expect("generate_handler list");
    for name in c::ALLOWED_BACKEND {
        assert!(
            handler.contains(&format!("commands::{name},")),
            "{name} is not a registered command"
        );
    }
    for spec in COMMANDS {
        if let Action::Backend(cmds) = spec.action {
            assert!(
                cmds.iter().all(|n| c::ALLOWED_BACKEND.contains(n)),
                "{}",
                spec.key
            );
        }
    }
}
