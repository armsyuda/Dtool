use dtool_core::boolean::PATH_KNIFE;
use dtool_core::context_toolbar::{options_for, prepare_knife_cut, OPTION_SET};
use dtool_core::object::Object;
use dtool_core::selection::SelectOp;
use dtool_core::workspace::Workspace;
use serde_json::json;

fn primed() -> Workspace {
    let mut workspace = Workspace::default();
    workspace.new_document("v23b");
    let session = workspace.session_mut().unwrap();
    let id = session.document.create_object(Object {
        name: "r".into(),
        pos: [0.0, 0.0],
        size: [100.0, 100.0],
        ..Object::default()
    });
    session.selection.set(vec![id], SelectOp::Replace);
    workspace
}

fn object_count(workspace: &Workspace) -> usize {
    workspace.session().unwrap().document.objects.len()
}

fn almost_closed_cut(auto_close: bool) -> serde_json::Value {
    json!({
        "cut": [[20.0, 20.0], [80.0, 20.0], [80.0, 80.0], [20.0, 80.0], [20.0, 20.5]],
        "auto_close": auto_close
    })
}

#[test]
fn knife_option_set_does_not_bump_document_version() {
    let mut workspace = primed();
    let version = workspace.session().unwrap().document.version;
    workspace
        .session_mut()
        .unwrap()
        .run_named(OPTION_SET, json!({ "tool": "tool.knife", "id": "knife.auto_close", "value": false }))
        .unwrap();
    workspace
        .session_mut()
        .unwrap()
        .run_named(OPTION_SET, json!({ "tool": "tool.knife", "id": "knife.smoothness", "value": 40.0 }))
        .unwrap();
    workspace
        .session_mut()
        .unwrap()
        .run_named(OPTION_SET, json!({ "tool": "tool.knife", "id": "knife.stabiliser", "value": 16.0 }))
        .unwrap();
    assert_eq!(workspace.session().unwrap().document.version, version);
    let ids: Vec<&str> = options_for("tool.knife").iter().map(|spec| spec.id).collect();
    assert!(ids.contains(&"knife.auto_close"));
    assert!(ids.contains(&"knife.smoothness"));
    assert!(ids.contains(&"knife.stabiliser"));
    assert_eq!(workspace.session().unwrap().tool_options.flag("tool.knife", "knife.auto_close"), Some(false));
}

#[test]
fn auto_close_on_and_off_change_the_cut() {
    let off = prepare_knife_cut(vec![[20.0, 20.0], [80.0, 20.0], [80.0, 80.0], [20.0, 80.0], [20.0, 20.5]], false, 0.0, 0.0);
    let on = prepare_knife_cut(vec![[20.0, 20.0], [80.0, 20.0], [80.0, 80.0], [20.0, 80.0], [20.0, 20.5]], true, 0.0, 0.0);
    assert_eq!(off.len(), 5);
    assert_eq!(on.len(), 6);
    assert_eq!(on[0], on[5]);

    let mut workspace = primed();
    let off_result = workspace.session_mut().unwrap().run_named(PATH_KNIFE, almost_closed_cut(false));
    assert_eq!(off_result.unwrap_err().code, "NOTHING_TO_CUT");
    assert_eq!(object_count(&workspace), 1);
    let on_result = workspace.session_mut().unwrap().run_named(PATH_KNIFE, almost_closed_cut(true)).unwrap();
    assert!(object_count(&workspace) > 1);
    assert_eq!(on_result["unused"], json!([]));
}

#[test]
fn path_knife_undo_restores_after_auto_close_cut() {
    let mut workspace = primed();
    let mut before = serde_json::to_value(&workspace.session().unwrap().document).unwrap();
    before.as_object_mut().unwrap().remove("version");
    workspace.session_mut().unwrap().run_named(PATH_KNIFE, almost_closed_cut(true)).unwrap();
    let mut mid = serde_json::to_value(&workspace.session().unwrap().document).unwrap();
    mid.as_object_mut().unwrap().remove("version");
    assert_ne!(mid["objects"], before["objects"]);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    let mut after = serde_json::to_value(&workspace.session().unwrap().document).unwrap();
    after.as_object_mut().unwrap().remove("version");
    assert_eq!(after["objects"], before["objects"]);
}
