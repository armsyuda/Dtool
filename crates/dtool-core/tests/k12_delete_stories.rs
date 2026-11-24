use dtool_core::command::Session;
use dtool_core::document::{Document, StoryId, Units};
use dtool_core::frame::story_owners;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1))
}

fn create_table(s: &mut Session) -> u64 {
    s.run(&Command::with_params("table.create", json!({"rows": 2, "cols": 2, "pos": [0.0, 100.0]}))).unwrap()["object_id"].as_u64().unwrap()
}

#[test]
fn delete_table_drops_cell_stories_and_undo_restores_table() {
    let mut s = session();
    let table = create_table(&mut s);
    assert_eq!(s.document.stories.len(), 4);
    s.run(&Command::with_params("object.delete", json!({"id": table}))).unwrap();
    assert!(s.document.stories.is_empty());
    s.run(&Command::new("edit.undo")).unwrap();
    let restored = s.document.get_object(ObjectId(table)).unwrap();
    let ObjectKind::Table(node) = &restored.kind else { panic!("not a table") };
    assert_eq!(node.cells.len(), 4);
    assert_eq!(s.document.stories.len(), 4);
    assert!(node.cells.iter().all(|cell| !story_owners(&s.document, cell.story).is_empty()));
}

#[test]
fn apply_path_delete_drops_unowned_stories_only() {
    let mut s = session();
    let reply = s.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 10.0], "text": "Art"}))).unwrap();
    let story = StoryId(reply["story"].as_u64().unwrap());
    let table = create_table(&mut s);
    let mut replaced = None;
    if let ObjectKind::Table(node) = &mut s.document.find_object_mut(table).unwrap().kind {
        replaced = Some(node.cells[0].story);
        node.cells[0].story = story;
    }
    s.document.stories.retain(|item| Some(item.id) != replaced);
    dtool_core::command::execute_object_delete(&mut s.document, serde_json::from_value(json!({"id": table})).unwrap()).unwrap();
    assert_eq!(s.document.stories.len(), 1);
    assert_eq!(s.document.stories[0].id, story);
}
