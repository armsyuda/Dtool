use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::resources::{ImageResource, ResourceId};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use serde_json::json;

fn story(id: u64, text: &str) -> Story {
    Story {
        id: StoryId(id),
        text: text.into(),
        ranges: Vec::new(),
        specials: Vec::new(),
        paras: Vec::new(),
    }
}

fn base_doc() -> Document {
    let mut doc = Document::new([200.0, 200.0], Units::Pt, 72.0, 1);
    let sid = doc.alloc_story_id();
    doc.stories.push(story(sid.0, "host"));
    doc.create_object(Object {
        kind: ObjectKind::FrameText { story: sid },
        page_id: Some(doc.pages[0].id),
        ..Object::default()
    });
    doc.add_image_resource(ImageResource {
        name: "host-res".into(),
        ..ImageResource::default()
    });
    doc.stories.clear();
    doc.resources.clear();
    doc.objects.clear();
    doc.raise_id_high_water();
    doc
}

#[test]
fn merge_uses_stored_story_and_resource_high_water_not_live_len() {
    let mut host = base_doc();
    assert!(host.next_story_id > 1);
    assert!(host.next_resource_id > 1);
    assert!(host.stories.is_empty());
    assert!(host.resources.is_empty());
    let reserved_story = host.next_story_id;
    let reserved_resource = host.next_resource_id;

    let mut other = Document::new([200.0, 200.0], Units::Pt, 72.0, 1);
    let sid = other.alloc_story_id();
    other.stories.push(story(sid.0, "incoming"));
    other.create_object(Object {
        kind: ObjectKind::ArtText { story: sid },
        page_id: Some(other.pages[0].id),
        ..Object::default()
    });
    let rid = other.add_image_resource(ImageResource {
        name: "incoming-res".into(),
        data: Some(vec![1, 2, 3]),
        ..ImageResource::default()
    });
    other.place_image(other.objects[0].id, rid).unwrap();

    host.merge_document(other, 1);
    assert!(host.validate_refs().is_ok());
    let merged_story = host.stories.iter().find(|s| s.text == "incoming").unwrap();
    assert!(merged_story.id.0 >= reserved_story, "retired story ids must not be reused");
    let merged_res = host.resources.iter().find(|r| r.name == "incoming-res").unwrap();
    assert!(merged_res.id.0 >= reserved_resource, "retired resource ids must not be reused");
    let obj = host.objects.iter().find(|o| matches!(o.kind, ObjectKind::ArtText { .. })).unwrap();
    match obj.kind {
        ObjectKind::ArtText { story } => assert_eq!(story, merged_story.id),
        _ => unreachable!(),
    }
    assert_eq!(host.placed_resource(obj.id), Some(merged_res.id));
    assert!(host.next_story_id > merged_story.id.0);
    assert!(host.next_resource_id > merged_res.id.0);
    let page_ids: Vec<u64> = host.pages.iter().map(|p| p.id.0).collect();
    let mut unique = page_ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), page_ids.len(), "merged page ids must stay unique");
    assert!(obj.page_id.is_some());
    assert!(host.pages.iter().any(|p| Some(p.id) == obj.page_id));
}

#[test]
fn merge_undo_does_not_lower_high_water() {
    let mut workspace = Workspace::default();
    let ctx = workspace.new_document("k01-merge");
    {
        let doc = &mut workspace.get_mut(ctx).unwrap().session.document;
        *doc = base_doc();
        let reserved = (doc.next_object_id, doc.next_story_id, doc.next_resource_id, doc.next_page_id);
        doc.create_object(Object::default());
        doc.alloc_story_id();
        doc.add_image_resource(Default::default());
        assert!(doc.next_object_id >= reserved.0);
        assert!(doc.next_story_id >= reserved.1);
        assert!(doc.next_resource_id >= reserved.2);
    }
    let before = {
        let doc = &workspace.get(ctx).unwrap().session.document;
        (doc.next_object_id, doc.next_story_id, doc.next_resource_id, doc.next_page_id)
    };
    let path = std::env::temp_dir().join(format!("k01_merge_{}.dtool", std::process::id()));
    let mut other = Document::new([200.0, 200.0], Units::Pt, 72.0, 1);
    let sid = other.alloc_story_id();
    other.stories.push(story(sid.0, "incoming"));
    other.create_object(Object {
        kind: ObjectKind::FrameText { story: sid },
        ..Object::default()
    });
    std::fs::write(&path, serde_json::to_string(&other).unwrap()).unwrap();
    let (_, result) = workspace.run(
        Scope::of(ctx),
        Command::with_params("doc.merge", json!({ "path": path, "at": 1 })),
    );
    result.expect("doc.merge");
    let (_, undone) = workspace.run(Scope::of(ctx), Command::new("edit.undo"));
    undone.expect("edit.undo");
    let doc = &workspace.get(ctx).unwrap().session.document;
    assert!(doc.next_object_id >= before.0);
    assert!(doc.next_story_id >= before.1);
    assert!(doc.next_resource_id >= before.2);
    assert!(doc.next_page_id >= before.3);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn feature_restore_command_does_not_lower_high_water() {
    let mut workspace = Workspace::default();
    let ctx = workspace.new_document("k01-restore");
    let snapshot = serde_json::to_value(&workspace.get(ctx).unwrap().session.document).unwrap();
    {
        let doc = &mut workspace.get_mut(ctx).unwrap().session.document;
        doc.next_object_id = 20;
        doc.next_story_id = 40;
        doc.next_resource_id = 30;
        doc.next_page_id = 10;
    }
    let (_, result) = workspace.run(
        Scope::of(ctx),
        Command::with_params("core.feature.restore", json!({ "document": snapshot })),
    );
    result.expect("core.feature.restore");
    let doc = &workspace.get(ctx).unwrap().session.document;
    assert_eq!(doc.next_object_id, 20);
    assert_eq!(doc.next_story_id, 40);
    assert_eq!(doc.next_resource_id, 30);
    assert_eq!(doc.next_page_id, 10);
}

#[test]
fn next_resource_id_honours_stored_counter_when_the_list_is_empty() {
    let mut doc = Document::default();
    doc.next_resource_id = 15;
    assert_eq!(doc.next_resource_id().0, 15);
    let id = doc.add_image_resource(ImageResource::default());
    assert_eq!(id, ResourceId(15));
    assert_eq!(doc.next_resource_id, 16);
    doc.resources.clear();
    assert_eq!(doc.next_resource_id().0, 16);
}
