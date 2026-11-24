use dtool_core::{Command, ContextId, Object, ObjectId, Scope, Workspace};
use dtool_core::selection::SelectOp;
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

fn pos(ws: &Workspace, id: ContextId, object: ObjectId) -> [f32; 2] {
    ws.get(id).unwrap().session.document.get_object(object).unwrap().pos
}

#[test]
fn power_duplicate_repeats_selection_box_drag_of_the_clone() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let source = {
        let session = &mut ws.get_mut(id).unwrap().session;
        let source = session.document.create_object(Object { name: "box".into(), size: [10.0, 10.0], pos: [0.0, 0.0], ..Object::default() });
        session.selection.set(vec![source], SelectOp::Replace);
        source
    };
    run(&mut ws, id, "edit.duplicate", json!({ "offset": [0.0, 0.0] }));
    let clone = ws.get(id).unwrap().session.last_duplicate_ids[0];
    assert_ne!(clone, source);
    run(&mut ws, id, "transform.selection_box", json!({ "origin": [0.0, 0.0], "translate": [15.0, 5.0] }));
    assert_eq!(pos(&ws, id, clone), [15.0, 5.0]);
    run(&mut ws, id, "edit.power_duplicate", json!({}));
    let next = ws.get(id).unwrap().session.last_duplicate_ids[0];
    assert_ne!(next, clone);
    assert_eq!(pos(&ws, id, next), [30.0, 10.0]);
    run(&mut ws, id, "edit.undo", json!({}));
    let doc = &ws.get(id).unwrap().session.document;
    assert!(doc.get_object(next).is_none() && doc.get_object(clone).is_some() && doc.get_object(source).is_some());
}