use dtool_core::selection::SelectOp;
use dtool_core::{Command, ContextId, Object, ObjectId, ObjectKind, Scope, Workspace};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Result<Value, String> {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.map_err(|e| e.code)
}

fn count(ws: &Workspace, id: ContextId) -> usize {
    ws.get(id).unwrap().session.document.objects.len()
}

fn rect(ws: &mut Workspace, id: ContextId) -> ObjectId {
    let session = &mut ws.get_mut(id).unwrap().session;
    let rect = session.document.create_object(Object { name: "r".into(), size: [10.0, 10.0], ..Object::default() });
    session.selection.set(vec![rect], SelectOp::Replace);
    rect
}

#[test]
fn paste_special_without_format_pastes_objects_and_undoes() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    rect(&mut ws, id);
    run(&mut ws, id, "edit.copy", json!({})).unwrap();
    run(&mut ws, id, "edit.paste_special", json!({})).unwrap();
    assert_eq!(count(&ws, id), 2);
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(count(&ws, id), 1);
    run(&mut ws, id, "edit.redo", json!({})).unwrap();
    assert_eq!(count(&ws, id), 2);
}

#[test]
fn paste_special_text_makes_art_text() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    ws.get_mut(id).unwrap().session.clipboard.set_text("hello".to_string());
    let reply = run(&mut ws, id, "edit.paste_special", json!({ "format": "CF_UNICODETEXT", "at": [3.0, 4.0] })).unwrap();
    let new_id = reply["ids"][0].as_u64().map(ObjectId).unwrap();
    let doc = &ws.get(id).unwrap().session.document;
    let object = doc.get_object(new_id).unwrap();
    assert!(matches!(object.kind, ObjectKind::ArtText { .. }));
    run(&mut ws, id, "edit.paste_special", json!({})).unwrap();
    assert_eq!(count(&ws, id), 2);
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(count(&ws, id), 1);
}

#[test]
fn paste_special_rejects_unknown_format() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    ws.get_mut(id).unwrap().session.clipboard.set_text("hello".to_string());
    assert_eq!(run(&mut ws, id, "edit.paste_special", json!({ "format": "dtool" })), Err("INVALID_PARAMS".to_string()));
    assert_eq!(run(&mut ws, id, "edit.paste_special", json!({ "format": 3 })), Err("INVALID_PARAMS".to_string()));
    assert_eq!(count(&ws, id), 0);
}

#[test]
fn duplicate_drag_clones_group_children_with_undo_redo_and_power_duplicate() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let (group, child) = {
        let session = &mut ws.get_mut(id).unwrap().session;
        let group = session.document.create_object(Object { name: "g".into(), kind: ObjectKind::Group, size: [20.0, 20.0], ..Object::default() });
        let child = session.document.create_object(Object { name: "c".into(), parent_id: Some(group), pos: [1.0, 2.0], size: [5.0, 5.0], ..Object::default() });
        session.selection.set(vec![group], SelectOp::Replace);
        (group, child)
    };
    let reply = run(&mut ws, id, "edit.duplicate_drag", json!({ "offset": [10.0, 0.0] })).unwrap();
    assert_eq!(reply["offset"], json!([10.0, 0.0]));
    assert_eq!(count(&ws, id), 4);
    {
        let session = &ws.get(id).unwrap().session;
        let clone = session.selection.ids[0];
        assert_ne!(clone, group);
        let doc = &session.document;
        let kids: Vec<&Object> = doc.objects.iter().filter(|o| o.parent_id == Some(clone)).collect();
        assert_eq!(kids.len(), 1);
        assert_ne!(kids[0].id, child);
        assert_eq!(kids[0].pos, [11.0, 2.0]);
    }
    run(&mut ws, id, "edit.power_duplicate", json!({})).unwrap();
    assert_eq!(count(&ws, id), 6);
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(count(&ws, id), 2);
    assert_eq!(ws.get(id).unwrap().session.selection.ids, vec![group]);
    run(&mut ws, id, "edit.redo", json!({})).unwrap();
    assert_eq!(count(&ws, id), 4);
}