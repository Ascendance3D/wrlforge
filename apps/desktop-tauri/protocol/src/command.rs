// SPDX-License-Identifier: GPL-3.0-or-later
//! APP-SHELL-0: the application identity and the typed command registry.
//!
//! One table describes every command the shell can run: its stable ID, its
//! label, its shortcut, the state it needs and the Rust IPC commands it may
//! call. The toolbar, the keyboard handler and (later) the menus read this
//! table; none of them keeps a private copy. Data only: the UI owns the
//! dispatch (`ui/src/command.rs`), the backend owns every document change.
//!
//! See `docs/architecture/APP_SHELL_0.md`.

use serde::{Deserialize, Serialize};

/// The product name. Not a version: see [`app_display_name`].
pub const APP_NAME: &str = "WRL Forge";

/// The Rust/Tauri application version (the `apps/desktop-tauri` workspace
/// version). `tauri.conf.json` must carry the same value; a test checks it.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release channel. Every 1.4.x Rust build is a Beta; `1.5.0` is reserved for
/// the complete Rust release on Linux, Windows and macOS.
pub const APP_CHANNEL: &str = "Beta";

/// `WRL Forge 1.4.5 (Beta)`: the one identity string for the toolbar, the
/// window title, About and `--version`.
pub fn app_display_name() -> String {
    format!("{APP_NAME} {APP_VERSION} ({APP_CHANNEL})")
}

/// What the backend reports about itself (`app_info`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub channel: String,
    pub display: String,
    /// The version Tauri read from `tauri.conf.json` (package metadata).
    pub config_version: String,
    pub identifier: String,
    pub tauri_version: String,
    /// The default 3D renderer, e.g. `X_ITE 16.5.0`.
    pub renderer: String,
    pub license: String,
}

/// Top-level menus (APP-SHELL-0 Task D). Every command belongs to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Menu {
    File,
    Edit,
    View,
    Create,
    Tools,
    World,
    Help,
}

/// The stable command IDs that the shared command path runs today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandId {
    FileNewWorld,
    FileOpen,
    FileSave,
    FileSaveAs,
    FileClose,
    FileReload,
    EditUndo,
    EditRedo,
    HelpAbout,
}

/// How Shift takes part in a shortcut match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftMatch {
    No,
    Yes,
    /// Shift is ignored (Ctrl+O and Ctrl+Shift+O both open).
    Any,
}

/// A shortcut on the primary modifier: Ctrl on Linux and Windows, Command
/// on macOS. The WebView handler accepts either modifier on every platform
/// (the pre-registry behavior); only the label is platform specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// Lowercase `KeyboardEvent.key`.
    pub key: &'static str,
    pub shift: ShiftMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
    MacOs,
}

impl Shortcut {
    pub fn label(&self, platform: Platform) -> String {
        let k = self.key.to_uppercase();
        let shift = self.shift == ShiftMatch::Yes;
        match platform {
            Platform::MacOs => format!("{}⌘{k}", if shift { "⇧" } else { "" }),
            _ => format!("Ctrl+{}{k}", if shift { "Shift+" } else { "" }),
        }
    }

    fn matches(&self, key: &str, shift: bool) -> bool {
        self.key == key
            && match self.shift {
                ShiftMatch::No => !shift,
                ShiftMatch::Yes => shift,
                ShiftMatch::Any => true,
            }
    }
}

/// The state a command needs before it can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requires {
    Nothing,
    /// An open document. Checked again when the command runs.
    Document,
    /// Undo history. Presentation only: the backend decides at run time
    /// (the UI may not have seen the acknowledgement of a just-typed edit).
    UndoHistory,
    /// Redo history. Presentation only, as [`Requires::UndoHistory`].
    RedoHistory,
}

/// Where the work is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Rust IPC commands the action may call. Only these.
    Backend(&'static [&'static str]),
    /// The UI alone (no IPC).
    UiOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: CommandId,
    /// Stable string ID (`file.save`). Persisted keymaps and menus use it.
    pub key: &'static str,
    pub label: &'static str,
    pub menu: Menu,
    pub shortcuts: &'static [Shortcut],
    pub requires: Requires,
    pub action: Action,
    /// True when the action can change document text or history.
    pub mutates_document: bool,
    /// Tooltip text without the shortcut.
    pub hint: &'static str,
}

/// The current state the registry reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandContext {
    pub has_document: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Why a command is not available now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Enabled,
    Disabled(&'static str),
}

const S: ShiftMatch = ShiftMatch::No;

/// The registry. Order is display order inside a menu.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: CommandId::FileNewWorld,
        key: "file.newWorld",
        label: "New World",
        menu: Menu::File,
        shortcuts: &[Shortcut {
            key: "n",
            shift: ShiftMatch::Any,
        }],
        requires: Requires::Nothing,
        action: Action::Backend(&["new_document", "close_document"]),
        mutates_document: true,
        hint: "Start a new, empty VRML97 world",
    },
    CommandSpec {
        id: CommandId::FileOpen,
        key: "file.open",
        label: "Open…",
        menu: Menu::File,
        shortcuts: &[Shortcut {
            key: "o",
            shift: ShiftMatch::Any,
        }],
        requires: Requires::Nothing,
        action: Action::Backend(&["open_document", "close_document"]),
        mutates_document: true,
        hint: "Open a .wrl / .wrz file",
    },
    CommandSpec {
        id: CommandId::FileSave,
        key: "file.save",
        label: "Save",
        menu: Menu::File,
        shortcuts: &[Shortcut { key: "s", shift: S }],
        requires: Requires::Document,
        action: Action::Backend(&["doc_save", "doc_save_as"]),
        mutates_document: true,
        hint: "Save the document (a new world asks for a file name)",
    },
    CommandSpec {
        id: CommandId::FileSaveAs,
        key: "file.saveAs",
        label: "Save As…",
        menu: Menu::File,
        shortcuts: &[Shortcut {
            key: "s",
            shift: ShiftMatch::Yes,
        }],
        requires: Requires::Document,
        action: Action::Backend(&["doc_save_as"]),
        mutates_document: true,
        hint: "Save the document to a new file",
    },
    CommandSpec {
        id: CommandId::FileClose,
        key: "file.close",
        label: "Close",
        menu: Menu::File,
        shortcuts: &[],
        requires: Requires::Document,
        action: Action::Backend(&["close_document"]),
        mutates_document: true,
        hint: "Close the document (refused while it has unsaved changes)",
    },
    CommandSpec {
        id: CommandId::FileReload,
        key: "file.reload",
        label: "Reload from Disk",
        menu: Menu::File,
        shortcuts: &[],
        requires: Requires::Document,
        action: Action::Backend(&["doc_reload"]),
        mutates_document: true,
        hint: "Reload the file from disk and discard your edits",
    },
    CommandSpec {
        id: CommandId::EditUndo,
        key: "edit.undo",
        label: "Undo",
        menu: Menu::Edit,
        shortcuts: &[Shortcut { key: "z", shift: S }],
        requires: Requires::UndoHistory,
        action: Action::Backend(&["doc_undo"]),
        mutates_document: true,
        hint: "Undo the last change",
    },
    CommandSpec {
        id: CommandId::EditRedo,
        key: "edit.redo",
        label: "Redo",
        menu: Menu::Edit,
        shortcuts: &[
            Shortcut {
                key: "y",
                shift: ShiftMatch::Any,
            },
            Shortcut {
                key: "z",
                shift: ShiftMatch::Yes,
            },
        ],
        requires: Requires::RedoHistory,
        action: Action::Backend(&["doc_redo"]),
        mutates_document: true,
        hint: "Redo the last undone change",
    },
    CommandSpec {
        id: CommandId::HelpAbout,
        key: "help.about",
        label: "About WRL Forge",
        menu: Menu::Help,
        shortcuts: &[],
        requires: Requires::Nothing,
        action: Action::Backend(&["app_info"]),
        mutates_document: false,
        hint: "Version, renderer and license information",
    },
];

/// Every Rust IPC command a registry action may call. A document-changing
/// command outside this set is a test failure, not a quiet extension.
pub const ALLOWED_BACKEND: &[&str] = &[
    "new_document",
    "open_document",
    "close_document",
    "doc_save",
    "doc_save_as",
    "doc_reload",
    "doc_undo",
    "doc_redo",
    "app_info",
];

impl CommandId {
    pub const ALL: [CommandId; 9] = [
        CommandId::FileNewWorld,
        CommandId::FileOpen,
        CommandId::FileSave,
        CommandId::FileSaveAs,
        CommandId::FileClose,
        CommandId::FileReload,
        CommandId::EditUndo,
        CommandId::EditRedo,
        CommandId::HelpAbout,
    ];

    pub fn spec(self) -> &'static CommandSpec {
        COMMANDS
            .iter()
            .find(|c| c.id == self)
            .expect("every CommandId has a registry entry")
    }

    pub fn key(self) -> &'static str {
        self.spec().key
    }

    pub fn from_key(key: &str) -> Option<CommandId> {
        COMMANDS.iter().find(|c| c.key == key).map(|c| c.id)
    }

    /// Button / menu state. The run-time check is [`CommandId::refusal`].
    pub fn availability(self, ctx: CommandContext) -> Availability {
        match self.spec().requires {
            Requires::Nothing => Availability::Enabled,
            Requires::Document if !ctx.has_document => Availability::Disabled(NO_DOCUMENT),
            Requires::Document => Availability::Enabled,
            Requires::UndoHistory if !ctx.can_undo => Availability::Disabled("Nothing to undo"),
            Requires::RedoHistory if !ctx.can_redo => Availability::Disabled("Nothing to redo"),
            Requires::UndoHistory | Requires::RedoHistory => Availability::Enabled,
        }
    }

    /// The check made when the command runs. Only the document requirement
    /// refuses here; history is decided by Rust (`doc_undo` / `doc_redo`).
    pub fn refusal(self, ctx: CommandContext) -> Option<&'static str> {
        (self.spec().requires == Requires::Document && !ctx.has_document).then_some(NO_DOCUMENT)
    }

    /// The tooltip: hint plus the first shortcut, if any.
    pub fn title(self, platform: Platform) -> String {
        let s = self.spec();
        match s.shortcuts.first() {
            Some(k) => format!("{} ({})", s.hint, k.label(platform)),
            None => s.hint.to_string(),
        }
    }
}

pub const NO_DOCUMENT: &str = "No document is open";

/// The command for a primary-modifier key press, if any. `primary` is
/// Ctrl or Command (either, on every platform). Alt is not examined, as
/// before the registry.
pub fn from_shortcut(key: &str, primary: bool, shift: bool) -> Option<CommandId> {
    if !primary {
        return None;
    }
    let key = key.to_lowercase();
    COMMANDS
        .iter()
        .find(|c| c.shortcuts.iter().any(|s| s.matches(&key, shift)))
        .map(|c| c.id)
}

#[cfg(test)]
mod tests;
