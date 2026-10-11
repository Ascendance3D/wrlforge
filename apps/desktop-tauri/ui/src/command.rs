// SPDX-License-Identifier: GPL-3.0-or-later
//! APP-SHELL-0: the one command path for the toolbar and the keyboard.
//!
//! The registry (`wrlforge_desktop_protocol::command`) says what each
//! command is; this module says how the UI runs it. Every action calls the
//! same function the toolbar called before the registry, so Rust still
//! makes every document change, check and refusal.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wrlforge_desktop_protocol::command::{self as c, Availability, CommandContext, CommandId};

use crate::ui::{self, ui};
use crate::{about, editor};

/// The current state, read reactively (button `disabled` follows it).
pub fn context() -> CommandContext {
    let u = ui();
    CommandContext {
        has_document: u.doc.with(|d| d.is_some()),
        can_undo: u.can_undo.get(),
        can_redo: u.can_redo.get(),
    }
}

fn context_untracked() -> CommandContext {
    let u = ui();
    CommandContext {
        has_document: u.doc.with_untracked(|d| d.is_some()),
        can_undo: u.can_undo.get_untracked(),
        can_redo: u.can_redo.get_untracked(),
    }
}

/// Reactive: true when the toolbar / menu item is enabled.
pub fn enabled(id: CommandId) -> bool {
    id.availability(context()) == Availability::Enabled
}

pub fn platform() -> c::Platform {
    let mac = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Mac"));
    if mac {
        c::Platform::MacOs
    } else {
        c::Platform::Linux
    }
}

/// Tooltip with the platform's shortcut label.
pub fn title(id: CommandId) -> String {
    id.title(platform())
}

/// Run a command. A command that needs a document says so instead of
/// doing nothing; history refusals come from Rust (`Nothing to undo`).
pub fn run(id: CommandId) {
    if let Some(why) = id.refusal(context_untracked()) {
        ui::flash(&format!("{} is not available: {why}.", id.spec().label));
        return;
    }
    match id {
        CommandId::FileNewWorld => ui::new_world(),
        CommandId::FileOpen => ui::open(),
        CommandId::FileSave => ui::save(false),
        CommandId::FileSaveAs => ui::save(true),
        CommandId::FileClose => ui::close(),
        CommandId::FileReload => ui::reload(),
        CommandId::EditUndo => spawn_local(editor::history(true)),
        CommandId::EditRedo => spawn_local(editor::history(false)),
        CommandId::HelpAbout => about::show(),
    }
}

/// The source editor's keydown: a registry shortcut runs its command.
pub fn on_keydown(ev: web_sys::KeyboardEvent) {
    if let Some(id) = c::from_shortcut(&ev.key(), ev.ctrl_key() || ev.meta_key(), ev.shift_key()) {
        ev.prevent_default();
        run(id);
    }
}

/// A toolbar button bound to a registry command.
#[component]
pub fn CommandButton(
    command: CommandId,
    #[prop(optional)] secondary: bool,
    #[prop(optional, into)] dom_id: Option<String>,
    /// Button text when it differs from the menu label.
    #[prop(optional, into)]
    text: Option<String>,
) -> impl IntoView {
    let text = text.unwrap_or_else(|| command.spec().label.to_string());
    view! {
        <button id=dom_id class:secondary=secondary data-command=command.key()
            title=title(command)
            disabled=move || !enabled(command)
            on:click=move |_| run(command)>{text}</button>
    }
}
