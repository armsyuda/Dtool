use dtool_core::command::Session;
use dtool_core::document::{BreakKind, Document, Special, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_protocol::Command;
use serde_json::{json, Value};

fn session_with_story(text: &str, ranges: Vec<Value>, specials: Vec<(usize, Special)>) -> (Session, u64) {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let story = StoryId(50);
    session.document.stories.push(Story { id: story, text: text.to_string(), ranges, specials, paras: Vec::new() });
    session.document.create_object(Object { kind: ObjectKind::FrameText { story }, pos: [0.0, 0.0], size: [200.0, 100.0], ..Object::default() });
    (session, story.0)
}

fn story(session: &Session, id: u64) -> Story {
    session.document.stories.iter().find(|s| s.id.0 == id).cloned().unwrap()
}

fn run(session: &mut Session, name: &str, params: Value) {
    session.run(&Command::with_params(name, params)).unwrap();
}

#[test]
fn t1_insert_and_delete_shift_ranges_and_specials_and_undo_restores() {
    let ranges = vec![json!({"start": 0, "end": 5, "family": "X"}), json!({"start": 2, "end": 3, "fill": "red"})];
    let specials = vec![(4, Special::Break(BreakKind::Line))];
    let (mut s, id) = session_with_story("abcde", ranges, specials);
    let original = story(&s, id);
    run(&mut s, "text.insert", json!({"story": id, "at": 1, "text": "Z"}));
    let after = story(&s, id);
    assert_eq!(after.ranges[0]["start"], 0);
    assert_eq!(after.ranges[0]["end"], 6);
    assert_eq!(after.ranges[0]["family"], "X");
    assert_eq!(after.ranges[1]["start"], 3);
    assert_eq!(after.ranges[1]["end"], 4);
    assert_eq!(after.ranges[1]["fill"], "red");
    assert_eq!(after.specials, vec![(5, Special::Break(BreakKind::Line))]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(story(&s, id), original);
    run(&mut s, "text.delete", json!({"story": id, "range": {"start": 1, "end": 2}}));
    let deleted = story(&s, id);
    assert_eq!(deleted.text, "acde");
    assert_eq!(deleted.specials, vec![(3, Special::Break(BreakKind::Line))]);
    assert_eq!(deleted.ranges[1]["start"], 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(story(&s, id), original);
}

#[test]
fn t2_text_style_undo_and_redo() {
    let (mut s, id) = session_with_story("abcde", vec![json!({"start": 0, "end": 5, "size_pt": 12.0})], Vec::new());
    let original = story(&s, id).ranges;
    run(&mut s, "text.style", json!({"story": id, "range": {"start": 0, "end": 2}, "patch": {"fill": "red"}}));
    let styled = story(&s, id).ranges;
    assert_ne!(styled, original);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(story(&s, id).ranges, original);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(story(&s, id).ranges, styled);
}

#[test]
fn text_style_fills_gaps_and_null_removes_key() {
    let (mut s, id) = session_with_story("abcde", Vec::new(), Vec::new());
    run(&mut s, "text.style", json!({"story": id, "range": {"start": 1, "end": 3}, "patch": {"family": "Y"}}));
    let ranges = story(&s, id).ranges;
    assert_eq!(ranges, vec![json!({"start": 1, "end": 3, "family": "Y"})]);
    run(&mut s, "text.style", json!({"story": id, "range": {"start": 1, "end": 3}, "patch": {"family": null}}));
    let cleared = story(&s, id).ranges;
    assert!(cleared.iter().all(|run| run.get("family").is_none()));
    assert!(cleared.iter().all(|run| !run.as_object().unwrap().values().any(Value::is_null)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(story(&s, id).ranges, ranges);
    run(&mut s, "edit.undo", json!({}));
    assert!(story(&s, id).ranges.is_empty());
}

#[test]
fn doc_object_query_reports_overset_known_without_provider() {
    let (mut s, _) = session_with_story("abcde", Vec::new(), Vec::new());
    let id = s.document.objects[0].id.0;
    let reply = s.run(&Command::with_params("doc.object.query", json!({"id": id}))).unwrap();
    assert!(reply.to_string().contains("overset_known"));
}

#[test]
fn art_undo_add_keeps_story_shared_with_table_cell() {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let reply = s.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 10.0], "text": "Art"}))).unwrap();
    let story = StoryId(reply["story"].as_u64().unwrap());
    let art = reply["id"].as_u64().unwrap();
    let table = s.run(&Command::with_params("table.create", json!({"rows": 1, "cols": 1, "pos": [0.0, 100.0]}))).unwrap()["object_id"].as_u64().unwrap();
    if let ObjectKind::Table(node) = &mut s.document.find_object_mut(table).unwrap().kind {
        node.cells[0].story = story;
    }
    run(&mut s, "text.art.undo_add", json!({"id": art, "story": story.0}));
    assert!(s.document.get_object(ObjectId(art)).is_none());
    assert!(s.document.stories.iter().any(|item| item.id == story));
}

#[test]
fn to_path_keeps_story_shared_with_table_cell() {
    let (mut s, id) = session_with_story("abc", Vec::new(), Vec::new());
    let frame = s.document.objects[0].id.0;
    let table = s.run(&Command::with_params("table.create", json!({"rows": 1, "cols": 1, "pos": [0.0, 100.0]}))).unwrap()["object_id"].as_u64().unwrap();
    if let ObjectKind::Table(node) = &mut s.document.find_object_mut(table).unwrap().kind {
        node.cells[0].story = StoryId(id);
    }
    let _ = dtool_core::text_lines::register_text_outlines(|_, _| Some(vec![vec![[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]]]));
    run(&mut s, "text.to_path", json!({"id": frame}));
    assert!(s.document.stories.iter().any(|item| item.id.0 == id && item.text == "abc"));
    run(&mut s, "edit.undo", json!({}));
    assert!(matches!(s.document.get_object(ObjectId(frame)).unwrap().kind, ObjectKind::FrameText { .. }));
}

#[test]
fn clone_object_copies_text_and_table_stories() {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let reply = s.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 10.0], "text": "Art"}))).unwrap();
    let art = ObjectId(reply["id"].as_u64().unwrap());
    let story = StoryId(reply["story"].as_u64().unwrap());
    let copy = s.document.clone_object(art).unwrap();
    let copy_story = s.document.get_object(copy).unwrap().story_id().unwrap();
    assert_ne!(copy_story, story);
    run(&mut s, "text.insert", json!({"story": copy_story.0, "at": 0, "text": "X"}));
    assert_eq!(s.document.stories.iter().find(|item| item.id == story).unwrap().text, "Art");
    let table = ObjectId(s.run(&Command::with_params("table.create", json!({"rows": 2, "cols": 1, "pos": [0.0, 100.0]}))).unwrap()["object_id"].as_u64().unwrap());
    let table_copy = s.document.clone_object(table).unwrap();
    let cells = |id: ObjectId| match &s.document.get_object(id).unwrap().kind { ObjectKind::Table(node) => node.cells.iter().map(|cell| cell.story).collect::<Vec<_>>(), _ => panic!() };
    let source = cells(table);
    let cloned = cells(table_copy);
    assert_eq!(cloned.len(), 2);
    assert!(cloned.iter().all(|story| !source.contains(story)));
    assert!(cloned.iter().all(|story| s.document.stories.iter().any(|item| item.id == *story)));
}
