use dtool_core::resources::{ImageFormat, ImageResource, PlacedImage};
use dtool_core::selection::SelectOp;
use dtool_core::{CloseAnswer, Command, ContextId, Object, ObjectId, ObjectKind, Scope, Workspace};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::of(id), Command { name: name.into(), params });
    result.unwrap_or_else(|e| panic!("{name} failed: {}", e.code))
}

fn doc_json(ws: &Workspace, id: ContextId) -> Value {
    let mut value = serde_json::to_value(&ws.get(id).unwrap().session.document).unwrap();
    value.as_object_mut().unwrap().remove("version");
    for key in ["next_object_id", "next_story_id", "next_resource_id", "next_page_id"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value
}

fn select(ws: &mut Workspace, id: ContextId, ids: Vec<ObjectId>) {
    ws.get_mut(id).unwrap().session.selection.set(ids, SelectOp::Replace);
}

fn rich_fixture(ws: &mut Workspace) -> (ContextId, ObjectId, ObjectId, ObjectId) {
    let id = ws.new_document("a");
    let text = run(ws, id, "text.art.add", json!({ "text": "hello", "pos": [5.0, 5.0] }))["id"].as_u64().map(ObjectId).unwrap();
    let session = &mut ws.get_mut(id).unwrap().session;
    let group = session.document.create_object(Object { name: "group".into(), kind: ObjectKind::Group, size: [50.0, 50.0], ..Object::default() });
    let image = session.document.create_object(Object { name: "image".into(), parent_id: Some(group), size: [20.0, 10.0], ..Object::default() });
    session.document.find_object_mut(text.0).unwrap().parent_id = Some(group);
    let mut resource = ImageResource::new_embedded(ImageFormat::Png, [2, 1], 72.0, vec![1, 2, 3, 4]);
    resource.id = session.document.alloc_resource_id();
    let rid = resource.id;
    session.document.resources.push(resource);
    session.document.placed_images.push(PlacedImage { object_id: image, resource: rid });
    session.selection.set(vec![group], SelectOp::Replace);
    (id, group, image, text)
}

#[test]
fn cut_of_group_undo_restores_children_story_and_image_then_redo() {
    let mut ws = Workspace::default();
    let (id, group, image, text) = rich_fixture(&mut ws);
    let before = doc_json(&ws, id);
    run(&mut ws, id, "edit.cut", json!({}));
    {
        let doc = &ws.get(id).unwrap().session.document;
        assert!(doc.get_object(group).is_none() && doc.get_object(image).is_none() && doc.get_object(text).is_none());
        assert!(doc.placed_images.is_empty());
        assert!(doc.validate_refs().is_ok());
    }
    let after = doc_json(&ws, id);
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
    assert_eq!(ws.get(id).unwrap().session.selection.ids, vec![group]);
    run(&mut ws, id, "edit.redo", json!({}));
    assert_eq!(doc_json(&ws, id), after);
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
}

#[test]
fn paste_of_two_objects_is_one_undo_step_with_redo() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let session = &mut ws.get_mut(id).unwrap().session;
    let a = session.document.create_object(Object { name: "a".into(), size: [5.0, 5.0], ..Object::default() });
    let b = session.document.create_object(Object { name: "b".into(), pos: [20.0, 0.0], size: [5.0, 5.0], ..Object::default() });
    session.selection.set(vec![a, b], SelectOp::Replace);
    run(&mut ws, id, "edit.copy", json!({}));
    let before = doc_json(&ws, id);
    let pasted = run(&mut ws, id, "edit.paste", json!({ "at": [100.0, 100.0] }));
    let ids: Vec<u64> = pasted["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    let after = doc_json(&ws, id);
    {
        let doc = &ws.get(id).unwrap().session.document;
        assert_eq!(doc.get_object(ObjectId(ids[0])).unwrap().pos, [100.0, 100.0]);
        assert_eq!(doc.get_object(ObjectId(ids[1])).unwrap().pos, [120.0, 100.0]);
        assert_eq!(ws.get(id).unwrap().session.selection.ids, vec![ObjectId(ids[0]), ObjectId(ids[1])]);
    }
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
    ws.get_mut(id).unwrap().session.clipboard.set_text("other".into());
    run(&mut ws, id, "edit.redo", json!({}));
    assert_eq!(doc_json(&ws, id), after);
}

#[test]
fn cross_document_paste_after_closing_source_keeps_image_and_text() {
    let mut ws = Workspace::default();
    let (a, _, _, _) = rich_fixture(&mut ws);
    run(&mut ws, a, "edit.copy", json!({}));
    let b = ws.new_document("b");
    let other = ws.get_mut(b).unwrap().session.document.create_object(Object { name: "keep".into(), size: [1.0, 1.0], ..Object::default() });
    select(&mut ws, b, vec![other]);
    assert!(ws.close_with(a, CloseAnswer::No).unwrap());
    assert!(ws.get(a).is_none());
    let before = doc_json(&ws, b);
    run(&mut ws, b, "edit.paste", json!({}));
    {
        let doc = &ws.get(b).unwrap().session.document;
        assert!(doc.validate_refs().is_ok());
        assert_eq!(doc.objects.len(), 4);
        let text = doc.objects.iter().find(|o| matches!(o.kind, ObjectKind::ArtText { .. })).unwrap();
        let story = text.story_id().unwrap();
        assert_eq!(doc.stories.iter().find(|s| s.id == story).unwrap().text, "hello");
        let image = doc.objects.iter().find(|o| o.name == "image").unwrap();
        let placed = doc.placed_images.iter().find(|p| p.object_id == image.id).unwrap();
        assert_eq!(doc.resources.iter().find(|r| r.id == placed.resource).unwrap().data, Some(vec![1, 2, 3, 4]));
        let group = doc.objects.iter().find(|o| o.name == "group").unwrap();
        assert_eq!(image.parent_id, Some(group.id));
        assert_eq!(text.parent_id, Some(group.id));
        assert_eq!(group.parent_id, None);
    }
    run(&mut ws, b, "edit.undo", json!({}));
    let doc = &ws.get(b).unwrap().session.document;
    assert_eq!(doc.objects.len(), 1);
    assert!(doc.get_object(other).is_some());
    assert!(doc.placed_images.is_empty());
    let _ = before;
}

#[test]
fn duplicate_copies_children_selects_clones_and_undoes() {
    let mut ws = Workspace::default();
    let (id, group, _, _) = rich_fixture(&mut ws);
    let before = doc_json(&ws, id);
    let out = run(&mut ws, id, "edit.duplicate", json!({}));
    let clone = ObjectId(out["ids"][0].as_u64().unwrap());
    assert_ne!(clone, group);
    {
        let session = &ws.get(id).unwrap().session;
        assert_eq!(session.selection.ids, vec![clone]);
        assert_eq!(session.document.objects.iter().filter(|o| o.parent_id == Some(clone)).count(), 2);
        assert_eq!(session.document.placed_images.len(), 2);
        assert_eq!(session.document.stories.len(), 2);
        assert!(session.document.validate_refs().is_ok());
    }
    let after = doc_json(&ws, id);
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id)["objects"], before["objects"]);
    assert_eq!(doc_json(&ws, id)["placed_images"], before["placed_images"]);
    assert_eq!(ws.get(id).unwrap().session.selection.ids, vec![group]);
    run(&mut ws, id, "edit.redo", json!({}));
    assert_eq!(doc_json(&ws, id)["objects"], after["objects"]);
    assert_eq!(doc_json(&ws, id)["stories"], after["stories"]);
}

#[test]
fn duplicate_and_cross_doc_paste_undo_restore_stories_and_resources_exactly() {
    let mut ws = Workspace::default();
    let (id, _, _, _) = rich_fixture(&mut ws);
    let before = doc_json(&ws, id);
    run(&mut ws, id, "edit.duplicate", json!({}));
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
    run(&mut ws, id, "edit.copy", json!({}));
    let b = ws.new_document("b");
    let before_b = doc_json(&ws, b);
    run(&mut ws, b, "edit.paste", json!({}));
    run(&mut ws, b, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, b), before_b);
    let doc = &ws.get(b).unwrap().session.document;
    assert!(doc.next_story_id >= 2 && doc.next_resource_id >= 2);
}

#[test]
fn plain_text_paste_makes_art_text_and_undoes() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    ws.get_mut(id).unwrap().session.clipboard.set_text("pasted words".into());
    let out = run(&mut ws, id, "edit.paste", json!({ "at": [3.0, 4.0] }));
    let new_id = ObjectId(out["ids"][0].as_u64().unwrap());
    {
        let session = &ws.get(id).unwrap().session;
        let object = session.document.get_object(new_id).unwrap();
        let story = object.story_id().expect("art text");
        assert!(matches!(object.kind, ObjectKind::ArtText { .. }));
        assert_eq!(object.pos, [3.0, 4.0]);
        assert_eq!(session.document.stories.iter().find(|s| s.id == story).unwrap().text, "pasted words");
        assert_eq!(session.selection.ids, vec![new_id]);
    }
    run(&mut ws, id, "edit.undo", json!({}));
    assert!(ws.get(id).unwrap().session.document.objects.is_empty());
    assert!(ws.get(id).unwrap().session.document.stories.is_empty());
}

#[test]
fn paste_style_is_strict_and_copies_stroke() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let session = &mut ws.get_mut(id).unwrap().session;
    let mut source = Object { name: "src".into(), size: [5.0, 5.0], fill: dtool_core::paint::paint_from_param(&json!("#336699")).unwrap(), opacity: 0.25, ..Object::default() };
    source.stroke.width = 3.0;
    let src = session.document.create_object(source);
    let dst = session.document.create_object(Object { name: "dst".into(), size: [5.0, 5.0], ..Object::default() });
    session.selection.set(vec![src], SelectOp::Replace);
    run(&mut ws, id, "edit.copy", json!({}));
    select(&mut ws, id, vec![dst]);
    run(&mut ws, id, "edit.paste_style", json!({}));
    {
        let doc = &ws.get(id).unwrap().session.document;
        let (s, d) = (doc.get_object(src).unwrap(), doc.get_object(dst).unwrap());
        assert_eq!((d.fill.clone(), d.stroke.clone(), d.opacity), (s.fill.clone(), s.stroke.clone(), s.opacity));
    }
    ws.get_mut(id).unwrap().session.clipboard.set_style(json!({ "fill": { "type": "bogus" } }));
    let before = doc_json(&ws, id);
    let (_, result) = ws.run(Scope::of(id), Command { name: "edit.paste_style".into(), params: json!({}) });
    assert!(result.is_err());
    assert_eq!(doc_json(&ws, id), before);
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(ws.get(id).unwrap().session.document.get_object(dst).unwrap().opacity, 1.0);
}