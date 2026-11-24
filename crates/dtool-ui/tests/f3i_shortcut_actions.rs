#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;
use dtool_ui::shortcut_commands::{self, ShellAction, ShortcutTarget};
use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap};
use serde_json::json;

fn m(ctrl: bool, shift: bool, alt: bool) -> Modifiers { Modifiers { ctrl, shift, alt } }

#[test]
fn f3i_keys_resolve() {
    let _home = test_home::enter("f3i_keys_resolve");
    let map = ShortcutMap::affinity_default();
    let r = |key: &str, mods| shortcut_commands::resolve(&map, Focus::Canvas, key, mods);
    assert_eq!(r("A", m(false, true, true)), Some(ShortcutTarget::Run { command: "layer.arrange.align.left", params: "{}" }));
    assert_eq!(r("S", m(false, true, true)), Some(ShortcutTarget::Run { command: "layer.arrange.align.bottom", params: "{}" }));
    assert_eq!(r("R", m(true, false, false)), Some(ShortcutTarget::Run { command: "ruler.toggle", params: "{}" }));
    assert_eq!(r("Oem1", m(true, false, false)), Some(ShortcutTarget::Run { command: "guides.toggle", params: "{}" }));
    assert_eq!(r("N", m(true, false, false)), Some(ShortcutTarget::Shell(ShellAction::NewDocument)));
    assert_eq!(r("S", m(true, false, false)), Some(ShortcutTarget::Shell(ShellAction::SaveDocument)));
    assert_eq!(r("W", m(true, false, false)), Some(ShortcutTarget::Shell(ShellAction::CloseDocument)));
}

#[test]
fn shell_new_save_close_and_align_dispatch() {
    let _home = test_home::enter("shell_new_save_close_and_align_dispatch");
    let mut shell = AppShell::new();
    let before = shell.workspace.contexts().len();
    assert!(shell.handle_shortcut_key(Focus::Canvas, "N", m(true, false, false)).is_some());
    assert_eq!(shell.workspace.contexts().len(), before);
    let state = shell.new_document_dialog.take().unwrap();
    dtool_ui::dialogs::create_new_document(&mut shell.workspace, &state).unwrap();
    assert_eq!(shell.workspace.contexts().len(), before + 1);
    let id = shell.workspace.active().unwrap();
    let session = shell.workspace.session_mut().unwrap();
    let a = session.document.next_object_id;
    session.run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    let b = session.document.next_object_id;
    session.run_named("object.create", json!({"name": "b", "pos": [40.0, 20.0], "size": [10.0, 10.0]})).unwrap();
    session.selection.ids = vec![dtool_core::ObjectId(a), dtool_core::ObjectId(b)];
    shell.handle_shortcut_key(Focus::Canvas, "A", m(false, true, true)).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    assert_eq!(session.document.get_object(dtool_core::ObjectId(b)).unwrap().pos, [0.0, 20.0]);
    shell.file_dialog = None;
    shell.handle_shortcut_key(Focus::Canvas, "S", m(true, false, false)).unwrap();
    assert!(shell.file_dialog.is_some());
    shell.handle_shortcut_key(Focus::Canvas, "W", m(true, false, false)).unwrap();
    assert_eq!(shell.pending_close_id, Some(id));
}
