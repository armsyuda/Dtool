use dtool_core::document::{Document, Units};
use dtool_core::Session;
use dtool_ui::shortcut_commands::{self, ShellAction, ShortcutTarget};
use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap};
use serde_json::json;

fn m(ctrl: bool, shift: bool, alt: bool) -> Modifiers { Modifiers { ctrl, shift, alt } }

fn session() -> Session {
    let mut s = Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1));
    for (name, x) in [("a", 0.0), ("b", 20.0)] {
        s.run_named("object.create", json!({"name": name, "pos": [x, x], "size": [10.0, 10.0]})).unwrap();
    }
    s.selection.ids = s.document.objects.iter().map(|o| o.id).collect();
    s
}

#[test]
fn every_table_id_is_bound_in_default_keymap() {
    let commands = ShortcutMap::affinity_default().commands();
    for (id, _) in shortcut_commands::entries() {
        assert!(commands.contains(&id), "{id}");
    }
}

#[test]
fn every_target_reaches_a_known_dtool_command() {
    for (id, target) in shortcut_commands::entries() {
        let mut s = session();
        match shortcut_commands::run_target(&mut s, target) {
            Some(Err(e)) => assert_ne!(e.code, "UNKNOWN_COMMAND", "{id}"),
            Some(Ok(_)) => {}
            None => assert!(matches!(target, ShortcutTarget::Shell(ShellAction::OpenDocument | ShellAction::SaveDocumentAs | ShellAction::NewDocument | ShellAction::NewFromClipboard | ShellAction::SaveDocument | ShellAction::CloseDocument | ShellAction::OpenFind)), "{id}"),
        }
        if let ShortcutTarget::Tool(tool) = target {
            assert_eq!(s.active_tool, tool, "{id}");
        }
    }
}

#[test]
fn resolve_picks_mapped_command_by_table_priority() {
    let map = ShortcutMap::affinity_default();
    let r = |key: &str, mods| shortcut_commands::resolve(&map, Focus::Canvas, key, mods);
    assert_eq!(r("Z", m(true, false, false)), Some(ShortcutTarget::Run { command: "edit.undo", params: "{}" }));
    assert_eq!(r("Z", m(true, true, false)), Some(ShortcutTarget::Run { command: "edit.redo", params: "{}" }));
    assert_eq!(r("V", m(false, false, false)), Some(ShortcutTarget::Tool("tool.move")));
    assert_eq!(r("R", m(false, false, false)), Some(ShortcutTarget::Tool("tool.shape.rectangle")));
    assert_eq!(r("T", m(false, false, false)), Some(ShortcutTarget::Tool("tool.text.artistic")));
    assert_eq!(r("G", m(true, false, false)), Some(ShortcutTarget::Run { command: "node.group", params: "{}" }));
    assert_eq!(r("O", m(true, false, false)), Some(ShortcutTarget::Shell(ShellAction::OpenDocument)));
    assert_eq!(r("OemPlus", m(true, false, false)), Some(ShortcutTarget::Shell(ShellAction::ZoomIn)));
    assert_eq!(r("X", m(false, false, false)), Some(ShortcutTarget::NotImplemented));
    assert_eq!(r("Q", m(false, false, true)), None);
}

#[test]
fn reserved_keys_and_non_canvas_focus_never_dispatch() {
    let map = ShortcutMap::affinity_default();
    for (key, mods) in [("Delete", m(false, false, false)), ("Delete", m(false, false, true)), ("Up", m(false, false, true)), ("Left", m(false, true, false)), ("V", m(true, true, false)), ("C", m(true, true, false)), ("Tab", m(false, false, false))] {
        assert!(shortcut_commands::is_reserved(key, mods), "{key}");
        assert_eq!(shortcut_commands::resolve(&map, Focus::Canvas, key, mods), None, "{key}");
    }
    assert!(!shortcut_commands::is_reserved("V", m(true, false, false)));
    for focus in [Focus::TextField, Focus::Dialog, Focus::Menu, Focus::Other] {
        assert_eq!(shortcut_commands::resolve(&map, focus, "Z", m(true, false, false)), None);
    }
}

#[test]
fn dispatch_all_lists_every_binding_for_a_chord() {
    let map = ShortcutMap::affinity_default();
    let v = map.dispatch_all(Focus::Canvas, "v", m(false, false, false));
    assert!(v.contains(&"tool.Select".to_string()) && v.contains(&"tool.Node".to_string()));
    assert!(map.dispatch_all(Focus::TextField, "v", m(false, false, false)).is_empty());
}

#[test]
fn egui_keys_map_to_keymap_names() {
    use egui::Key;
    let cases = [(Key::A, "A"), (Key::Num1, "D1"), (Key::F5, "F5"), (Key::Plus, "OemPlus"), (Key::Equals, "OemPlus"), (Key::Minus, "OemMinus"), (Key::Backtick, "Oem3"), (Key::Backslash, "Oem5"), (Key::Semicolon, "Oem1"), (Key::Quote, "OemQuotes"), (Key::Comma, "OemComma"), (Key::PageDown, "Next"), (Key::Backspace, "Back"), (Key::ArrowUp, "Up")];
    for (key, name) in cases {
        assert_eq!(shortcut_commands::key_name(key).as_deref(), Some(name), "{key:?}");
    }
    let mods = shortcut_commands::modifiers_from(egui::Modifiers { ctrl: true, shift: true, ..Default::default() });
    assert_eq!(mods, m(true, true, false));
}

#[test]
fn zoom_in_and_out_step_from_current_zoom() {
    let mut s = session();
    shortcut_commands::run_target(&mut s, ShortcutTarget::Shell(ShellAction::ZoomIn)).unwrap().unwrap();
    assert_eq!(s.view.zoom, 2.0);
    shortcut_commands::run_target(&mut s, ShortcutTarget::Shell(ShellAction::ZoomOut)).unwrap().unwrap();
    shortcut_commands::run_target(&mut s, ShortcutTarget::Shell(ShellAction::ZoomOut)).unwrap().unwrap();
    assert_eq!(s.view.zoom, 0.5);
}
