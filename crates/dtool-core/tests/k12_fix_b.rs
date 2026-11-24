use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::frame::story_owners;
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1))
}

fn unowned(session: &Session) -> Vec<StoryId> {
    session.document.stories.iter().map(|s| s.id).filter(|id| story_owners(&session.document, *id).is_empty()).collect()
}

fn cell_stories(reply: &serde_json::Value) -> Vec<u64> {
    reply["table"]["cells"].as_array().unwrap().iter().map(|cell| cell["story"].as_u64().unwrap()).collect()
}

#[test]
fn preview_create_adds_no_story_and_keeps_allocator() {
    let mut s = session();
    let next = s.document.next_story_id;
    let reply = s.run(&Command::with_params("table.create", json!({"rows": 2, "cols": 3}))).unwrap();
    assert!(s.document.stories.is_empty());
    assert_eq!(s.document.next_story_id, next);
    assert!(unowned(&s).is_empty());
    let ids = cell_stories(&reply);
    assert_eq!(ids.len(), 6);
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 6);
}

#[test]
fn preview_ids_do_not_collide_with_existing_stories() {
    let mut s = session();
    s.document.stories.push(Story { id: StoryId(7), text: "x".into(), ..Default::default() });
    let reply = s.run(&Command::with_params("table.create", json!({"rows": 1, "cols": 2}))).unwrap();
    assert!(cell_stories(&reply).iter().all(|id| *id > 7));
    assert_eq!(s.document.stories.len(), 1);
}

#[test]
fn inserted_create_owns_every_story_and_undo_leaves_none() {
    for params in [json!({"rows": 2, "cols": 2, "pos": [10.0, 20.0]}), json!({"rows": 2, "cols": 2, "insert": true})] {
        let mut s = session();
        s.run(&Command::with_params("table.create", params)).unwrap();
        assert_eq!(s.document.stories.len(), 4);
        assert!(unowned(&s).is_empty());
        s.run(&Command::new("edit.undo")).unwrap();
        assert!(s.document.objects.is_empty());
        assert!(s.document.stories.is_empty());
        assert!(unowned(&s).is_empty());
        s.run(&Command::new("edit.redo")).unwrap();
        assert_eq!(s.document.stories.len(), 4);
        assert!(unowned(&s).is_empty());
    }
}

#[test]
fn preview_then_insert_leaves_zero_unowned() {
    let mut s = session();
    s.run(&Command::with_params("table.create", json!({"rows": 3, "cols": 3}))).unwrap();
    s.run(&Command::with_params("table.create", json!({"rows": 1, "cols": 2, "insert": true}))).unwrap();
    assert_eq!(s.document.stories.len(), 2);
    assert!(unowned(&s).is_empty());
}