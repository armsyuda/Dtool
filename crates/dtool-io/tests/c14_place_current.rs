use std::collections::HashSet;
use std::path::PathBuf;

use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{ContextId, ObjectId, ObjectKind, StoryId};
use dtool_protocol::Command;
use serde_json::{json, Value};

const EPS: &str = "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 72 72\n%%EndComments\nnewpath 10 10 moveto 60 10 lineto 60 60 lineto closepath fill\nshowpage\n%%EOF\n";
const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><rect x=\"5\" y=\"5\" width=\"40\" height=\"30\" fill=\"red\"/><g><circle cx=\"80\" cy=\"40\" r=\"10\"/><rect x=\"100\" y=\"10\" width=\"20\" height=\"20\"/></g><text x=\"10\" y=\"80\" font-size=\"12\">Hi</text></svg>";

fn file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("c14_place_current_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn live() -> (Workspace, ContextId) {
    dtool_io::import_commands::install();
    let mut ws = Workspace::default();
    let id = ws.new_document("Live");
    let (_, result) = ws.run(Scope::active(), Command::with_params("text.frame.add", json!({ "rect": [0.0, 0.0, 100.0, 40.0], "text": "Existing" })));
    result.expect("existing frame");
    ws.get_mut(id).unwrap().session.text_edit = None;
    (ws, id)
}

fn document(ws: &Workspace, id: ContextId) -> Value {
    serde_json::to_value(&ws.get(id).unwrap().session.document.objects).unwrap()
}

fn stories(ws: &Workspace, id: ContextId) -> Vec<StoryId> {
    ws.get(id).unwrap().session.document.stories.iter().map(|story| story.id).collect()
}

fn text_story(kind: &ObjectKind) -> Option<StoryId> {
    match kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => Some(*story),
        ObjectKind::PathText(data) => Some(data.story),
        _ => None,
    }
}

#[test]
fn svg_place_into_current_remaps_ids_and_commits_once() {
    let (mut ws, live) = live();
    let path = file("place.svg", SVG.as_bytes());
    let before_objects = document(&ws, live);
    let before_ids: HashSet<ObjectId> = ws.get(live).unwrap().session.document.objects.iter().map(|object| object.id).collect();
    let before_stories = stories(&ws, live);
    let undo_depth = ws.get(live).unwrap().session.undo.applied_steps();
    let reply = dtool_io::import_into_current(&mut ws, &path).expect("svg places into the live document");
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(ws.active(), Some(live));
    let session = &ws.get(live).unwrap().session;
    assert_eq!(session.undo.applied_steps(), undo_depth + 1, "place is one undo step");
    let placed: Vec<ObjectId> = reply["ids"].as_array().unwrap().iter().map(|id| ObjectId(id.as_u64().unwrap())).collect();
    let mut opened = Workspace::default();
    let copy = dtool_io::open_interchange(&mut opened, &path).expect("svg opens");
    let expected = opened.get(copy).unwrap().session.document.objects.len();
    assert!(expected >= 3);
    assert_eq!(placed.len(), expected, "every object of the file is placed: {placed:?}");
    assert!(placed.iter().all(|id| !before_ids.contains(id)), "placed ids must not reuse existing ids");
    let all: Vec<ObjectId> = session.document.objects.iter().map(|object| object.id).collect();
    assert_eq!(all.len(), all.iter().collect::<HashSet<_>>().len(), "object ids stay unique");
    for id in &placed {
        let object = session.document.get_object(*id).expect("placed object exists");
        if let Some(parent) = object.parent_id {
            assert!(session.document.get_object(parent).is_some(), "parent links point at placed objects");
        }
        if let Some(story) = text_story(&object.kind) {
            assert!(!before_stories.contains(&story), "placed text gets a fresh story");
            assert!(session.document.stories.iter().any(|item| item.id == story), "placed story exists");
        }
    }
    assert!(!session.selection.ids.is_empty());
    assert!(session.selection.ids.iter().all(|id| placed.contains(id)));
    let existing: Vec<Value> = session.document.objects.iter().filter(|object| before_ids.contains(&object.id)).map(|object| serde_json::to_value(object).unwrap()).collect();
    assert_eq!(Value::Array(existing), before_objects, "existing objects untouched");
    let (_, undo) = ws.run(Scope::active(), Command::new("edit.undo"));
    undo.expect("undo through the live workspace");
    assert_eq!(document(&ws, live), before_objects);
    assert_eq!(stories(&ws, live), before_stories);
    let (_, redo) = ws.run(Scope::active(), Command::new("edit.redo"));
    redo.expect("redo place");
    let session = &ws.get(live).unwrap().session;
    assert!(placed.iter().all(|id| session.document.get_object(*id).is_some()));
}

#[test]
fn eps_place_into_current_and_failures_change_nothing() {
    let (mut ws, live) = live();
    let good = file("place.eps", EPS.as_bytes());
    let bad = file("broken.eps", b"not postscript");
    let missing = std::env::temp_dir().join(format!("c14_place_missing_{}.svg", std::process::id()));
    let before = document(&ws, live);
    let undo_depth = ws.get(live).unwrap().session.undo.applied_steps();
    for path in [&bad, &missing] {
        assert!(dtool_io::import_into_current(&mut ws, path).is_err());
        assert_eq!(document(&ws, live), before);
        assert_eq!(ws.get(live).unwrap().session.undo.applied_steps(), undo_depth);
        assert_eq!(ws.contexts().len(), 1);
    }
    let reply = dtool_io::import_into_current(&mut ws, &good).expect("eps places");
    let id = ObjectId(reply["ids"][0].as_u64().unwrap());
    let curve = ws.get(live).unwrap().session.document.get_object(id).cloned().unwrap();
    assert!(curve.curve().is_some());
    assert_eq!(curve.pos, [10.0, 12.0]);
    assert_eq!(ws.contexts().len(), 1);
    ws.run(Scope::active(), Command::new("edit.undo")).1.expect("undo");
    assert_eq!(document(&ws, live), before);
}

#[test]
fn place_needs_a_document() {
    dtool_io::import_commands::install();
    let mut ws = Workspace::default();
    let path = file("none.svg", SVG.as_bytes());
    assert_eq!(dtool_io::import_into_current(&mut ws, &path).expect_err("no document").code, "NO_DOCUMENT");
    assert!(ws.contexts().is_empty());
}