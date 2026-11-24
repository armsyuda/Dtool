use dtool_core::clipboard::{os_text_for_payload, take_os_text, ClipboardKind};
use dtool_core::selection::SelectOp;
use dtool_core::{Command, ContextId, ObjectId, ObjectKind, Scope, Workspace};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

#[test]
fn os_text_paste_makes_art_text_with_that_text() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    assert!(take_os_text(&mut ws.get_mut(id).unwrap().session, "abc\r\ndef"));
    let reply = run(&mut ws, id, "edit.paste", json!({}));
    let doc = &ws.get(id).unwrap().session.document;
    let object = reply.get("ids").and_then(|ids| ids.get(0)).and_then(Value::as_u64).map(ObjectId)
        .and_then(|oid| doc.get_object(oid))
        .or_else(|| doc.objects.last())
        .unwrap();
    let story = match &object.kind { ObjectKind::ArtText { story } => *story, other => panic!("not ArtText: {other:?}") };
    assert_eq!(doc.stories.iter().find(|s| s.id == story).unwrap().text, "abc\ndef");
}

#[test]
fn os_text_equal_to_own_object_copy_keeps_object_payload() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let text = run(&mut ws, id, "text.art.add", json!({ "text": "hello", "pos": [5.0, 5.0] }))["id"].as_u64().map(ObjectId).unwrap();
    ws.get_mut(id).unwrap().session.selection.set(vec![text], SelectOp::Replace);
    run(&mut ws, id, "edit.copy", json!({}));
    let session = &mut ws.get_mut(id).unwrap().session;
    let exported = os_text_for_payload(&session.clipboard.get_payload());
    assert_eq!(exported, "hello");
    assert!(!take_os_text(session, &exported));
    assert_eq!(session.clipboard.get_payload().kind, ClipboardKind::Objects);
    assert!(take_os_text(session, "other"));
    assert_eq!(session.clipboard.get_payload().kind, ClipboardKind::Text);
    assert!(!take_os_text(session, ""));
}