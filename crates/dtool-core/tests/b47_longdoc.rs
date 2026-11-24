use dtool_core::document::{Document, Story, Units};
use dtool_core::frame::FrameChain;
use dtool_core::longdoc::fields::{resolve_field, FieldKind, FieldRun};
use dtool_core::longdoc::index::build_index;
use dtool_core::longdoc::toc::{build_toc, toc_text, TocSpec};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::table::model::StoryId;
use dtool_core::{Command, FrameProps, Session};
use serde_json::{json, Value};

const BLOCK: usize = 210;
const PAGES: usize = 7;
const HEADING: &str = "제목 1";

fn paged_doc(heading_pages: &[usize]) -> (Document, Vec<usize>) {
    let page = [400.0, 300.0];
    let mut doc = Document::new(page, Units::Px, 72.0, PAGES as u32);
    let story = StoryId(1);
    let mut text = String::new();
    let mut paras = Vec::new();
    let mut starts = Vec::new();
    for i in 0..PAGES {
        let start = text.chars().count();
        let head = format!("Head{}", i + 1);
        text.push_str(&head);
        text.push('\n');
        if heading_pages.contains(&i) {
            paras.push(json!({ "start": start, "end": start + head.chars().count(), "style": HEADING }));
        }
        starts.push(start);
        text.push_str(&"a".repeat(BLOCK - head.chars().count() - 2));
        text.push('\n');
    }
    doc.stories.push(Story { id: story, text, ranges: Vec::new(), specials: Vec::new(), paras });
    let mut frames = Vec::new();
    for p in 0..PAGES {
        let id = doc.create_object(Object {
            pos: [p as f32 * page[0] + 20.0, 20.0],
            size: [200.0, 100.0],
            kind: ObjectKind::FrameText { story },
            frame: Some(FrameProps::from_rect([200.0, 100.0])),
            ..Object::default()
        });
        frames.push(id);
    }
    doc.frame_chains.push(FrameChain { story, frames });
    let starts = starts.into_iter().map(|at| doc.story_pos(story, at)).collect();
    (doc, starts)
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn numbers(session: &mut Session, kind: &str) -> Vec<(u64, u64)> {
    let list = run(session, "note.list", json!({}));
    let items = list.as_array().cloned().or_else(|| list["notes"].as_array().cloned()).expect("note.list must return a list");
    items
        .iter()
        .filter(|item| item["kind"] == kind)
        .map(|item| (item["id"].as_u64().expect("note id"), item["number"].as_u64().expect("note number")))
        .collect()
}

fn two_notes_renumber(kind: &str) {
    let (doc, starts) = paged_doc(&[]);
    let mut session = Session::new(doc);
    let first = run(&mut session, "note.insert", json!({ "kind": kind, "story_pos": starts[0] + 5, "body": "one" }));
    let second = run(&mut session, "note.insert", json!({ "kind": kind, "story_pos": starts[0] + 50, "body": "two" }));
    let first_id = first["id"].as_u64().expect("first id");
    let second_id = second["id"].as_u64().expect("second id");
    assert_eq!(numbers(&mut session, kind), vec![(first_id, 1), (second_id, 2)]);
    run(&mut session, "note.delete", json!({ "id": first_id }));
    assert_eq!(numbers(&mut session, kind), vec![(second_id, 1)]);
}

#[test]
fn two_footnotes_number_one_two_and_renumber_after_delete() {
    two_notes_renumber("footnote");
}

#[test]
fn two_endnotes_number_one_two_and_renumber_after_delete() {
    two_notes_renumber("endnote");
}

#[test]
fn two_sidenotes_number_one_two_and_renumber_after_delete() {
    two_notes_renumber("sidenote");
}

#[test]
fn toc_lists_heading_pages_one_three_five() {
    let (doc, _) = paged_doc(&[0, 2, 4]);
    let spec = TocSpec { styles: vec![(HEADING.to_string(), 1)], ..TocSpec::default() };
    let entries = build_toc(&doc, &spec);
    let (text, _) = toc_text(&entries, &spec);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    for (line, page) in lines.iter().zip(["1", "3", "5"]) {
        assert!(line.ends_with(page), "{line:?} must end with {page}");
    }
}

#[test]
fn index_marks_merge_into_page_ranges() {
    let (doc, starts) = paged_doc(&[]);
    let mut session = Session::new(doc);
    for page in [1usize, 2, 3, 6] {
        run(&mut session, "index.mark.insert", json!({ "topic": "사과", "story_pos": starts[page] + 3 }));
    }
    let lines = build_index(&session.document);
    assert!(lines.iter().any(|line| line.text == "사과 2\u{2013}4, 7"), "{:?}", lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>());
}

#[test]
fn page_number_field_on_page_three_and_total_pages() {
    let (doc, _) = paged_doc(&[]);
    let page_number = FieldRun { kind: FieldKind::PageNumber, format: String::new() };
    let total = FieldRun { kind: FieldKind::TotalPages, format: String::new() };
    assert_eq!(resolve_field(&doc, 2, &page_number), "3");
    assert_eq!(resolve_field(&doc, 2, &total), PAGES.to_string());
}
