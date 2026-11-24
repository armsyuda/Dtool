use dtool_core::workspace::Workspace;
use dtool_ui::dialogs::blend_options::{blend_set_command, BlendOptionsDialog};
use dtool_ui::dialogs::layer_effects::{add_command, cancel_command, preview_command, LayerEffectsDialog, OUTER_SHADOW};
use serde_json::json;

fn primed() -> (Workspace, u64) {
    let mut workspace = Workspace::default();
    workspace.new_document("c08");
    workspace
        .session_mut()
        .unwrap()
        .run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [40.0, 30.0]}))
        .unwrap();
    let id = workspace.session().unwrap().document.objects.last().unwrap().id.0;
    (workspace, id)
}

fn undo_depth(workspace: &Workspace) -> usize {
    format!("{:?}", workspace.session().unwrap().undo).matches("Change {").count()
}

fn blend_mode(workspace: &Workspace) -> serde_json::Value {
    let object = workspace.session().unwrap().document.objects.last().unwrap();
    serde_json::to_value(object).unwrap().get("blend_mode").cloned().unwrap_or(serde_json::Value::Null)
}

fn version(workspace: &Workspace) -> u64 {
    workspace.session().unwrap().document.version
}

#[test]
fn layer_effects_outer_shadow_cancel_leaves_document_and_undo() {
    let (mut workspace, id) = primed();
    let before_version = version(&workspace);
    let before_depth = undo_depth(&workspace);
    assert_eq!(preview_command(id, OUTER_SHADOW).name, "effect.preview");
    assert_eq!(cancel_command(id).name, "effect.cancel");
    let mut dialog = LayerEffectsDialog::default();
    dialog.open_for(id);
    dialog.enable_outer_shadow(workspace.session_mut().unwrap());
    dialog.cancel(workspace.session_mut().unwrap());
    assert!(!dialog.open);
    assert_eq!(version(&workspace), before_version);
    assert_eq!(undo_depth(&workspace), before_depth);
}

#[test]
fn blend_options_ok_is_one_undo_or_module_command() {
    let (mut workspace, id) = primed();
    let command = blend_set_command(id, "Multiply");
    assert_eq!(command.name, "layer.blend.set");
    assert_eq!(command.params["id"], json!(id));
    assert_eq!(command.params["mode"], json!("Multiply"));
    assert_eq!(add_command(id, OUTER_SHADOW).name, "effect.add");
    let before_version = version(&workspace);
    let before_depth = undo_depth(&workspace);
    let mut dialog = BlendOptionsDialog::default();
    dialog.open_for(id, "Normal");
    dialog.set_mode("Multiply");
    if dialog.apply(workspace.session_mut().unwrap()) {
        assert_eq!(undo_depth(&workspace), before_depth + 1);
        assert_eq!(blend_mode(&workspace), json!("Multiply"));
        workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
        assert_eq!(blend_mode(&workspace), serde_json::Value::Null);
    } else {
        assert_eq!(version(&workspace), before_version);
        assert_eq!(undo_depth(&workspace), before_depth);
    }
}
