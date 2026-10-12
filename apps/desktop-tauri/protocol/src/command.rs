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

use crate::gizmo::Tool;
use crate::layout::{PanelId, PresetId};
use crate::Primitive;

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
    // UI-DOCK-1: panels and layouts (View menu).
    ViewShowSceneTree,
    ViewShowInspector,
    ViewShowDiagnostics,
    ViewHideSceneTree,
    ViewHideInspector,
    ViewHideDiagnostics,
    ViewFocusViewport,
    ViewFocusSource,
    ViewPresetModeling,
    ViewPresetSource,
    ViewResetLayout,
    // UI-MENU-1.
    AppQuit,
    EditCut,
    EditCopy,
    EditPaste,
    EditSelectAll,
    EditFind,
    EditReplace,
    EditGoToLine,
    EditDeleteNode,
    EditDuplicateNode,
    ViewThemeTokyoNight,
    ViewThemeTokyoNightStorm,
    ViewThemeTokyoNightLight,
    ViewLivePreview,
    ViewUpdatePreview,
    ViewZoomIn,
    ViewZoomOut,
    ViewZoomReset,
    CreateBox,
    CreateSphere,
    CreateCylinder,
    CreateCone,
    CreateExtrusion,
    ToolSelect,
    ToolMove,
    ToolRotate,
    ToolScale,
    ToolExtrusionOutline,
    WorldMallValidate,
    WorldMallRepack,
    WorldProjectOpen,
    WorldProjectScan,
    WorldProjectPreview,
    WorldProjectPackage,
    HelpShortcuts,
    HelpLicenses,
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
    /// UI-DOCK-1: the panel is shown (a Hide command).
    PanelShown(PanelId),
    /// UI-MENU-1: the Source editor had the focus (menu Paste).
    SourceFocus,
    /// UI-MENU-1: the Source editor had the focus and a non-empty
    /// selection (menu Cut).
    SourceSelection,
    /// UI-MENU-1: a text control (Source or a text field) had the focus and
    /// a non-empty selection (menu Copy).
    TextSelection,
    /// UI-MENU-1: a text control had the focus (menu Select All).
    TextFocus,
    /// UI-MENU-1: an open document shown in the X_ITE viewport (the viewport
    /// tools; the native test viewport has none).
    ViewportTool,
    /// UI-MENU-1: the Inspector shows an editable Extrusion `crossSection`.
    ExtrusionOutline,
}

/// Where the work is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Rust IPC commands the action may call. Only these.
    Backend(&'static [&'static str]),
    /// The UI alone (no IPC). Never changes the document.
    UiOnly,
    /// UI-MENU-1: a text change made in the Source editor widget. It reaches
    /// Rust ONLY through the editor's own edit path (`editor::on_input` ->
    /// `doc_edit`, revision-checked and hash-verified), exactly like typing.
    /// Valid only with a Source requirement (a test checks it).
    SourceEdit,
    /// UI-MENU-1: shown in a menu, never runs. The text says why.
    Planned(&'static str),
}

/// The checked state a menu shows for a command. Read from the existing UI
/// state; no menu keeps its own copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    None,
    /// The viewport Select mode.
    Pick,
    /// The active manipulation tool.
    Gizmo(Tool),
    /// The active theme (a `theme::THEMES` id).
    Theme(&'static str),
    /// Live Preview is on.
    LivePreview,
}

/// What had the keyboard focus when a menu opened (or now, for a shortcut).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FocusKind {
    #[default]
    Other,
    /// The Source editor textarea.
    Source,
    /// Another text field (Inspector, extrusion table).
    TextField,
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
    /// UI-MENU-1: the key the PLATFORM handles (Ctrl+X, C, V, A in a text
    /// control). Label only: the window handler never dispatches it.
    pub native_keys: &'static [Shortcut],
    pub check: Check,
}

/// The current state the registry reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandContext {
    pub has_document: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// UI-DOCK-1: the panels in the current layout.
    pub shown: PanelSet,
    /// UI-MENU-1: the focused control (when the menu opened).
    pub focus: FocusKind,
    /// UI-MENU-1: that control has a non-empty text selection.
    pub text_selected: bool,
    /// UI-MENU-1: the native test viewport is shown instead of X_ITE.
    pub native_viewport: bool,
    /// UI-MENU-1: the Inspector offers the Extrusion outline editor.
    pub outline_ready: bool,
}

/// A set of [`PanelId`]s (UI-DOCK-1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelSet(u8);

impl PanelSet {
    pub fn of(panels: &[PanelId]) -> PanelSet {
        PanelSet(panels.iter().fold(0, |m, p| m | Self::bit(*p)))
    }

    fn bit(p: PanelId) -> u8 {
        1 << PanelId::ALL.iter().position(|x| *x == p).unwrap_or(0)
    }

    pub fn contains(self, p: PanelId) -> bool {
        self.0 & Self::bit(p) != 0
    }
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
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
        native_keys: &[],
        check: Check::None,
    },
    // UI-DOCK-1. A layout change persists through `layout_set` (settings
    // only) and never touches the document, its history or the preview.
    layout_cmd(
        CommandId::ViewShowSceneTree,
        "view.panel.sceneTree",
        "Show Scene Tree",
        Requires::Nothing,
        "Show the Scene Tree panel (it returns to its place in the layout)",
    ),
    layout_cmd(
        CommandId::ViewShowInspector,
        "view.panel.inspector",
        "Show Inspector",
        Requires::Nothing,
        "Show the Inspector panel (it returns to its place in the layout)",
    ),
    layout_cmd(
        CommandId::ViewShowDiagnostics,
        "view.panel.diagnostics",
        "Show Diagnostics",
        Requires::Nothing,
        "Show the Diagnostics panel (it returns to its place in the layout)",
    ),
    layout_cmd(
        CommandId::ViewHideSceneTree,
        "view.hidePanel.sceneTree",
        "Hide Scene Tree",
        Requires::PanelShown(PanelId::SceneTree),
        "Hide the Scene Tree panel (the document is not changed)",
    ),
    layout_cmd(
        CommandId::ViewHideInspector,
        "view.hidePanel.inspector",
        "Hide Inspector",
        Requires::PanelShown(PanelId::Inspector),
        "Hide the Inspector panel (an unapplied field value is kept, never applied)",
    ),
    layout_cmd(
        CommandId::ViewHideDiagnostics,
        "view.hidePanel.diagnostics",
        "Hide Diagnostics",
        Requires::PanelShown(PanelId::Diagnostics),
        "Hide the Diagnostics panel (the document is not changed)",
    ),
    layout_cmd(
        CommandId::ViewFocusViewport,
        "view.focus.viewport",
        "Focus 3D Viewport",
        Requires::Nothing,
        "Bring the 3D Viewport tab to the front and focus it",
    ),
    layout_cmd(
        CommandId::ViewFocusSource,
        "view.focus.source",
        "Focus Source",
        Requires::Nothing,
        "Bring the Source tab to the front and focus the editor",
    ),
    layout_cmd(
        CommandId::ViewPresetModeling,
        "view.preset.modeling",
        "Modeling Layout",
        Requires::Nothing,
        "Large viewport; Scene Tree left; Inspector and Diagnostics right; Source as a tab",
    ),
    layout_cmd(
        CommandId::ViewPresetSource,
        "view.preset.source",
        "Source Layout",
        Requires::Nothing,
        "Large source editor; Diagnostics below; Viewport and Inspector right",
    ),
    layout_cmd(
        CommandId::ViewResetLayout,
        "view.resetLayout",
        "Reset Layout",
        Requires::Nothing,
        "Restore the active layout preset (the document is not changed)",
    ),
    // UI-MENU-1. Quit closes the window: the close goes through the same
    // CloseGate as the title-bar button (unsaved work is asked about).
    CommandSpec {
        id: CommandId::AppQuit,
        key: "app.quit",
        label: "Quit",
        menu: Menu::File,
        shortcuts: &[Shortcut { key: "q", shift: S }],
        requires: Requires::Nothing,
        action: Action::Backend(&["app_quit"]),
        mutates_document: false,
        hint: "Quit WRL Forge (unsaved changes are asked about first)",
        native_keys: &[],
        check: Check::None,
    },
    CommandSpec {
        id: CommandId::EditCut,
        key: "edit.cut",
        label: "Cut",
        menu: Menu::Edit,
        shortcuts: &[],
        requires: Requires::SourceSelection,
        action: Action::SourceEdit,
        mutates_document: true,
        hint: "Cut the Source selection (the edit goes through the Rust document like typing)",
        native_keys: &[Shortcut { key: "x", shift: S }],
        check: Check::None,
    },
    CommandSpec {
        id: CommandId::EditCopy,
        key: "edit.copy",
        label: "Copy",
        menu: Menu::Edit,
        shortcuts: &[],
        requires: Requires::TextSelection,
        action: Action::UiOnly,
        mutates_document: false,
        hint: "Copy the selected text",
        native_keys: &[Shortcut { key: "c", shift: S }],
        check: Check::None,
    },
    CommandSpec {
        id: CommandId::EditPaste,
        key: "edit.paste",
        label: "Paste",
        menu: Menu::Edit,
        shortcuts: &[],
        requires: Requires::SourceFocus,
        action: Action::SourceEdit,
        mutates_document: true,
        hint: "Paste into the Source editor (the edit goes through the Rust document like typing)",
        native_keys: &[Shortcut { key: "v", shift: S }],
        check: Check::None,
    },
    CommandSpec {
        id: CommandId::EditSelectAll,
        key: "edit.selectAll",
        label: "Select All",
        menu: Menu::Edit,
        shortcuts: &[],
        requires: Requires::TextFocus,
        action: Action::UiOnly,
        mutates_document: false,
        hint: "Select all text in the focused text control",
        native_keys: &[Shortcut { key: "a", shift: S }],
        check: Check::None,
    },
    planned(CommandId::EditFind, "edit.find", "Find…", Menu::Edit, FUTURE_LANE),
    planned(CommandId::EditReplace, "edit.replace", "Replace…", Menu::Edit, FUTURE_LANE),
    planned(CommandId::EditGoToLine, "edit.goToLine", "Go to Line…", Menu::Edit, FUTURE_LANE),
    planned(CommandId::EditDeleteNode, "edit.deleteNode", "Delete Node", Menu::Edit, FUTURE_LANE),
    planned(CommandId::EditDuplicateNode, "edit.duplicateNode", "Duplicate Node", Menu::Edit, FUTURE_LANE),
    theme_cmd(CommandId::ViewThemeTokyoNight, "view.theme.tokyoNight", "Tokyo Night", "tokyo-night"),
    theme_cmd(CommandId::ViewThemeTokyoNightStorm, "view.theme.tokyoNightStorm", "Tokyo Night Storm", "tokyo-night-storm"),
    theme_cmd(CommandId::ViewThemeTokyoNightLight, "view.theme.tokyoNightLight", "Tokyo Night Light", "tokyo-night-light"),
    CommandSpec {
        id: CommandId::ViewLivePreview,
        key: "view.livePreview",
        label: "Live Preview",
        menu: Menu::View,
        shortcuts: &[],
        requires: Requires::Nothing,
        action: Action::UiOnly,
        mutates_document: false,
        hint: "Update the 3D preview automatically while you edit",
        native_keys: &[],
        check: Check::LivePreview,
    },
    CommandSpec {
        id: CommandId::ViewUpdatePreview,
        key: "view.updatePreview",
        label: "Update Preview",
        menu: Menu::View,
        shortcuts: &[],
        requires: Requires::Document,
        action: Action::Backend(&["doc_preview_source"]),
        mutates_document: false,
        hint: "Show the current buffer in the 3D preview now",
        native_keys: &[],
        check: Check::None,
    },
    planned(CommandId::ViewZoomIn, "view.zoomIn", "Zoom In", Menu::View, FUTURE_LANE),
    planned(CommandId::ViewZoomOut, "view.zoomOut", "Zoom Out", Menu::View, FUTURE_LANE),
    planned(CommandId::ViewZoomReset, "view.zoomReset", "Actual Size", Menu::View, FUTURE_LANE),
    create_cmd(CommandId::CreateBox, "create.box", "Box", "Add a Box (2 × 2 × 2) inside a new Transform at the origin"),
    create_cmd(CommandId::CreateSphere, "create.sphere", "Sphere", "Add a Sphere (radius 1) inside a new Transform at the origin"),
    create_cmd(CommandId::CreateCylinder, "create.cylinder", "Cylinder", "Add a Cylinder (radius 1, height 2) inside a new Transform at the origin"),
    create_cmd(CommandId::CreateCone, "create.cone", "Cone", "Add a Cone (bottom radius 1, height 2) inside a new Transform at the origin"),
    create_cmd(CommandId::CreateExtrusion, "create.extrusion", "Extrusion", "Add an Extrusion (a closed hexagon outline swept along a 2-point spine, height 2) inside a new Transform at the origin"),
    tool_cmd(CommandId::ToolSelect, "tool.select", "Select", Check::Pick,
        "Select: click an object in the viewport to select its exact source node (drag still moves the camera)"),
    tool_cmd(CommandId::ToolMove, "tool.move", "Move", Check::Gizmo(Tool::Move),
        "Move: drag the X (red), Y (green) or Z (blue) handle of the selected object. Esc cancels a drag; the Inspector takes exact values."),
    tool_cmd(CommandId::ToolRotate, "tool.rotate", "Rotate", Check::Gizmo(Tool::Rotate),
        "Rotate: drag the X (red), Y (green) or Z (blue) ring of the selected object (world axes, about its center). Shift snaps to 15°. Esc cancels."),
    tool_cmd(CommandId::ToolScale, "tool.scale", "Scale", Check::Gizmo(Tool::Scale),
        "Scale: drag the X, Y or Z handle (the object's own scale axes) or the center square (uniform). Esc cancels."),
    CommandSpec {
        id: CommandId::ToolExtrusionOutline,
        key: "tool.extrusionOutline",
        label: "Extrusion Outline Editor…",
        menu: Menu::Tools,
        shortcuts: &[],
        requires: Requires::ExtrusionOutline,
        action: Action::UiOnly,
        mutates_document: false,
        hint: "Open the 2D outline editor of the selected Extrusion (Apply in the editor makes the one undo step)",
        native_keys: &[],
        check: Check::None,
    },
    planned(CommandId::WorldMallValidate, "world.mall.validate", "Validate Mall Item", Menu::World, NOT_MIGRATED),
    planned(CommandId::WorldMallRepack, "world.mall.repack", "Repack Mall Item", Menu::World, NOT_MIGRATED),
    planned(CommandId::WorldProjectOpen, "world.project.open", "Open Project…", Menu::World, NOT_MIGRATED),
    planned(CommandId::WorldProjectScan, "world.project.scan", "Scan Assets", Menu::World, NOT_MIGRATED),
    planned(CommandId::WorldProjectPreview, "world.project.preview", "Preview Project", Menu::World, NOT_MIGRATED),
    planned(CommandId::WorldProjectPackage, "world.project.package", "Package Project…", Menu::World, NOT_MIGRATED),
    planned(CommandId::HelpShortcuts, "help.shortcuts", "Keyboard Shortcuts", Menu::Help, FUTURE_LANE),
    planned(CommandId::HelpLicenses, "help.licenses", "Licenses", Menu::Help, FUTURE_LANE),
];

/// UI-MENU-1: the reason on every World menu item.
pub const NOT_MIGRATED: &str = "Not migrated to the Rust application.";
/// UI-MENU-1: the reason on a reserved Edit / View / Help item.
pub const FUTURE_LANE: &str = "Planned for a future feature lane.";

const fn planned(
    id: CommandId,
    key: &'static str,
    label: &'static str,
    menu: Menu,
    why: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        key,
        label,
        menu,
        shortcuts: &[],
        requires: Requires::Nothing,
        action: Action::Planned(why),
        mutates_document: false,
        hint: why,
        native_keys: &[],
        check: Check::None,
    }
}

/// A theme choice: an application preference (`theme_set`), never the
/// document.
const fn theme_cmd(
    id: CommandId,
    key: &'static str,
    label: &'static str,
    theme: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        key,
        label,
        menu: Menu::View,
        shortcuts: &[],
        requires: Requires::Nothing,
        action: Action::Backend(&["theme_set"]),
        mutates_document: false,
        hint: "Application color theme (saved for next launch)",
        native_keys: &[],
        check: Check::Theme(theme),
    }
}

/// Create: Rust generates and inserts the source (`doc_create`).
const fn create_cmd(
    id: CommandId,
    key: &'static str,
    label: &'static str,
    hint: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        key,
        label,
        menu: Menu::Create,
        shortcuts: &[],
        requires: Requires::Document,
        action: Action::Backend(&["doc_create"]),
        mutates_document: true,
        hint,
        native_keys: &[],
        check: Check::None,
    }
}

/// A viewport tool toggle. Selecting a tool changes no source; a gizmo drag
/// writes through its own proven transform commands, not through this one.
const fn tool_cmd(
    id: CommandId,
    key: &'static str,
    label: &'static str,
    check: Check,
    hint: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        key,
        label,
        menu: Menu::Tools,
        shortcuts: &[],
        requires: Requires::ViewportTool,
        action: Action::UiOnly,
        mutates_document: false,
        hint,
        native_keys: &[],
        check,
    }
}

const fn layout_cmd(
    id: CommandId,
    key: &'static str,
    label: &'static str,
    requires: Requires,
    hint: &'static str,
) -> CommandSpec {
    CommandSpec {
        id,
        key,
        label,
        menu: Menu::View,
        shortcuts: &[],
        requires,
        action: Action::Backend(&["layout_set"]),
        mutates_document: false,
        hint,
        native_keys: &[],
        check: Check::None,
    }
}

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
    "layout_set",
    // UI-MENU-1.
    "app_quit",
    "theme_set",
    "doc_create",
    "doc_preview_source",
];

impl CommandId {
    pub const ALL: [CommandId; 56] = [
        CommandId::FileNewWorld,
        CommandId::FileOpen,
        CommandId::FileSave,
        CommandId::FileSaveAs,
        CommandId::FileClose,
        CommandId::FileReload,
        CommandId::EditUndo,
        CommandId::EditRedo,
        CommandId::HelpAbout,
        CommandId::ViewShowSceneTree,
        CommandId::ViewShowInspector,
        CommandId::ViewShowDiagnostics,
        CommandId::ViewHideSceneTree,
        CommandId::ViewHideInspector,
        CommandId::ViewHideDiagnostics,
        CommandId::ViewFocusViewport,
        CommandId::ViewFocusSource,
        CommandId::ViewPresetModeling,
        CommandId::ViewPresetSource,
        CommandId::ViewResetLayout,
        CommandId::AppQuit,
        CommandId::EditCut,
        CommandId::EditCopy,
        CommandId::EditPaste,
        CommandId::EditSelectAll,
        CommandId::EditFind,
        CommandId::EditReplace,
        CommandId::EditGoToLine,
        CommandId::EditDeleteNode,
        CommandId::EditDuplicateNode,
        CommandId::ViewThemeTokyoNight,
        CommandId::ViewThemeTokyoNightStorm,
        CommandId::ViewThemeTokyoNightLight,
        CommandId::ViewLivePreview,
        CommandId::ViewUpdatePreview,
        CommandId::ViewZoomIn,
        CommandId::ViewZoomOut,
        CommandId::ViewZoomReset,
        CommandId::CreateBox,
        CommandId::CreateSphere,
        CommandId::CreateCylinder,
        CommandId::CreateCone,
        CommandId::CreateExtrusion,
        CommandId::ToolSelect,
        CommandId::ToolMove,
        CommandId::ToolRotate,
        CommandId::ToolScale,
        CommandId::ToolExtrusionOutline,
        CommandId::WorldMallValidate,
        CommandId::WorldMallRepack,
        CommandId::WorldProjectOpen,
        CommandId::WorldProjectScan,
        CommandId::WorldProjectPreview,
        CommandId::WorldProjectPackage,
        CommandId::HelpShortcuts,
        CommandId::HelpLicenses,
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
        let s = self.spec();
        if let Action::Planned(why) = s.action {
            return Availability::Disabled(why);
        }
        match s.requires {
            Requires::UndoHistory if !ctx.can_undo => Availability::Disabled("Nothing to undo"),
            Requires::RedoHistory if !ctx.can_redo => Availability::Disabled("Nothing to redo"),
            r => match unmet(r, ctx) {
                Some(why) => Availability::Disabled(why),
                None => Availability::Enabled,
            },
        }
    }

    /// The check made when the command runs. History is decided by Rust
    /// (`doc_undo` / `doc_redo`); every other requirement refuses here.
    pub fn refusal(self, ctx: CommandContext) -> Option<&'static str> {
        let s = self.spec();
        if let Action::Planned(why) = s.action {
            return Some(why);
        }
        unmet(s.requires, ctx)
    }

    /// UI-MENU-1: the primitive a Create command inserts.
    pub fn primitive(self) -> Option<Primitive> {
        Some(match self {
            CommandId::CreateBox => Primitive::Box,
            CommandId::CreateSphere => Primitive::Sphere,
            CommandId::CreateCylinder => Primitive::Cylinder,
            CommandId::CreateCone => Primitive::Cone,
            CommandId::CreateExtrusion => Primitive::Extrusion,
            _ => return None,
        })
    }

    /// UI-MENU-1: the Create command for a primitive.
    pub fn create_for(prim: Primitive) -> CommandId {
        match prim {
            Primitive::Box => CommandId::CreateBox,
            Primitive::Sphere => CommandId::CreateSphere,
            Primitive::Cylinder => CommandId::CreateCylinder,
            Primitive::Cone => CommandId::CreateCone,
            Primitive::Extrusion => CommandId::CreateExtrusion,
        }
    }

    /// UI-MENU-1: the theme id a theme command selects.
    pub fn theme(self) -> Option<&'static str> {
        match self.spec().check {
            Check::Theme(id) => Some(id),
            _ => None,
        }
    }

    /// UI-MENU-1: the command that selects theme `id`.
    pub fn theme_for(id: &str) -> Option<CommandId> {
        COMMANDS
            .iter()
            .find(|c| matches!(c.check, Check::Theme(t) if t == id))
            .map(|c| c.id)
    }

    /// UI-DOCK-1: the panel a Show / Hide / Focus command names.
    pub fn panel(self) -> Option<PanelId> {
        use CommandId::*;
        Some(match self {
            ViewShowSceneTree | ViewHideSceneTree => PanelId::SceneTree,
            ViewShowInspector | ViewHideInspector => PanelId::Inspector,
            ViewShowDiagnostics | ViewHideDiagnostics => PanelId::Diagnostics,
            ViewFocusViewport => PanelId::Viewport,
            ViewFocusSource => PanelId::Source,
            _ => return None,
        })
    }

    /// UI-DOCK-1: the preset a layout command selects.
    pub fn preset(self) -> Option<PresetId> {
        match self {
            CommandId::ViewPresetModeling => Some(PresetId::Modeling),
            CommandId::ViewPresetSource => Some(PresetId::Source),
            _ => None,
        }
    }

    /// The Hide command for an optional panel.
    pub fn hide_for(p: PanelId) -> Option<CommandId> {
        match p {
            PanelId::SceneTree => Some(CommandId::ViewHideSceneTree),
            PanelId::Inspector => Some(CommandId::ViewHideInspector),
            PanelId::Diagnostics => Some(CommandId::ViewHideDiagnostics),
            _ => None,
        }
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
pub const PANEL_HIDDEN: &str = "The panel is not shown";
pub const NEEDS_SOURCE: &str =
    "Works in the Source editor (in other text fields use the keyboard shortcut)";
pub const NEEDS_SOURCE_SELECTION: &str = "Select text in the Source editor first";
pub const NEEDS_TEXT_SELECTION: &str = "Select text first";
pub const NEEDS_TEXT_FOCUS: &str = "Works in a text field or the Source editor";
pub const NEEDS_XITE: &str = "The viewport tools need the X_ITE viewport";
pub const NEEDS_OUTLINE: &str = "Select an Extrusion with an editable crossSection first";

/// The first requirement `ctx` does not meet, if any (history excluded:
/// that is the backend's run-time decision).
fn unmet(r: Requires, ctx: CommandContext) -> Option<&'static str> {
    let source = ctx.focus == FocusKind::Source;
    let text = source || ctx.focus == FocusKind::TextField;
    match r {
        Requires::Nothing | Requires::UndoHistory | Requires::RedoHistory => None,
        Requires::Document if !ctx.has_document => Some(NO_DOCUMENT),
        Requires::Document => None,
        Requires::PanelShown(p) if !ctx.shown.contains(p) => Some(PANEL_HIDDEN),
        Requires::PanelShown(_) => None,
        Requires::SourceFocus | Requires::SourceSelection if !ctx.has_document => Some(NO_DOCUMENT),
        Requires::SourceFocus | Requires::SourceSelection if !source => Some(NEEDS_SOURCE),
        Requires::SourceSelection if !ctx.text_selected => Some(NEEDS_SOURCE_SELECTION),
        Requires::SourceFocus | Requires::SourceSelection => None,
        Requires::TextSelection if !text => Some(NEEDS_TEXT_FOCUS),
        Requires::TextSelection if !ctx.text_selected => Some(NEEDS_TEXT_SELECTION),
        Requires::TextSelection => None,
        Requires::TextFocus if !text => Some(NEEDS_TEXT_FOCUS),
        Requires::TextFocus => None,
        Requires::ViewportTool if !ctx.has_document => Some(NO_DOCUMENT),
        Requires::ViewportTool if ctx.native_viewport => Some(NEEDS_XITE),
        Requires::ViewportTool => None,
        Requires::ExtrusionOutline if !ctx.outline_ready => Some(NEEDS_OUTLINE),
        Requires::ExtrusionOutline => None,
    }
}

/// One entry of a menu (UI-MENU-1). Display order and grouping only; every
/// label, shortcut, state and action comes from the command's spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Command(CommandId),
    /// A panel shown as one checked item: Show when hidden, Hide when shown.
    Panel(CommandId, CommandId),
    /// A layout preset shown as a radio item.
    Preset(CommandId),
    Separator,
    /// A non-interactive group heading.
    Group(&'static str),
}

/// A top-level menu: its label, its Alt mnemonic and its entries.
#[derive(Debug, Clone, Copy)]
pub struct MenuDef {
    pub menu: Menu,
    pub label: &'static str,
    /// Lowercase mnemonic letter (Alt+letter on Linux and Windows).
    pub mnemonic: &'static str,
    pub entries: &'static [MenuEntry],
}

/// Commands that live in the macOS application menu (WRL Forge → About /
/// Quit). The in-window menus leave them out on macOS, so no command is
/// active in two menus.
pub const MAC_APP_MENU: &[CommandId] = &[CommandId::HelpAbout, CommandId::AppQuit];

/// The event a native application-menu item sends to the UI. Its payload is
/// the command key.
pub const MENU_EVENT: &str = "menu-command";

/// The command a native application-menu event may run: a registry key of
/// [`MAC_APP_MENU`], nothing else.
pub fn from_native_menu(key: &str) -> Option<CommandId> {
    CommandId::from_key(key).filter(|c| MAC_APP_MENU.contains(c))
}

/// UI-MENU-1: the seven menus, in order.
pub const MENUS: &[MenuDef] = {
    use CommandId::*;
    use MenuEntry::{Command as C, Group as G, Panel as P, Preset as R, Separator as Sep};
    &[
        MenuDef {
            menu: Menu::File,
            label: "File",
            mnemonic: "f",
            entries: &[
                C(FileNewWorld),
                C(FileOpen),
                Sep,
                C(FileSave),
                C(FileSaveAs),
                C(FileReload),
                Sep,
                C(FileClose),
                C(AppQuit),
            ],
        },
        MenuDef {
            menu: Menu::Edit,
            label: "Edit",
            mnemonic: "e",
            entries: &[
                C(EditUndo),
                C(EditRedo),
                Sep,
                C(EditCut),
                C(EditCopy),
                C(EditPaste),
                C(EditSelectAll),
                Sep,
                C(EditFind),
                C(EditReplace),
                C(EditGoToLine),
                Sep,
                C(EditDeleteNode),
                C(EditDuplicateNode),
            ],
        },
        MenuDef {
            menu: Menu::View,
            label: "View",
            mnemonic: "v",
            entries: &[
                G("Panels"),
                P(ViewShowSceneTree, ViewHideSceneTree),
                P(ViewShowInspector, ViewHideInspector),
                P(ViewShowDiagnostics, ViewHideDiagnostics),
                C(ViewFocusViewport),
                C(ViewFocusSource),
                G("Layout"),
                R(ViewPresetModeling),
                R(ViewPresetSource),
                C(ViewResetLayout),
                G("Theme"),
                C(ViewThemeTokyoNight),
                C(ViewThemeTokyoNightStorm),
                C(ViewThemeTokyoNightLight),
                G("Preview"),
                C(ViewLivePreview),
                C(ViewUpdatePreview),
                G("Zoom"),
                C(ViewZoomIn),
                C(ViewZoomOut),
                C(ViewZoomReset),
            ],
        },
        MenuDef {
            menu: Menu::Create,
            label: "Create",
            mnemonic: "c",
            entries: &[
                C(CreateBox),
                C(CreateSphere),
                C(CreateCylinder),
                C(CreateCone),
                C(CreateExtrusion),
            ],
        },
        MenuDef {
            menu: Menu::Tools,
            label: "Tools",
            mnemonic: "t",
            entries: &[
                C(ToolSelect),
                C(ToolMove),
                C(ToolRotate),
                C(ToolScale),
                Sep,
                C(ToolExtrusionOutline),
            ],
        },
        MenuDef {
            menu: Menu::World,
            label: "World",
            mnemonic: "w",
            entries: &[
                G("Mall Item"),
                C(WorldMallValidate),
                C(WorldMallRepack),
                G("World Project"),
                C(WorldProjectOpen),
                C(WorldProjectScan),
                C(WorldProjectPreview),
                C(WorldProjectPackage),
            ],
        },
        MenuDef {
            menu: Menu::Help,
            label: "Help",
            mnemonic: "h",
            entries: &[C(HelpShortcuts), C(HelpLicenses), Sep, C(HelpAbout)],
        },
    ]
};

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
