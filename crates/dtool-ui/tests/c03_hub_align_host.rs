#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, ObjectId};
use dtool_ui::context_bar::align::{AlignTarget, ALIGN_POPUP_COMMAND};
use dtool_ui::menu_tree::MenuTree;
use dtool_ui::shell::AppShell;
use serde_json::json;

fn shell_with_three() -> (AppShell, [ObjectId; 3]) {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("align");
    shell.workspace.activate(id).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    let mut ids = Vec::new();
    for (x, w) in [(10.0, 10.0), (50.0, 20.0), (100.0, 10.0)] {
        let next = session.document.next_object_id;
        session.run_named("object.create", json!({"name": "o", "pos": [x, 0.0], "size": [w, 10.0]})).unwrap();
        ids.push(ObjectId(next));
    }
    session.selection.ids = ids.clone();
    (shell, [ids[0], ids[1], ids[2]])
}

fn x(shell: &AppShell, id: ObjectId) -> f32 {
    shell.workspace.session().unwrap().document.get_object(id).unwrap().pos[0]
}

#[test]
fn space_and_distribute_menu_leaves_reach_distribute_commands() {
    let _home = test_home::enter("space_and_distribute_menu_leaves_reach_distribute_commands");
    let tree = MenuTree::for_persona("pixel");
    for (id, command) in [
        ("layer.arrange.align.space_horizontally", "layer.arrange.distribute.h"),
        ("layer.arrange.align.space_vertically", "layer.arrange.distribute.v"),
        ("layer.arrange.align.distribute_horizontally", "layer.arrange.distribute.h"),
        ("layer.arrange.align.distribute_vertically", "layer.arrange.distribute.v"),
    ] {
        let item = tree.find(id).expect(id);
        assert_eq!(item.command, Some(command), "{id}");
        assert!(!item.planned, "{id}");
    }
    let (mut shell, [a, b, c]) = shell_with_three();
    let command = tree.find("layer.arrange.align.space_horizontally").unwrap().command.unwrap();
    shell.run_and_reconcile(Command::with_params(command, json!({}))).unwrap();
    assert_eq!([x(&shell, a), x(&shell, b), x(&shell, c)], [10.0, 50.0, 100.0]);
    shell.workspace.session_mut().unwrap().document.objects.iter_mut().find(|o| o.id == b).unwrap().pos[0] = 30.0;
    shell.run_and_reconcile(Command::with_params(command, json!({}))).unwrap();
    assert_eq!(x(&shell, b), 50.0);
    shell.workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    assert_eq!(x(&shell, b), 30.0);
}

#[test]
fn move_align_context_action_toggles_hosted_popup() {
    let _home = test_home::enter("move_align_context_action_toggles_hosted_popup");
    let (mut shell, [a, b, _]) = shell_with_three();
    assert!(!shell.align_popup_open);
    assert_eq!(shell.align_popup.target, AlignTarget::Selection);
    shell.apply_context_command(Command::new(ALIGN_POPUP_COMMAND)).unwrap();
    assert!(shell.align_popup_open);
    shell.align_popup.spacing = Some(4.0);
    let command = shell.align_popup.distribute_command(true);
    shell.run_and_reconcile(command).unwrap();
    assert_eq!(x(&shell, a), 10.0);
    assert_eq!(x(&shell, b), 24.0);
    shell.apply_context_command(Command::new(ALIGN_POPUP_COMMAND)).unwrap();
    assert!(!shell.align_popup_open);
}

#[test]
fn hosted_popup_renders_inside_shell_frame() {
    let _home = test_home::enter("hosted_popup_renders_inside_shell_frame");
    let (mut shell, _) = shell_with_three();
    shell.align_popup_open = true;
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |ui| shell.show_align_popup(ui.ctx())).textures_delta.clear();
    assert!(shell.align_popup_open);
}