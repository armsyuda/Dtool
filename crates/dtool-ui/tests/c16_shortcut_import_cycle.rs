use dtool_ui::shell::AppShell;
use dtool_ui::shortcut_commands::{self, ShortcutTarget};
use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap, CMD_IMPORT};
use serde_json::json;

fn m(ctrl: bool, shift: bool, alt: bool) -> Modifiers { Modifiers { ctrl, shift, alt } }

fn temp_file(name: &str, content: &str) -> String {
    let path = std::env::temp_dir().join(format!("dtool_c16_{}_{name}", std::process::id()));
    std::fs::write(&path, content).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn import_empty_map_keeps_existing_bindings() {
    let mut map = ShortcutMap::affinity_default();
    let before = map.list();
    assert!(!before.is_empty());
    for (name, content) in [("a.json", r#"{"bindings":{}}"#), ("b.json", "[]"), ("c.json", "{}")] {
        let path = temp_file(name, content);
        assert!(ShortcutMap::import_file(&path).is_err(), "{name}");
        assert_eq!(map.run(CMD_IMPORT, &json!({ "path": path })).unwrap_err(), "INVALID_PARAMS");
        assert_eq!(map.list(), before);
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn import_non_empty_map_is_accepted() {
    let path = temp_file("d.json", r#"[{"command":"Serif.Interop.Persona.Commands.UndoCommand","key":"Z","modifiers":"Ctrl"}]"#);
    let map = ShortcutMap::import_file(&path).unwrap();
    assert!(!map.is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn unmapped_affinity_id_is_not_implemented() {
    let mut map = ShortcutMap::default();
    map.set("Serif.Interop.Persona.Commands.SomethingUnknownCommand", "Q", m(true, false, true));
    map.set("tool.SomethingUnknown", "J", m(false, false, false));
    assert_eq!(shortcut_commands::resolve(&map, Focus::Canvas, "Q", m(true, false, true)), Some(ShortcutTarget::NotImplemented));
    assert_eq!(shortcut_commands::resolve(&map, Focus::Canvas, "J", m(false, false, false)), Some(ShortcutTarget::NotImplemented));
    assert_eq!(shortcut_commands::resolve(&map, Focus::Canvas, "K", m(false, false, false)), None);
    assert_eq!(shortcut_commands::resolve(&map, Focus::TextField, "Q", m(true, false, true)), None);
    let mut s = dtool_core::Session::new(dtool_core::Document::new([100.0, 100.0], dtool_core::document::Units::Px, 72.0, 1));
    let err = shortcut_commands::run_target(&mut s, ShortcutTarget::NotImplemented).unwrap().unwrap_err();
    assert_eq!(err.code, "NOT_IMPLEMENTED");
}

#[test]
fn shell_reports_unmapped_id_on_status_bar() {
    let mut shell = AppShell::new();
    let mut map = ShortcutMap::default();
    map.set("Serif.Interop.Persona.Commands.SomethingUnknownCommand", "Q", m(true, false, true));
    shell.shortcuts = map;
    shell.status_bar.hint_key.clear();
    assert_eq!(shell.handle_shortcut_key(Focus::Canvas, "Q", m(true, false, true)), Some(ShortcutTarget::NotImplemented));
    assert_eq!(shell.status_bar.hint_key, "error.not_implemented");
}

#[test]
fn same_key_cycles_flyout_tools() {
    let map = ShortcutMap::affinity_default();
    let tools = shortcut_commands::bound_tools(&map, Focus::Canvas, "R", m(false, false, false));
    assert!(tools.len() >= 2, "{tools:?}");
    let c = |current: &str| shortcut_commands::resolve_cycling(&map, Focus::Canvas, "R", m(false, false, false), current, false);
    assert_eq!(c("tool.move"), Some(ShortcutTarget::Tool(tools[0])));
    let mut current = tools[0];
    for step in 1..=tools.len() {
        let Some(ShortcutTarget::Tool(next)) = c(current) else { panic!("no tool") };
        assert_eq!(next, tools[step % tools.len()]);
        current = next;
    }
}

#[test]
fn shift_cycles_tools_needs_shift_to_cycle() {
    let map = ShortcutMap::affinity_default();
    let key = ["R", "T", "G", "P", "B"].into_iter().find(|key| {
        shortcut_commands::bound_tools(&map, Focus::Canvas, key, m(false, false, false)).len() >= 2 && map.dispatch_all(Focus::Canvas, key, m(false, true, false)).is_empty()
    }).unwrap();
    let tools = shortcut_commands::bound_tools(&map, Focus::Canvas, key, m(false, false, false));
    let c = |current: &str, shift: bool| shortcut_commands::resolve_cycling(&map, Focus::Canvas, key, m(false, shift, false), current, true);
    assert_eq!(c(tools[0], false), Some(ShortcutTarget::Tool(tools[0])));
    assert_eq!(c(tools[0], true), Some(ShortcutTarget::Tool(tools[1])));
    assert_eq!(c("tool.move", false), Some(ShortcutTarget::Tool(tools[0])));
}

#[test]
fn explicit_shift_binding_wins_over_cycling() {
    let map = ShortcutMap::affinity_default();
    let explicit = shortcut_commands::resolve(&map, Focus::Canvas, "V", m(false, true, false));
    assert!(explicit.is_some());
    assert_eq!(shortcut_commands::resolve_cycling(&map, Focus::Canvas, "V", m(false, true, false), "tool.move", true), explicit);
}

#[test]
fn shell_cycles_active_tool() {
    let mut shell = AppShell::new();
    shell.shortcuts = ShortcutMap::affinity_default();
    shell.prefs.shift_cycles_tools = false;
    let state = dtool_ui::dialogs::NewDocumentDialogState::default();
    dtool_ui::dialogs::create_new_document(&mut shell.workspace, &state).unwrap();
    let tools = shortcut_commands::bound_tools(&shell.shortcuts, Focus::Canvas, "R", m(false, false, false));
    shell.handle_shortcut_key(Focus::Canvas, "R", m(false, false, false)).unwrap();
    let first = shell.workspace.session().unwrap().active_tool.clone();
    assert_eq!(first, dtool_core::context_toolbar::active_id(tools[0]));
    shell.handle_shortcut_key(Focus::Canvas, "R", m(false, false, false)).unwrap();
    assert_eq!(shell.workspace.session().unwrap().active_tool, dtool_core::context_toolbar::active_id(tools[1]));
}