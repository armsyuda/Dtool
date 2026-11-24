use dtool_core::colour::swatch::{Swatch, SwatchBinding, SwatchTarget};
use dtool_core::paint::Colour;
use dtool_core::selection::SelectOp;
use dtool_core::text_link::TextLink;
use dtool_core::{CloseAnswer, Command, ContextId, Object, ObjectId, ObjectKind, Scope, Workspace};
use serde_json::{json, Value};
use std::path::PathBuf;

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

fn doc_json(ws: &Workspace, id: ContextId) -> Value {
    let mut value = serde_json::to_value(&ws.get(id).unwrap().session.document).unwrap();
    for key in ["version", "next_object_id", "next_story_id", "next_resource_id", "next_page_id"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value
}

fn assert_same(left: &Value, right: &Value) {
    let keys: Vec<(&String, &Value, &Value)> = left.as_object().unwrap().iter().filter_map(|(k, v)| (right.get(k) != Some(v)).then(|| (k, v, &right[k]))).collect();
    assert!(keys.is_empty(), "{keys:?}");
}

fn brand() -> Colour {
    Colour::new(200, 10, 30, 255)
}

fn linked_fixture(ws: &mut Workspace, swatch_id: u64) -> (ContextId, ObjectId, ObjectId) {
    let id = ws.new_document("a");
    let text = run(ws, id, "text.art.add", json!({ "text": "hello", "pos": [5.0, 5.0] }))["id"].as_u64().map(ObjectId).unwrap();
    let session = &mut ws.get_mut(id).unwrap().session;
    let rect = session.document.create_object(Object { name: "rect".into(), size: [10.0, 10.0], ..Object::default() });
    let story = session.document.get_object(text).and_then(|o| o.story_id()).unwrap();
    session.document.swatches.push(Swatch::new(swatch_id, "Brand", brand()));
    session.document.next_swatch_id = swatch_id + 1;
    session.document.swatch_bindings.push(SwatchBinding::paint(rect, SwatchTarget::Fill, swatch_id));
    session.document.text_links.push(TextLink { story, path: PathBuf::from("C:/docs/hello.txt"), mtime: Some(100), hash: "abc".into() });
    session.selection.set(vec![rect, text], SelectOp::Replace);
    (id, rect, text)
}

fn new_ids(ws: &Workspace, id: ContextId, old: &[ObjectId]) -> (ObjectId, ObjectId) {
    let doc = &ws.get(id).unwrap().session.document;
    let rect = doc.objects.iter().find(|o| o.name == "rect" && !old.contains(&o.id)).unwrap().id;
    let text = doc.objects.iter().find(|o| matches!(o.kind, ObjectKind::ArtText { .. }) && !old.contains(&o.id)).unwrap().id;
    (rect, text)
}

#[test]
fn duplicate_carries_swatch_binding_and_text_link_to_the_clones_with_exact_undo() {
    let mut ws = Workspace::default();
    let (a, rect, text) = linked_fixture(&mut ws, 3);
    let before = doc_json(&ws, a);
    run(&mut ws, a, "edit.duplicate", json!({}));
    let (rect2, text2) = new_ids(&ws, a, &[rect, text]);
    {
        let doc = &ws.get(a).unwrap().session.document;
        assert!(doc.swatch_bindings.iter().any(|b| b.object == rect2 && b.swatch == 3 && b.target == SwatchTarget::Fill));
        assert!(doc.swatch_bindings.iter().any(|b| b.object == rect && b.swatch == 3));
        let story2 = doc.get_object(text2).and_then(|o| o.story_id()).unwrap();
        let story1 = doc.get_object(text).and_then(|o| o.story_id()).unwrap();
        assert_ne!(story1, story2);
        assert_eq!(doc.text_links.len(), 2);
        assert_eq!(doc.text_links.iter().find(|l| l.story == story2).unwrap().path, PathBuf::from("C:/docs/hello.txt"));
        assert_eq!(doc.swatches.len(), 1);
    }
    run(&mut ws, a, "edit.undo", json!({}));
    assert_same(&doc_json(&ws, a), &before);
}

#[test]
fn cross_document_paste_maps_binding_to_the_matching_swatch_and_drops_it_otherwise() {
    let mut ws = Workspace::default();
    let (a, _, _) = linked_fixture(&mut ws, 3);
    run(&mut ws, a, "edit.copy", json!({}));
    assert!(ws.close_with(a, CloseAnswer::No).unwrap());
    let b = ws.new_document("b");
    ws.get_mut(b).unwrap().session.document.swatches.push(Swatch::new(9, "Brand", brand()));
    ws.get_mut(b).unwrap().session.document.next_swatch_id = 10;
    let before_b = doc_json(&ws, b);
    run(&mut ws, b, "edit.paste", json!({}));
    {
        let doc = &ws.get(b).unwrap().session.document;
        let rect = doc.objects.iter().find(|o| o.name == "rect").unwrap().id;
        assert_eq!(doc.swatch_bindings, vec![SwatchBinding::paint(rect, SwatchTarget::Fill, 9)]);
        assert_eq!(doc.swatches.len(), 1);
        let story = doc.objects.iter().find(|o| matches!(o.kind, ObjectKind::ArtText { .. })).and_then(|o| o.story_id()).unwrap();
        assert_eq!(doc.text_links.len(), 1);
        assert_eq!(doc.text_links[0].story, story);
        assert!(doc.validate_refs().is_ok());
    }
    run(&mut ws, b, "edit.undo", json!({}));
    assert_same(&doc_json(&ws, b), &before_b);
    let c = ws.new_document("c");
    let before_c = doc_json(&ws, c);
    run(&mut ws, c, "edit.paste", json!({}));
    {
        let doc = &ws.get(c).unwrap().session.document;
        assert_eq!(doc.objects.len(), 2);
        assert!(doc.swatch_bindings.is_empty());
        assert!(doc.swatches.is_empty());
        assert_eq!(doc.objects.iter().find(|o| o.name == "rect").unwrap().fill, ws.app_clipboard().get_payload().bundle.as_ref().unwrap().objects.iter().find(|o| o.name == "rect").unwrap().fill);
    }
    run(&mut ws, c, "edit.undo", json!({}));
    assert_same(&doc_json(&ws, c), &before_c);
}

#[test]
fn cut_drops_the_cut_objects_bindings_and_undo_restores_them() {
    let mut ws = Workspace::default();
    let (a, rect, _) = linked_fixture(&mut ws, 3);
    let before = doc_json(&ws, a);
    run(&mut ws, a, "edit.cut", json!({}));
    {
        let doc = &ws.get(a).unwrap().session.document;
        assert!(doc.get_object(rect).is_none());
        assert!(doc.swatch_bindings.iter().all(|b| b.object != rect));
    }
    run(&mut ws, a, "edit.undo", json!({}));
    assert_same(&doc_json(&ws, a), &before);
}