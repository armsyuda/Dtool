use dtool_core::undo::Change;
use dtool_core::{Command, ContextId, Object, ObjectId, ObjectKind, Scope, Workspace};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

fn doc_json(ws: &Workspace, id: ContextId) -> Value {
    let mut value = serde_json::to_value(&ws.get(id).unwrap().session.document).unwrap();
    value.as_object_mut().unwrap().remove("version");
    value
}

fn art(ws: &mut Workspace, id: ContextId, text: &str) -> ObjectId {
    run(ws, id, "text.art.add", json!({ "text": text, "pos": [5.0, 5.0] }))["id"].as_u64().map(ObjectId).unwrap()
}

fn is_shape(ws: &Workspace, id: ContextId, object: ObjectId) -> bool {
    matches!(ws.get(id).unwrap().session.document.get_object(object).unwrap().kind, ObjectKind::Curve(_))
}

#[test]
fn undo_entry_holding_edit_batch_applies_every_command() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let _ = dtool_core::text_lines::register_text_outlines(|_, _| Some(vec![vec![[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]]));
    let a = art(&mut ws, id, "one");
    let b = art(&mut ws, id, "two");
    let batch = Command { name: "edit.batch".into(), params: json!({ "commands": [
        { "name": "text.to_path", "params": { "id": a.0 } },
        { "name": "text.to_path", "params": { "id": b.0 } }
    ] }) };
    ws.get_mut(id).unwrap().session.undo.push(Change { name: "test".into(), redo: batch.clone(), undo: batch });
    run(&mut ws, id, "edit.undo", json!({}));
    assert!(is_shape(&ws, id, a) && is_shape(&ws, id, b));
}

#[test]
fn failing_edit_batch_inside_undo_rolls_back_whole_batch() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let _ = dtool_core::text_lines::register_text_outlines(|_, _| Some(vec![vec![[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]]));
    let a = art(&mut ws, id, "one");
    let before = doc_json(&ws, id);
    let batch = Command { name: "edit.batch".into(), params: json!({ "commands": [
        { "name": "text.to_path", "params": { "id": a.0 } },
        { "name": "text.to_path", "params": { "id": 999_999 } }
    ] }) };
    ws.get_mut(id).unwrap().session.undo.push(Change { name: "test".into(), redo: batch.clone(), undo: batch });
    let (_, result) = ws.run(Scope::of(id), Command { name: "edit.undo".into(), params: json!({}) });
    assert!(result.is_err());
    assert_eq!(doc_json(&ws, id), before);
}

#[test]
fn to_path_undo_keeps_z_order_and_exact_document() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let _ = dtool_core::text_lines::register_text_outlines(|_, _| Some(vec![vec![[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]]));
    let a = art(&mut ws, id, "one");
    let _b = art(&mut ws, id, "two");
    let _c = ws.get_mut(id).unwrap().session.document.create_object(Object { name: "c".into(), size: [4.0, 4.0], ..Object::default() });
    let before = doc_json(&ws, id);
    run(&mut ws, id, "text.to_path", json!({ "id": a.0 }));
    let after = doc_json(&ws, id);
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
    run(&mut ws, id, "edit.redo", json!({}));
    assert_eq!(doc_json(&ws, id), after);
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
}

#[test]
fn to_path_keeps_story_shared_with_other_frames() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let _ = dtool_core::text_lines::register_text_outlines(|_, _| Some(vec![vec![[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]]));
    let a = art(&mut ws, id, "shared");
    let b = art(&mut ws, id, "other");
    let story = match ws.get(id).unwrap().session.document.get_object(a).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    ws.get_mut(id).unwrap().session.document.find_object_mut(b.0).unwrap().kind = ObjectKind::ArtText { story };
    let before = doc_json(&ws, id);
    run(&mut ws, id, "text.to_path", json!({ "id": a.0 }));
    {
        let doc = &ws.get(id).unwrap().session.document;
        assert!(doc.stories.iter().any(|s| s.id == story));
        assert!(doc.validate_refs().is_ok());
    }
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
}