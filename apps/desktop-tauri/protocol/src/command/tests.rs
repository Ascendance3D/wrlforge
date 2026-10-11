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
