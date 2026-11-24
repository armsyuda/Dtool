use dtool_core::object::ObjectId;
use dtool_core::workspace::Workspace;
use dtool_ui::panels::layers::{
    drop_command, drop_zone_at, filter_rows, thumbnail_command, thumbnail_request, DropZone, LAYER_THUMB_PX, LayerRow,
    LayersPanel,
};
use serde_json::json;

fn create(workspace: &mut Workspace, name: &str) -> u64 {
    workspace
        .session_mut()
        .unwrap()
        .run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]}))
        .unwrap();
    workspace.session().unwrap().document.objects.last().unwrap().id.0
}

fn row(id: u64, name: &str) -> LayerRow {
    LayerRow { id, depth: 0, name: name.into(), selected: false, has_children: false, visible: true, locked: false }
}

#[test]
fn drag_inside_a_group_becomes_node_reparent() {
    let mut workspace = Workspace::default();
    workspace.new_document("c06");
    let a = create(&mut workspace, "a");
    let b = create(&mut workspace, "b");
    let group = workspace
        .session_mut()
        .unwrap()
        .run_named("node.group", json!({"empty": true, "name": "g"}))
        .unwrap();
    let group_id = group.get("id").and_then(|value| value.as_u64()).or_else(|| {
        workspace.session().unwrap().document.objects.iter().find(|object| object.name == "g").map(|object| object.id.0)
    }).expect("group");
    let mut panel = LayersPanel::default();
    let zone = DropZone::Inside(ObjectId(group_id));
    let command = drop_command(ObjectId(b), zone, &dtool_ui::panels::layers::document_nodes(&workspace.session().unwrap().document));
    assert_eq!(command.name, "node.reparent");
    assert_eq!(command.params["ids"], json!([b]));
    assert_eq!(command.params["parent"], json!(group_id));
    assert!(panel.apply_drop(&mut workspace, ObjectId(b), zone));
    let child = workspace.session().unwrap().document.get_object(ObjectId(b)).unwrap();
    assert_eq!(child.parent_id, Some(ObjectId(group_id)));
    let _ = a;
}

#[test]
fn filter_hides_rows_not_objects() {
    let mut workspace = Workspace::default();
    workspace.new_document("c06");
    let star = create(&mut workspace, "star");
    let box_id = create(&mut workspace, "box");
    let mut panel = LayersPanel::default();
    panel.tags.insert(star, "red".into());
    let before = workspace.session().unwrap().document.objects.len();
    let all = panel.rows(&mut workspace);
    assert_eq!(all.len(), 2);
    panel.filter = "star".into();
    let by_name = panel.rows(&mut workspace);
    assert_eq!(by_name.iter().map(|row| row.id).collect::<Vec<_>>(), vec![star]);
    panel.filter = "red".into();
    let by_tag = filter_rows(&all, "red", &panel.tags);
    assert_eq!(by_tag.iter().map(|row| row.id).collect::<Vec<_>>(), vec![star]);
    assert_eq!(workspace.session().unwrap().document.objects.len(), before);
    assert!(workspace.session().unwrap().document.get_object(ObjectId(star)).is_some());
    assert!(workspace.session().unwrap().document.get_object(ObjectId(box_id)).is_some());
}

#[test]
fn thumbnail_matches_object_preview_helper() {
    let id = 42u64;
    let command = thumbnail_command(id);
    assert_eq!(command.name, "render.node_preview");
    let request = thumbnail_request(id).expect("request");
    let helper = dtool_core::commands::preview::parse(&json!({ "id": id, "max_px": LAYER_THUMB_PX })).expect("helper");
    assert_eq!(request.id, helper.id);
    assert_eq!(request.max_px, helper.max_px);
    assert_eq!(request.id, Some(ObjectId(id)));
    let _ = drop_zone_at(ObjectId(id), true, 0.5);
    let _ = row(id, "n");
}
