// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

#[test]
fn identity_is_one_string() {
    assert_eq!(
        app_display_name(),
        format!("WRL Forge {APP_VERSION} (Beta)")
    );
}

#[test]
fn every_id_has_exactly_one_entry_and_a_unique_key() {
    assert_eq!(COMMANDS.len(), CommandId::ALL.len());
    for id in CommandId::ALL {
        assert_eq!(COMMANDS.iter().filter(|c| c.id == id).count(), 1, "{id:?}");
        assert_eq!(CommandId::from_key(id.key()), Some(id));
    }
    let mut keys: Vec<_> = COMMANDS.iter().map(|c| c.key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), COMMANDS.len());
}

/// The table the pre-registry `on_keydown` hard-coded, key by key.
#[test]
fn shortcuts_match_the_pre_registry_handler() {
    use CommandId::*;
    let cases = [
        ("z", false, Some(EditUndo)),
        ("Z", false, Some(EditUndo)),
        ("z", true, Some(EditRedo)),
        ("y", false, Some(EditRedo)),
        ("y", true, Some(EditRedo)),
        ("s", false, Some(FileSave)),
        ("s", true, Some(FileSaveAs)),
        ("o", false, Some(FileOpen)),
        ("o", true, Some(FileOpen)),
        ("n", false, Some(FileNewWorld)),
        ("n", true, Some(FileNewWorld)),
        // B2: Find / Replace / Go to Line.
        ("f", false, Some(EditFind)),
        ("h", false, Some(EditReplace)),
        ("g", false, Some(EditGoToLine)),
        ("f", true, None),
        ("h", true, None),
        ("g", true, None),
        ("w", false, None),
        ("a", false, None),
        ("c", false, None),
        ("v", false, None),
        ("x", false, None),
    ];
    for (k, shift, want) in cases {
        assert_eq!(from_shortcut(k, true, shift), want, "{k} shift={shift}");
        assert_eq!(from_shortcut(k, false, shift), None, "{k} without Ctrl/Cmd");
    }
}

#[test]
fn no_two_commands_share_a_key_combination() {
    for k in "abcdefghijklmnopqrstuvwxyz".chars().map(|c| c.to_string()) {
        for shift in [false, true] {
            let n = COMMANDS
                .iter()
                .filter(|c| c.shortcuts.iter().any(|s| s.matches(&k, shift)))
                .count();
            assert!(n <= 1, "{k} shift={shift} matches {n} commands");
        }
    }
}

#[test]
fn availability_mirrors_the_toolbar_rules() {
    use CommandId::*;
    let none = CommandContext::default();
    let doc = CommandContext {
        has_document: true,
        ..none
    };
    let hist = CommandContext {
        has_document: true,
        can_undo: true,
        can_redo: true,
        ..none
    };
    for id in [FileNewWorld, FileOpen, HelpAbout] {
        assert_eq!(id.availability(none), Availability::Enabled);
    }
    for id in [FileSave, FileSaveAs, FileClose, FileReload] {
        assert_eq!(id.availability(none), Availability::Disabled(NO_DOCUMENT));
        assert_eq!(id.availability(doc), Availability::Enabled);
        assert_eq!(id.refusal(none), Some(NO_DOCUMENT));
        assert_eq!(id.refusal(doc), None);
    }
    assert_eq!(
        EditUndo.availability(doc),
        Availability::Disabled("Nothing to undo")
    );
    assert_eq!(
        EditRedo.availability(doc),
        Availability::Disabled("Nothing to redo")
    );
    assert_eq!(EditUndo.availability(hist), Availability::Enabled);
    assert_eq!(EditRedo.availability(hist), Availability::Enabled);
    // History is the backend's decision at run time, never refused early.
    assert_eq!(EditUndo.refusal(none), None);
    assert_eq!(EditRedo.refusal(none), None);
}

#[test]
fn actions_only_name_allowed_backend_commands() {
    for c in COMMANDS {
        match c.action {
            Action::Backend(cmds) => {
                assert!(!cmds.is_empty(), "{}", c.key);
                for cmd in cmds {
                    assert!(ALLOWED_BACKEND.contains(cmd), "{} calls {cmd}", c.key);
                }
            }
            Action::UiOnly => assert!(!c.mutates_document, "{}", c.key),
            // UI-MENU-1: a Source widget edit reaches Rust only through the
            // editor's doc_edit path, and only with the Source focused.
            Action::SourceEdit => {
                assert!(c.mutates_document, "{}", c.key);
                assert!(
                    matches!(
                        c.requires,
                        Requires::SourceFocus | Requires::SourceSelection
                    ),
                    "{}",
                    c.key
                );
            }
            Action::Planned(why) => {
                assert!(!c.mutates_document, "{}", c.key);
                assert!(!why.is_empty(), "{}", c.key);
            }
        }
    }
    // Nothing in the registry writes text directly: no doc_edit* or field edit.
    for cmd in ALLOWED_BACKEND {
        assert!(
            !cmd.starts_with("doc_edit") && *cmd != "doc_add_field",
            "{cmd}"
        );
    }
}

#[test]
fn labels_are_platform_specific() {
    let s = CommandId::FileSaveAs.spec().shortcuts[0];
    assert_eq!(s.label(Platform::Linux), "Ctrl+Shift+S");
    assert_eq!(s.label(Platform::Windows), "Ctrl+Shift+S");
    assert_eq!(s.label(Platform::MacOs), "⇧⌘S");
    assert_eq!(
        CommandId::FileOpen.title(Platform::Linux),
        "Open a .wrl / .wrz file (Ctrl+O)"
    );
    assert_eq!(
        CommandId::FileClose.title(Platform::Linux),
        CommandId::FileClose.spec().hint
    );
}

/// UI-DOCK-1: the layout commands are View commands that persist settings
/// only; none can reach the document.
#[test]
fn layout_commands_never_touch_the_document() {
    use crate::layout::{PanelId, PresetId};
    use CommandId::*;
    let layout = [
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
    ];
    let keys: Vec<_> = layout.iter().map(|c| c.key()).collect();
    assert_eq!(
        keys,
        [
            "view.panel.sceneTree",
            "view.panel.inspector",
            "view.panel.diagnostics",
            "view.hidePanel.sceneTree",
            "view.hidePanel.inspector",
            "view.hidePanel.diagnostics",
            "view.focus.viewport",
            "view.focus.source",
            "view.preset.modeling",
            "view.preset.source",
            "view.resetLayout",
        ]
    );
    for c in layout {
        let s = c.spec();
        assert_eq!(s.menu, Menu::View, "{}", s.key);
        assert!(!s.mutates_document, "{}", s.key);
        assert_eq!(s.action, Action::Backend(&["layout_set"]), "{}", s.key);
        assert!(
            s.shortcuts.is_empty(),
            "{}: no second shortcut table",
            s.key
        );
        // Usable with no document open.
        assert_ne!(s.requires, Requires::Document);
    }
    assert_eq!(ViewPresetModeling.preset(), Some(PresetId::Modeling));
    assert_eq!(ViewPresetSource.preset(), Some(PresetId::Source));
    assert_eq!(ViewResetLayout.preset(), None);
    assert_eq!(ViewFocusSource.panel(), Some(PanelId::Source));
    // Hide needs the panel shown; required panels have no Hide command.
    assert_eq!(CommandId::hide_for(PanelId::Viewport), None);
    assert_eq!(CommandId::hide_for(PanelId::Source), None);
    let none = CommandContext::default();
    let shown = CommandContext {
        shown: crate::command::PanelSet::of(&[PanelId::Inspector]),
        ..none
    };
    assert_eq!(
        ViewHideInspector.availability(none),
        Availability::Disabled(PANEL_HIDDEN)
    );
    assert_eq!(ViewHideInspector.refusal(none), Some(PANEL_HIDDEN));
    assert_eq!(ViewHideInspector.availability(shown), Availability::Enabled);
    assert_eq!(ViewHideInspector.refusal(shown), None);
    assert_eq!(ViewShowInspector.availability(none), Availability::Enabled);
    assert_eq!(
        ViewHideSceneTree.availability(shown),
        Availability::Disabled(PANEL_HIDDEN)
    );
}

// ---- UI-MENU-1 ----

#[test]
fn every_menu_shows_each_of_its_commands_exactly_once() {
    let mut menus: Vec<Menu> = MENUS.iter().map(|m| m.menu).collect();
    assert_eq!(
        menus,
        [
            Menu::File,
            Menu::Edit,
            Menu::View,
            Menu::Create,
            Menu::Tools,
            Menu::World,
            Menu::Help
        ]
    );
    menus.dedup();
    assert_eq!(menus.len(), 7);
    let mut seen = Vec::new();
    for m in MENUS {
        for e in m.entries {
            let ids: Vec<CommandId> = match *e {
                MenuEntry::Command(id) | MenuEntry::Preset(id) => vec![id],
                MenuEntry::Panel(show, hide) => {
                    assert_eq!(show.panel(), hide.panel());
                    assert_eq!(CommandId::hide_for(show.panel().unwrap()), Some(hide));
                    vec![show, hide]
                }
                MenuEntry::Separator | MenuEntry::Group(_) => vec![],
            };
            for id in ids {
                assert_eq!(
                    id.spec().menu,
                    m.menu,
                    "{} is in the {} menu",
                    id.key(),
                    m.label
                );
                seen.push(id);
            }
        }
    }
    for id in CommandId::ALL {
        assert_eq!(
            seen.iter().filter(|s| **s == id).count(),
            1,
            "{} must appear in exactly one menu entry",
            id.key()
        );
    }
    let mut mn: Vec<_> = MENUS.iter().map(|m| m.mnemonic).collect();
    mn.sort_unstable();
    mn.dedup();
    assert_eq!(mn.len(), 7, "mnemonics are unique");
}

#[test]
fn world_items_are_disabled_with_the_migration_reason() {
    let ctx = CommandContext {
        has_document: true,
        ..CommandContext::default()
    };
    let world: Vec<_> = COMMANDS.iter().filter(|c| c.menu == Menu::World).collect();
    assert_eq!(world.len(), 6);
    for c in world {
        assert_eq!(c.action, Action::Planned(NOT_MIGRATED), "{}", c.key);
        assert_eq!(c.id.availability(ctx), Availability::Disabled(NOT_MIGRATED));
        assert_eq!(c.id.refusal(ctx), Some(NOT_MIGRATED));
    }
    assert_eq!(NOT_MIGRATED, "Not migrated to the Rust application.");
}

#[test]
fn planned_commands_never_run_in_any_state() {
    use crate::layout::PanelId;
    let all = CommandContext {
        has_document: true,
        can_undo: true,
        can_redo: true,
        shown: PanelSet::of(&PanelId::ALL),
        focus: FocusKind::Source,
        text_selected: true,
        native_viewport: false,
        outline_ready: true,
        node_selected: true,
    };
    let planned: Vec<_> = COMMANDS
        .iter()
        .filter(|c| matches!(c.action, Action::Planned(_)))
        .map(|c| c.key)
        .collect();
    assert_eq!(
        planned,
        [
            "view.zoomIn",
            "view.zoomOut",
            "view.zoomReset",
            "world.mall.validate",
            "world.mall.repack",
            "world.project.open",
            "world.project.scan",
            "world.project.preview",
            "world.project.package",
            "help.shortcuts",
            "help.licenses",
        ]
    );
    for c in COMMANDS {
        if let Action::Planned(why) = c.action {
            assert_eq!(c.id.availability(all), Availability::Disabled(why));
            assert!(
                c.shortcuts.is_empty() && c.native_keys.is_empty(),
                "{}",
                c.key
            );
        }
    }
}

#[test]
fn theme_commands_match_the_theme_registry() {
    use crate::theme::THEMES;
    let cmds: Vec<_> = COMMANDS
        .iter()
        .filter_map(|c| c.id.theme().map(|t| (c, t)))
        .collect();
    assert_eq!(cmds.len(), THEMES.len());
    for t in THEMES {
        let id = CommandId::theme_for(t.id).expect("a command per theme");
        assert_eq!(id.spec().label, t.label);
        assert_eq!(id.spec().menu, Menu::View);
        assert!(!id.spec().mutates_document);
    }
    for (c, t) in cmds {
        assert!(
            crate::theme::is_known(t),
            "{} names unknown theme {t}",
            c.key
        );
    }
}

#[test]
fn create_and_tool_commands_map_to_the_existing_actions() {
    use crate::Primitive;
    for prim in Primitive::ALL {
        let id = CommandId::create_for(prim);
        assert_eq!(id.primitive(), Some(prim));
        assert_eq!(id.spec().label, prim.label());
        assert_eq!(id.spec().action, Action::Backend(&["doc_create"]));
        assert_eq!(id.spec().requires, Requires::Document);
    }
    use crate::gizmo::Tool;
    assert_eq!(CommandId::ToolSelect.spec().check, Check::Pick);
    assert_eq!(CommandId::ToolMove.spec().check, Check::Gizmo(Tool::Move));
    assert_eq!(
        CommandId::ToolRotate.spec().check,
        Check::Gizmo(Tool::Rotate)
    );
    assert_eq!(CommandId::ToolScale.spec().check, Check::Gizmo(Tool::Scale));
}

#[test]
fn delete_and_duplicate_need_a_selected_node() {
    let none = CommandContext::default();
    let doc = CommandContext {
        has_document: true,
        ..Default::default()
    };
    let sel = CommandContext {
        has_document: true,
        node_selected: true,
        ..Default::default()
    };
    for id in [CommandId::EditDeleteNode, CommandId::EditDuplicateNode] {
        let s = id.spec();
        assert_eq!(s.action, Action::Backend(&["doc_structure_edit"]));
        assert!(s.mutates_document);
        assert_eq!(id.availability(none), Availability::Disabled(NO_DOCUMENT));
        assert_eq!(id.availability(doc), Availability::Disabled(NEEDS_NODE));
        assert_eq!(id.refusal(doc), Some(NEEDS_NODE));
        assert_eq!(id.availability(sel), Availability::Enabled);
        assert_eq!(id.refusal(sel), None);
    }
    assert_eq!(
        from_shortcut("d", true, false),
        Some(CommandId::EditDuplicateNode)
    );
    assert_eq!(from_shortcut("d", true, true), None);
    assert!(CommandId::EditDeleteNode.spec().shortcuts.is_empty());
}

#[test]
fn quit_uses_the_window_close_gate_and_ctrl_q() {
    let s = CommandId::AppQuit.spec();
    assert_eq!(s.action, Action::Backend(&["app_quit"]));
    assert!(ALLOWED_BACKEND.contains(&"app_quit"));
    assert_eq!(from_shortcut("q", true, false), Some(CommandId::AppQuit));
    assert_eq!(from_shortcut("q", false, false), None);
    assert_eq!(s.shortcuts[0].label(Platform::MacOs), "⌘Q");
    for id in MAC_APP_MENU {
        assert!(matches!(id, CommandId::HelpAbout | CommandId::AppQuit));
    }
}

/// B2: Find / Replace / Go to Line open a UI bar; the bar's replacements go
/// through the editor's doc_edit path, never a registry backend command.
#[test]
fn find_replace_go_to_line_are_ui_commands_needing_a_document() {
    use CommandId::*;
    let none = CommandContext::default();
    let doc = CommandContext {
        has_document: true,
        ..CommandContext::default()
    };
    for (id, k) in [(EditFind, "f"), (EditReplace, "h"), (EditGoToLine, "g")] {
        let s = id.spec();
        assert_eq!(s.menu, Menu::Edit);
        assert_eq!(s.action, Action::UiOnly, "{}", s.key);
        assert!(!s.mutates_document, "{}", s.key);
        assert_eq!(s.shortcuts, &[Shortcut { key: k, shift: S }]);
        assert_eq!(id.availability(none), Availability::Disabled(NO_DOCUMENT));
        assert_eq!(id.refusal(none), Some(NO_DOCUMENT));
        assert_eq!(id.availability(doc), Availability::Enabled);
        assert_eq!(id.refusal(doc), None);
    }
    assert_eq!(EditFind.title(Platform::Linux), "Find text in the Source editor (Ctrl+F)");
    assert_eq!(EditGoToLine.spec().shortcuts[0].label(Platform::MacOs), "⌘G");
}

#[test]
fn clipboard_keys_stay_native_and_are_never_dispatched() {
    use CommandId::*;
    for (id, k) in [
        (EditCut, "x"),
        (EditCopy, "c"),
        (EditPaste, "v"),
        (EditSelectAll, "a"),
    ] {
        let s = id.spec();
        assert!(s.shortcuts.is_empty(), "{}: no dispatched shortcut", s.key);
        assert_eq!(s.native_keys[0].key, k);
        assert_eq!(from_shortcut(k, true, false), None);
    }
    assert_eq!(EditCut.spec().action, Action::SourceEdit);
    assert_eq!(EditPaste.spec().action, Action::SourceEdit);
    assert_eq!(EditCopy.spec().action, Action::UiOnly);
    assert_eq!(EditSelectAll.spec().action, Action::UiOnly);
}

#[test]
fn focus_requirements_explain_why() {
    use CommandId::*;
    let base = CommandContext {
        has_document: true,
        ..CommandContext::default()
    };
    let src = CommandContext {
        focus: FocusKind::Source,
        ..base
    };
    let src_sel = CommandContext {
        text_selected: true,
        ..src
    };
    let field_sel = CommandContext {
        focus: FocusKind::TextField,
        text_selected: true,
        ..base
    };
    assert_eq!(
        EditPaste.availability(base),
        Availability::Disabled(NEEDS_SOURCE)
    );
    assert_eq!(EditPaste.availability(src), Availability::Enabled);
    assert_eq!(
        EditPaste.availability(field_sel),
        Availability::Disabled(NEEDS_SOURCE)
    );
    assert_eq!(
        EditCut.availability(src),
        Availability::Disabled(NEEDS_SOURCE_SELECTION)
    );
    assert_eq!(EditCut.availability(src_sel), Availability::Enabled);
    assert_eq!(EditCut.refusal(field_sel), Some(NEEDS_SOURCE));
    assert_eq!(
        EditCopy.availability(base),
        Availability::Disabled(NEEDS_TEXT_FOCUS)
    );
    assert_eq!(
        EditCopy.availability(src),
        Availability::Disabled(NEEDS_TEXT_SELECTION)
    );
    assert_eq!(EditCopy.availability(field_sel), Availability::Enabled);
    assert_eq!(
        EditSelectAll.availability(base),
        Availability::Disabled(NEEDS_TEXT_FOCUS)
    );
    assert_eq!(EditSelectAll.availability(src), Availability::Enabled);
    // No document: Source edits say so first.
    let none = CommandContext {
        focus: FocusKind::Source,
        text_selected: true,
        ..CommandContext::default()
    };
    assert_eq!(EditPaste.refusal(none), Some(NO_DOCUMENT));
    // Viewport tools.
    assert_eq!(
        ToolMove.availability(CommandContext::default()),
        Availability::Disabled(NO_DOCUMENT)
    );
    assert_eq!(ToolMove.availability(base), Availability::Enabled);
    let native = CommandContext {
        native_viewport: true,
        ..base
    };
    assert_eq!(ToolSelect.refusal(native), Some(NEEDS_XITE));
    assert_eq!(
        ToolExtrusionOutline.availability(base),
        Availability::Disabled(NEEDS_OUTLINE)
    );
    let ready = CommandContext {
        outline_ready: true,
        ..base
    };
    assert_eq!(
        ToolExtrusionOutline.availability(ready),
        Availability::Enabled
    );
}

#[test]
fn every_disabled_state_has_a_reason() {
    use crate::layout::PanelId;
    // Every combination of the boolean facts and focus kinds.
    for bits in 0u32..128 {
        for focus in [FocusKind::Other, FocusKind::Source, FocusKind::TextField] {
            let ctx = CommandContext {
                has_document: bits & 1 != 0,
                can_undo: bits & 2 != 0,
                can_redo: bits & 4 != 0,
                shown: if bits & 8 != 0 {
                    PanelSet::of(&PanelId::ALL)
                } else {
                    PanelSet::default()
                },
                focus,
                text_selected: bits & 16 != 0,
                native_viewport: bits & 32 != 0,
                outline_ready: bits & 16 != 0,
                node_selected: bits & 64 != 0,
            };
            for c in COMMANDS {
                if let Availability::Disabled(why) = c.id.availability(ctx) {
                    assert!(!why.is_empty(), "{}", c.key);
                }
                // A run-time refusal is always also shown as disabled.
                if let Some(why) = c.id.refusal(ctx) {
                    assert_eq!(
                        c.id.availability(ctx),
                        Availability::Disabled(why),
                        "{}",
                        c.key
                    );
                }
            }
        }
    }
}

#[test]
fn native_menu_events_accept_only_app_menu_keys() {
    assert_eq!(MENU_EVENT, "menu-command");
    for c in MAC_APP_MENU {
        assert_eq!(from_native_menu(c.key()), Some(*c));
    }
    // A real registry command outside the application menu is refused.
    assert_eq!(from_native_menu(CommandId::FileSave.key()), None);
    assert_eq!(from_native_menu(CommandId::EditCut.key()), None);
    for bad in [
        "",
        "app.quit ",
        "APP.QUIT",
        "help",
        "file.delete-everything",
    ] {
        assert_eq!(from_native_menu(bad), None, "{bad:?}");
    }
}

// ---- B2-CAMERA ----

#[test]
fn frame_selection_is_a_camera_item_for_a_selected_node() {
    use CommandId::*;
    let s = ViewFrameSelection.spec();
    assert_eq!((s.menu, s.action), (Menu::View, Action::UiOnly));
    assert!(!s.mutates_document);
    assert_eq!(s.requires, Requires::NodeSelected);
    assert_eq!(ViewFrameSelection.standard_view(), None);
    let view = MENUS.iter().find(|m| m.menu == Menu::View).unwrap();
    let at = view
        .entries
        .iter()
        .position(|e| *e == MenuEntry::Command(ViewResetCamera))
        .unwrap();
    assert_eq!(view.entries[at + 1], MenuEntry::Command(ViewFrameSelection));
}

#[test]
fn camera_commands_are_view_items_that_never_touch_the_document() {
    use crate::camera::StandardView;
    use CommandId::*;
    let cams = [
        ViewFitAll,
        ViewFront,
        ViewBack,
        ViewLeft,
        ViewRight,
        ViewTop,
        ViewBottom,
        ViewResetCamera,
    ];
    for id in cams {
        let s = id.spec();
        assert_eq!(s.menu, Menu::View, "{}", s.key);
        assert_eq!(s.action, Action::UiOnly, "{}", s.key);
        assert!(!s.mutates_document, "{}", s.key);
        assert_eq!(s.requires, Requires::ViewportCamera, "{}", s.key);
        assert!(s.key.starts_with("view.camera."), "{}", s.key);
    }
    let views: Vec<_> = cams.iter().filter_map(|c| c.standard_view()).collect();
    assert_eq!(views, StandardView::ALL);
    assert_eq!(ViewFitAll.standard_view(), None);
    assert_eq!(ViewResetCamera.standard_view(), None);
    // One "Camera" group in View, in this order.
    let view = MENUS.iter().find(|m| m.menu == Menu::View).unwrap();
    let at = view
        .entries
        .iter()
        .position(|e| *e == MenuEntry::Group("Camera"))
        .expect("a Camera group");
    let got: Vec<_> = view.entries[at + 1..at + 1 + cams.len()].to_vec();
    assert_eq!(got, cams.map(MenuEntry::Command));
    // Availability: a document in the X_ITE viewport.
    assert_eq!(
        ViewFitAll.availability(CommandContext::default()),
        Availability::Disabled(NO_DOCUMENT)
    );
    let doc = CommandContext {
        has_document: true,
        ..CommandContext::default()
    };
    assert_eq!(ViewTop.availability(doc), Availability::Enabled);
    let native = CommandContext {
        native_viewport: true,
        ..doc
    };
    assert_eq!(ViewTop.refusal(native), Some(NEEDS_XITE_CAMERA));
}

#[test]
fn viewport_keys_fit_only_unmodified_and_never_shadow_a_shortcut() {
    use CommandId::*;
    assert_eq!(from_viewport_key("f", false), Some(ViewFitAll));
    assert_eq!(from_viewport_key("F", false), Some(ViewFitAll));
    assert_eq!(from_viewport_key("Home", false), Some(ViewFitAll));
    assert_eq!(from_viewport_key("f", true), None);
    assert_eq!(from_viewport_key("g", false), None);
    assert_eq!(viewport_key_label(ViewFitAll), "F / Home");
    assert_eq!(viewport_key_label(ViewTop), "");
    assert_eq!(
        ViewFitAll.title(Platform::Linux),
        format!("{} (F / Home in the viewport)", ViewFitAll.spec().hint)
    );
    // A viewport key never fires with the primary modifier, so Ctrl+F
    // stays Find while a bare F in the viewport is Fit.
    for (k, _) in VIEWPORT_KEYS {
        assert_eq!(from_viewport_key(k, true), None, "{k}");
    }
    assert_eq!(from_shortcut("f", true, false), Some(EditFind));
}
