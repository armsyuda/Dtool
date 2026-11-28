use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::frame::{drop_story_if_unowned, story_owners, StoryOwner};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::model::{CellFormat, CellFormatMask, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_protocol::Command;
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1))
}

fn cell_stories(session: &Session, id: u64) -> Vec<StoryId> {
    match &session.document.get_object(ObjectId(id)).unwrap().kind {
        ObjectKind::Table(table) => table.cells.iter().map(|cell| cell.story).collect(),
        _ => panic!("not a table"),
    }
}

fn assert_unique_and_live(session: &Session, table: u64) {
    let mut ids: Vec<u64> = session.document.stories.iter().map(|s| s.id.0).collect();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count);
    for story in cell_stories(session, table) {
        assert!(session.document.stories.iter().any(|s| s.id == story));
    }
}

fn one_cell_table(story: StoryId) -> TableNode {
    let mut alloc = CellIdAllocator { high_water: 0 };
    let cell = TableCell { id: alloc.next(), row: 0, col: 0, row_span: 1, col_span: 1, merged_into: None, story, local: CellFormat::default(), mask: CellFormatMask::default() };
    TableNode { alloc, col_widths_pt: vec![50.0], row_heights_pt: vec![20.0], cells: vec![cell], header_rows: 0, footer_rows: 0, format: None, frame_local: TableFrameFormat::default(), frame_mask: FrameFormatMask::default(), text_defaults: None }
}

#[test]
fn t5_table_create_after_art_text_gets_unique_story_ids() {
    let mut session = session();
    session.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 10.0], "text": "Art"}))).unwrap();
    let art_story = session.document.stories[0].id;
    let reply = session.run(&Command::with_params("table.create", json!({"rows": 2, "cols": 2, "pos": [0.0, 100.0]}))).unwrap();
    let table = reply["object_id"].as_u64().unwrap();
    let stories = cell_stories(&session, table);
    assert_eq!(stories.len(), 4);
    assert!(!stories.contains(&art_story));
    assert_unique_and_live(&session, table);
    session.run(&Command::with_params("table.row.insert", json!({"object_id": table, "row": 1}))).unwrap();
    session.run(&Command::with_params("table.column.insert", json!({"object_id": table, "col": 1}))).unwrap();
    assert_eq!(cell_stories(&session, table).len(), 9);
    assert_unique_and_live(&session, table);
}

#[test]
fn t5_table_story_ids_are_not_reused_after_delete() {
    let mut session = session();
    session.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 10.0], "text": "Art"}))).unwrap();
    let removed = session.document.stories[0].id;
    session.document.objects.clear();
    session.document.stories.clear();
    let reply = session.run(&Command::with_params("table.create", json!({"rows": 1, "cols": 2, "pos": [0.0, 0.0]}))).unwrap();
    let stories = cell_stories(&session, reply["object_id"].as_u64().unwrap());
    assert!(stories.iter().all(|story| story.0 > removed.0));
}

#[test]
fn t5_paste_twice_keeps_story_count_and_writes_text() {
    let mut session = session();
    let reply = session.run(&Command::with_params("table.create", json!({"rows": 2, "cols": 2, "pos": [0.0, 0.0]}))).unwrap();
    let table = reply["object_id"].as_u64().unwrap();
    let count = session.document.stories.len();
    session.run(&Command::with_params("table.paste", json!({"object_id": table, "row": 0, "col": 0, "tsv": "a\tb"}))).unwrap();
    assert_eq!(session.document.stories.len(), count);
    session.run(&Command::with_params("table.paste", json!({"object_id": table, "row": 0, "col": 0, "tsv": "c\td"}))).unwrap();
    assert_eq!(session.document.stories.len(), count);
    let first = cell_stories(&session, table)[0];
    assert_eq!(session.document.stories.iter().find(|s| s.id == first).unwrap().text, "c");
    assert_unique_and_live(&session, table);
}

#[test]
fn t6_story_shared_by_frame_and_table_cell_survives_frame_removal() {
    let mut doc = Document::default();
    doc.stories.push(Story { id: StoryId(5), text: "shared".into(), ..Story::default() });
    doc.objects.push(Object { id: ObjectId(1), kind: ObjectKind::FrameText { story: StoryId(5) }, ..Object::default() });
    doc.objects.push(Object { id: ObjectId(2), kind: ObjectKind::Table(one_cell_table(StoryId(5))), ..Object::default() });
    assert_eq!(story_owners(&doc, StoryId(5)), vec![StoryOwner::Object(ObjectId(1)), StoryOwner::TableCell(ObjectId(2), CellId(1))]);
    doc.objects.retain(|object| object.id != ObjectId(1));
    assert!(!drop_story_if_unowned(&mut doc, StoryId(5)));
    assert_eq!(doc.stories.len(), 1);
    doc.objects.clear();
    assert!(drop_story_if_unowned(&mut doc, StoryId(5)));
    assert!(doc.stories.is_empty());
}

#[test]
fn t6_link_refuses_target_story_owned_by_table_cell() {
    let mut session = session();
    let a = session.run(&Command::with_params("text.frame.add", json!({"rect": [0, 0, 100, 20], "text": "one"}))).unwrap()["id"].as_u64().unwrap();
    let b = session.run(&Command::with_params("text.frame.add", json!({"rect": [0, 40, 100, 20], "text": "two"}))).unwrap()["id"].as_u64().unwrap();
    let b_story = dtool_core::frame::frame_story(&session.document, ObjectId(b)).unwrap();
    session.document.objects.push(Object { id: ObjectId(900), kind: ObjectKind::Table(one_cell_table(b_story)), ..Object::default() });
    let before = serde_json::to_value(&session.document).unwrap();
    assert!(session.run(&Command::with_params("text.link", json!({"from": a, "to": b}))).is_err());
    assert_eq!(serde_json::to_value(&session.document).unwrap(), before);
}

#[test]
fn t3_para_set_rejects_bad_values_without_change() {
    let mut session = session();
    session.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 10.0], "text": "abc"}))).unwrap();
    let story = session.document.stories[0].id.0;
    let before = serde_json::to_value(&session.document).unwrap();
    let generation = session.generation;
    for patch in [
        json!({"list": {"level": 0}}),
        json!({"space_before": -1.0}),
        json!({"tabs": [{"pos": 20.0}, {"pos": 10.0}]}),
    ] {
        let result = session.run(&Command::with_params("text.para.set", json!({"story": story, "range": "all", "patch": patch})));
        assert!(result.is_err());
        assert_eq!(serde_json::to_value(&session.document).unwrap(), before);
        assert_eq!(session.generation, generation);
    }
}