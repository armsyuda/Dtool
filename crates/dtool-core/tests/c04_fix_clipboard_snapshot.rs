use dtool_core::colour::swatch::{SwatchBinding, SwatchTarget};
use dtool_core::commands::arrange::{commit_snapshot, snapshot, NodeSnapshot};
use dtool_core::document::Story;
use dtool_core::resources::{ImageFormat, ImageResource, PlacedImage};
use dtool_core::selection::SelectOp;
use dtool_core::text_link::TextLink;
use dtool_core::{Command, ContextId, Object, ObjectId, ObjectKind, Scope, Session, StoryId, Workspace};
use serde_json::{json, Value};
use std::path::PathBuf;

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

fn commit_edit(session: &mut Session, name: &str, before: NodeSnapshot) {
    commit_snapshot(session, name, before, json!({})).unwrap();
}

fn push_story_and_resource(session: &mut Session, text: &str, bytes: Vec<u8>) {
    let story_id = session.document.next_story_id;
    session.document.next_story_id = story_id + 1;
    session.document.stories.push(Story {
        id: StoryId(story_id),
        text: text.into(),
        ranges: Vec::new(),
        specials: Vec::new(),
        paras: Vec::new(),
    });
    let mut resource = ImageResource::new_embedded(ImageFormat::Png, [2, 1], 72.0, bytes);
    resource.id = session.document.alloc_resource_id();
    session.document.resources.push(resource);
    session.document.create_object(Object { name: text.into(), size: [4.0, 4.0], ..Object::default() });
}

#[test]
fn duplicate_and_cross_doc_paste_undo_restore_stories_and_resources_exactly() {
    let mut ws = Workspace::default();
    let (id, _, _, _) = rich_fixture(&mut ws);
    let before = doc_json(&ws, id);
    {
        let session = &mut ws.get_mut(id).unwrap().session;
        let snap = snapshot(session);
        push_story_and_resource(session, "dup", vec![9, 8, 7, 6]);
        commit_edit(session, "edit.duplicate", snap);
    }
    run(&mut ws, id, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, id), before);
    let b = ws.new_document("b");
    let before_b = doc_json(&ws, b);
    {
        let session = &mut ws.get_mut(b).unwrap().session;
        let snap = snapshot(session);
        push_story_and_resource(session, "pasted", vec![1, 2, 3, 4]);
        commit_edit(session, "edit.paste", snap);
    }
    run(&mut ws, b, "edit.undo", json!({}));
    assert_eq!(doc_json(&ws, b), before_b);
    let doc = &ws.get(b).unwrap().session.document;
    assert!(doc.next_story_id >= 2 && doc.next_resource_id >= 2);
}

#[test]
fn undo_restores_swatch_bindings_and_text_links() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let text = run(&mut ws, id, "text.art.add", json!({ "text": "hello", "pos": [5.0, 5.0] }))["id"].as_u64().map(ObjectId).unwrap();
    let (story, binding, link) = {
        let session = &mut ws.get_mut(id).unwrap().session;
        let story = session.document.get_object(text).and_then(|obj| obj.story_id()).unwrap();
        let binding = SwatchBinding {
            object: text,
            target: SwatchTarget::Fill,
            swatch: 7,
            stop: None,
            story: Some(story),
            range: None,
            range_id: None,
        };
        let link = TextLink {
            story,
            path: PathBuf::from("C:/docs/hello.txt"),
            mtime: Some(100),
            hash: "abc".into(),
        };
        session.document.swatch_bindings.push(binding);
        session.document.text_links.push(link.clone());
        let snap = snapshot(session);
        session.document.swatch_bindings.clear();
        session.document.text_links.clear();
        session.document.stories.clear();
        commit_edit(session, "edit.cut", snap);
        (story, binding, link)
    };
    let mid = doc_json(&ws, id);
    assert!(ws.get(id).unwrap().session.document.swatch_bindings.is_empty());
    assert!(ws.get(id).unwrap().session.document.text_links.is_empty());
    assert!(ws.get(id).unwrap().session.document.stories.is_empty());
    run(&mut ws, id, "edit.undo", json!({}));
    {
        let doc = &ws.get(id).unwrap().session.document;
        assert_eq!(doc.swatch_bindings, vec![binding]);
        assert_eq!(doc.text_links, vec![link]);
        assert!(doc.stories.iter().any(|item| item.id == story && item.text == "hello"));
    }
    run(&mut ws, id, "edit.redo", json!({}));
    assert_eq!(doc_json(&ws, id), mid);
    assert!(ws.get(id).unwrap().session.document.swatch_bindings.is_empty());
    assert!(ws.get(id).unwrap().session.document.text_links.is_empty());
}

#[test]
fn old_snapshot_without_link_fields_leaves_bindings_in_place() {
    let mut session = Session::new(dtool_core::Document::new([100.0, 80.0], dtool_core::document::Units::Px, 72.0, 1));
    let object = session.document.create_object(Object { name: "o".into(), size: [10.0, 10.0], ..Object::default() });
    session.document.swatch_bindings.push(SwatchBinding {
        object,
        target: SwatchTarget::Fill,
        swatch: 3,
        stop: None,
        story: None,
        range: None,
        range_id: None,
    });
    session.document.text_links.push(TextLink {
        story: StoryId(1),
        path: PathBuf::from("keep.txt"),
        mtime: None,
        hash: String::new(),
    });
    let raw = json!({
        "objects": session.document.objects,
        "next_object_id": session.document.next_object_id,
        "selection": []
    });
    let snap: NodeSnapshot = serde_json::from_value(raw).unwrap();
    assert!(snap.swatch_bindings.is_none() && snap.text_links.is_none());
    dtool_core::commands::arrange::restore(&mut session, &snap);
    assert_eq!(session.document.swatch_bindings.len(), 1);
    assert_eq!(session.document.text_links.len(), 1);
}
