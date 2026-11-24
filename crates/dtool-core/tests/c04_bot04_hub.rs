use dtool_core::clipboard::new_document_from_clipboard;
use dtool_core::selection::SelectOp;
use dtool_core::{Command, ContextId, Object, ObjectId, ObjectKind, Scope, Workspace};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

fn obj(ws: &Workspace, id: ContextId, object: ObjectId) -> Object {
    ws.get(id).unwrap().session.document.get_object(object).unwrap().clone()
}

fn centre(o: &Object) -> [f32; 2] {
    [o.pos[0] + o.size[0] * 0.5, o.pos[1] + o.size[1] * 0.5]
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

fn duplicated(ws: &mut Workspace) -> (ContextId, ObjectId, ObjectId) {
    let id = ws.new_document("a");
    let source = {
        let session = &mut ws.get_mut(id).unwrap().session;
        let source = session.document.create_object(Object { name: "box".into(), size: [10.0, 10.0], pos: [0.0, 0.0], ..Object::default() });
        session.selection.set(vec![source], SelectOp::Replace);
        source
    };
    run(ws, id, "edit.duplicate", json!({}));
    let clone = ws.get(id).unwrap().session.last_duplicate_ids[0];
    (id, source, clone)
}

#[test]
fn power_duplicate_repeats_rotate_scale_and_move_of_a_selection_box_drag() {
    let mut ws = Workspace::default();
    let (id, source, clone) = duplicated(&mut ws);
    run(&mut ws, id, "transform.selection_box", json!({ "origin": [5.0, 5.0], "translate": [20.0, 0.0], "scale": [1.5, 1.5], "rotation_deg": 15.0 }));
    let first = obj(&ws, id, clone);
    assert_eq!(first.rotation_deg, 15.0);
    assert!(close(centre(&first), [25.0, 5.0]));
    run(&mut ws, id, "edit.power_duplicate", json!({}));
    let next_id = ws.get(id).unwrap().session.last_duplicate_ids[0];
    assert_ne!(next_id, clone);
    let next = obj(&ws, id, next_id);
    assert_eq!(next.rotation_deg, 30.0);
    assert!(close(next.size, [22.5, 22.5]));
    assert!(close(centre(&next), [45.0, 5.0]), "{:?}", centre(&next));
    assert_eq!(serde_json::to_value(obj(&ws, id, clone)).unwrap(), serde_json::to_value(&first).unwrap());
    run(&mut ws, id, "edit.power_duplicate", json!({}));
    let third = obj(&ws, id, ws.get(id).unwrap().session.last_duplicate_ids[0]);
    assert_eq!(third.rotation_deg, 45.0);
    assert!(close(centre(&third), [65.0, 5.0]));
    run(&mut ws, id, "edit.undo", json!({}));
    let doc = &ws.get(id).unwrap().session.document;
    assert_eq!(doc.objects.len(), 3);
    assert!(doc.get_object(next_id).is_some() && doc.get_object(source).is_some());
}

#[test]
fn power_duplicate_repeats_a_transform_set_rotation_and_move() {
    let mut ws = Workspace::default();
    let (id, _, clone) = duplicated(&mut ws);
    run(&mut ws, id, "transform.set", json!({ "ids": [clone.0], "x": 12.0, "rotation_deg": 10.0 }));
    let first = obj(&ws, id, clone);
    run(&mut ws, id, "edit.power_duplicate", json!({}));
    let next = obj(&ws, id, ws.get(id).unwrap().session.last_duplicate_ids[0]);
    assert_eq!(next.rotation_deg, first.rotation_deg + 10.0);
    assert!(close(centre(&next), [centre(&first)[0] + 12.0, centre(&first)[1]]), "{:?} {:?}", centre(&first), centre(&next));
}

#[test]
fn power_duplicate_without_a_recorded_step_is_refused_and_changes_nothing() {
    let mut ws = Workspace::default();
    let (id, _, _) = duplicated(&mut ws);
    let before = ws.get(id).unwrap().session.document.objects.len();
    let (_, result) = ws.run(Scope::of(id), Command { name: "edit.power_duplicate".into(), params: json!({}) });
    assert!(result.is_err());
    assert_eq!(ws.get(id).unwrap().session.document.objects.len(), before);
}

#[test]
fn new_from_clipboard_text_makes_an_art_text_document() {
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    ws.get_mut(a).unwrap().session.clipboard.set_text("hello".into());
    let b = new_document_from_clipboard(&mut ws).unwrap();
    let doc = &ws.get(b).unwrap().session.document;
    let text = doc.objects.iter().find(|o| matches!(o.kind, ObjectKind::ArtText { .. })).unwrap();
    assert_eq!(doc.stories.iter().find(|s| Some(s.id) == text.story_id()).unwrap().text, "hello");
    assert!(!ws.get(b).unwrap().session.undo.can_undo());
}

#[test]
fn new_from_clipboard_with_empty_clipboard_is_refused_without_a_tab() {
    let mut ws = Workspace::default();
    let _a = ws.new_document("a");
    let count = ws.contexts().len();
    assert!(new_document_from_clipboard(&mut ws).is_err());
    assert_eq!(ws.contexts().len(), count);
}

#[test]
fn doc_new_from_clipboard_command_routes_through_the_workspace() {
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    ws.get_mut(a).unwrap().session.clipboard.set_text("hi".into());
    let count = ws.contexts().len();
    let (target, result) = ws.run(Scope::of(a), Command { name: "doc.new_from_clipboard".into(), params: json!({}) });
    let reply = result.unwrap();
    assert_eq!(ws.contexts().len(), count + 1);
    assert_eq!(target.map(|c| c.0), reply["context"].as_u64());
}

#[test]
fn paste_without_format_makes_art_text_with_one_undo() {
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    ws.get_mut(a).unwrap().session.clipboard.set_text("plain".into());
    let reply = run(&mut ws, a, "edit.paste_without_format", json!({ "at": [4.0, 6.0] }));
    let id = ObjectId(reply["ids"][0].as_u64().unwrap());
    let o = obj(&ws, a, id);
    assert!(matches!(o.kind, ObjectKind::ArtText { .. }));
    assert_eq!(ws.get(a).unwrap().session.selection.ids, vec![id]);
    run(&mut ws, a, "edit.undo", json!({}));
    let doc = &ws.get(a).unwrap().session.document;
    assert!(doc.get_object(id).is_none());
    assert!(doc.stories.is_empty());
}
#[test]
fn power_duplicate_repeats_shear_and_is_one_undo_with_redo() {
    let mut ws = Workspace::default();
    let (id, _, clone) = duplicated(&mut ws);
    run(&mut ws, id, "transform.selection_box", json!({ "origin": [5.0, 5.0], "shear_deg": 8.0 }));
    let first = obj(&ws, id, clone);
    run(&mut ws, id, "edit.power_duplicate", json!({}));
    let next_id = ws.get(id).unwrap().session.last_duplicate_ids[0];
    let next = obj(&ws, id, next_id);
    assert_eq!(first.shear_deg, 8.0);
    assert_eq!(next.shear_deg, 16.0);
    run(&mut ws, id, "edit.undo", json!({}));
    assert!(ws.get(id).unwrap().session.document.get_object(next_id).is_none());
    assert_eq!(obj(&ws, id, clone).shear_deg, 8.0);
    run(&mut ws, id, "edit.redo", json!({}));
    assert_eq!(serde_json::to_value(obj(&ws, id, next_id)).unwrap(), serde_json::to_value(next).unwrap());
}
