use dtool_core::document::{Document, PageId, Story, StoryId, Units, ValidateError};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathTextData};
use dtool_core::workspace::Workspace;

fn story(id: u64) -> Story {
    Story { id: StoryId(id), text: "abc".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() }
}

fn temp_file(name: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("k01_{}_{}", name, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.dtool"));
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn new_document_pages_have_stable_ids() {
    let doc = Document::new([100.0, 100.0], Units::Px, 96.0, 3);
    let ids: Vec<u64> = doc.pages.iter().map(|p| p.id.0).collect();
    assert_eq!(ids, vec![1, 2, 3]);
    assert_eq!(doc.next_page_id, 4);
}

#[test]
fn legacy_file_without_page_ids_hydrates_ids_and_counters() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 2);
    doc.create_object(Object { id: ObjectId(7), ..Object::default() });
    let mut value = serde_json::to_value(&doc).unwrap();
    for page in value["pages"].as_array_mut().unwrap() { page.as_object_mut().unwrap().remove("id"); }
    let obj = value.as_object_mut().unwrap();
    obj.remove("next_page_id");
    obj.remove("next_story_id");
    obj.insert("next_object_id".into(), serde_json::json!(2));
    let loaded = Document::from_native_json(&value.to_string()).unwrap();
    assert_eq!(loaded.pages.iter().map(|p| p.id.0).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(loaded.next_page_id, 3);
    assert!(loaded.next_object_id >= 8, "stale counter must be raised above max id");
}

#[test]
fn missing_next_object_id_key_still_opens() {
    let doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let mut value = serde_json::to_value(&doc).unwrap();
    value.as_object_mut().unwrap().remove("next_object_id");
    let loaded = Document::from_native_json(&value.to_string()).unwrap();
    assert!(loaded.next_object_id >= 1);
}

#[test]
fn page_membership_wins_over_position_and_survives_round_trip() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 2);
    let second = doc.pages[1].id;
    let id = doc.create_object(Object { pos: [10.0, 10.0], size: [5.0, 5.0], page_id: Some(second), ..Object::default() });
    assert_eq!(dtool_core::data_merge::layout::object_page_index(&doc, id), 1);
    let body = serde_json::to_string(&doc).unwrap();
    let loaded = Document::from_native_json(&body).unwrap();
    assert_eq!(loaded.get_object(id).unwrap().page_id, Some(second));
    assert_eq!(dtool_core::data_merge::layout::object_page_index(&loaded, id), 1);
}

#[test]
fn dangling_page_membership_is_rejected() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    doc.create_object(Object { page_id: Some(PageId(99)), ..Object::default() });
    assert!(matches!(doc.validate_refs(), Err(ValidateError::MissingPage { page: 99, .. })));
}

#[test]
fn page_ids_are_not_reused_after_delete() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 2);
    let last = doc.push_page([100.0, 100.0]);
    doc.pages.pop();
    let next = doc.push_page([100.0, 100.0]);
    assert!(next.0 > last.0);
}

#[test]
fn story_and_resource_ids_are_not_reused_after_delete() {
    let mut doc = Document::default();
    let s = doc.alloc_story_id();
    doc.stories.push(story(s.0));
    doc.stories.clear();
    assert!(doc.alloc_story_id().0 > s.0);

    let r = doc.add_image_resource(Default::default());
    doc.remove_image_resource(r);
    let r2 = doc.add_image_resource(Default::default());
    assert!(r2.0 > r.0, "removed resource id must not be reused");
}

#[test]
fn create_object_never_inserts_a_duplicate_id() {
    let mut doc = Document::default();
    let a = doc.create_object(Object { id: ObjectId(5), ..Object::default() });
    let b = doc.create_object(Object { id: ObjectId(5), ..Object::default() });
    assert_eq!(a, ObjectId(5));
    assert_ne!(b, ObjectId(5));
    assert!(doc.try_create_object(Object { id: ObjectId(5), ..Object::default() }).is_err());
    assert_eq!(doc.objects.len(), 2);
    assert!(doc.validate_refs().is_ok());
}

#[test]
fn restore_does_not_lower_high_water() {
    let old = Document::default();
    let mut current = Document::default();
    current.next_story_id = 40;
    current.next_resource_id = 30;
    current.next_object_id = 20;
    let mut restored = old.clone();
    restored.keep_high_water_from(&current);
    assert_eq!((restored.next_object_id, restored.next_story_id, restored.next_resource_id), (20, 40, 30));
}

#[test]
fn open_rejects_duplicate_ids_and_missing_story() {
    let mut doc = Document::default();
    doc.objects.push(Object { id: ObjectId(1), ..Object::default() });
    doc.objects.push(Object { id: ObjectId(1), ..Object::default() });
    assert_eq!(doc.validate_refs(), Err(ValidateError::DuplicateObjectId(1)));

    let mut doc = Document::default();
    let path = doc.create_object(Object::default());
    doc.create_object(Object { kind: ObjectKind::PathText(PathTextData { path, story: StoryId(9), ..PathTextData::default() }), ..Object::default() });
    assert!(matches!(doc.validate_refs(), Err(ValidateError::MissingStory { story: 9, .. })));
    let body = serde_json::to_string(&doc).unwrap();
    assert_eq!(Document::from_native_json(&body).unwrap_err().code, "IO.INVALID_FORMAT");
}

#[test]
fn workspace_open_of_invalid_file_keeps_current_document() {
    let mut doc = Document::default();
    doc.objects.push(Object { id: ObjectId(3), ..Object::default() });
    doc.objects.push(Object { id: ObjectId(3), ..Object::default() });
    let path = temp_file("dup", &serde_json::to_string(&doc).unwrap());

    let mut ws = Workspace::default();
    let first = ws.new_document("A");
    let before = ws.get(first).unwrap().session.document.objects.len();
    let err = ws.open_path(&path).unwrap_err();
    assert_eq!(err.code, "IO.INVALID_FORMAT");
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(ws.active(), Some(first));
    assert_eq!(ws.get(first).unwrap().session.document.objects.len(), before);
}

#[test]
fn shared_story_delete_undo_save_reopen_keeps_unique_ids() {
    let mut ws = Workspace::default();
    let id = ws.new_document("A");
    {
        let session = &mut ws.get_mut(id).unwrap().session;
        let s = session.document.alloc_story_id();
        session.document.stories.push(story(s.0));
        session.document.create_object(Object { kind: ObjectKind::ArtText { story: s }, ..Object::default() });
        session.document.create_object(Object { kind: ObjectKind::ArtText { story: s }, ..Object::default() });
    }
    let victim = ws.get(id).unwrap().session.document.objects[0].id.0;
    let (_, r) = ws.run(dtool_core::workspace::Scope::active(), dtool_protocol::Command::with_params("object.delete", serde_json::json!({ "id": victim })));
    assert!(r.is_ok(), "{r:?}");
    let (_, r) = ws.run(dtool_core::workspace::Scope::active(), dtool_protocol::Command::new("edit.undo"));
    assert!(r.is_ok(), "{r:?}");
    let path = temp_file("shared", "{}");
    ws.save_as(id, &path).unwrap();
    let reopened = Document::from_native_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut ids: Vec<u64> = reopened.objects.iter().map(|o| o.id.0).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), reopened.objects.len());
    assert!(reopened.next_object_id > *ids.last().unwrap());
    assert!(reopened.validate_refs().is_ok());
}
