use dtool_core::command::TextEditState;
use dtool_core::document::{Document, Story, Units};
use dtool_core::frame::FrameChain;
use dtool_core::longdoc::fields::{format_time, resolve_field, FieldKind, FieldRun};
use dtool_core::longdoc::sections::{NumberStyle, Section};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::table::model::StoryId;
use dtool_core::workspace::{ContextId, Scope, SharedServices, Workspace};
use dtool_core::{Command, FrameProps, Session};
use serde_json::{json, Value};
use std::sync::Arc;

const STORY: StoryId = StoryId(1);

fn doc_with_story(pages: u32, text: &str, paras: Vec<Value>) -> Document {
    let mut doc = Document::new([400.0, 300.0], Units::Px, 72.0, pages);
    doc.stories.push(Story { id: STORY, text: text.to_string(), ranges: Vec::new(), specials: Vec::new(), paras });
    let id = doc.create_object(Object {
        pos: [20.0, 20.0],
        size: [200.0, 100.0],
        kind: ObjectKind::FrameText { story: STORY },
        frame: Some(FrameProps::from_rect([200.0, 100.0])),
        ..Object::default()
    });
    doc.frame_chains.push(FrameChain { story: STORY, frames: vec![id] });
    doc
}

fn workspace_with(doc: Document) -> (Workspace, ContextId) {
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    let id = workspace.new_document("t");
    let ctx = workspace.get_mut(id).expect("context");
    ctx.session = Session::new(doc);
    ctx.session.text_edit = Some(TextEditState { story: STORY, object: None, caret: 0, anchor: 0, before: None });
    (workspace, id)
}

fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    workspace
        .run(Scope::of(id), Command::with_params(name, params))
        .1
        .unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn story_text(workspace: &Workspace, id: ContextId) -> String {
    workspace.get(id).expect("context").session.document.stories[0].text.clone()
}

fn insert(workspace: &mut Workspace, id: ContextId, kind: Value) -> String {
    let out = run(workspace, id, "text.field.insert", json!({ "kind": kind }));
    out["text"].as_str().expect("field text").to_string()
}

#[test]
fn total_pages_refreshes_when_page_removed() {
    let (mut ws, id) = workspace_with(doc_with_story(5, "", Vec::new()));
    assert_eq!(insert(&mut ws, id, json!("TotalPages")), "5");
    ws.get_mut(id).expect("context").session.document.pages.remove(1);
    run(&mut ws, id, "text.field.query", json!({}));
    assert_eq!(story_text(&ws, id), "4");
}

#[test]
fn section_name_shows_section() {
    let mut doc = doc_with_story(3, "", Vec::new());
    doc.set_section(
        0,
        Section {
            start_page: 1,
            name: "\u{BD80}\u{B85D}".to_string(),
            restart_at: None,
            prefix: String::new(),
            style: NumberStyle::Arabic,
            include_on_export: true,
        },
    )
    .expect("section");
    let (mut ws, id) = workspace_with(doc);
    assert_eq!(insert(&mut ws, id, json!("SectionName")), "\u{BD80}\u{B85D}");
}

#[test]
fn running_header_uses_style_paragraph() {
    let text = "Chapter One\nbody";
    let doc = doc_with_story(2, text, vec![json!({ "start": 0, "end": 11, "style": "H1" })]);
    let field = FieldRun { kind: FieldKind::RunningHeader, format: "H1".to_string() };
    assert_eq!(resolve_field(&doc, 0, &field), "Chapter One");
    assert_eq!(resolve_field(&doc, 1, &field), "Chapter One");
    let other = FieldRun { kind: FieldKind::RunningHeader, format: "H9".to_string() };
    assert_eq!(resolve_field(&doc, 0, &other), "");
}

#[test]
fn format_time_default_and_tokens() {
    assert_eq!(format_time(0, 540, ""), "1970-01-01 09:00");
    assert_eq!(format_time(1_700_000_000, 0, "yyyy/MM/dd HH:mm:ss"), "2023/11/14 22:13:20");
    assert_eq!(format_time(951_782_400, 0, "yy-M-d"), "00-2-29");
}

fn save_as_with(kind: Value, name: &str) -> (Workspace, ContextId, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("dtool_b47f_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let target = dir.join("a.dtool");
    let (mut ws, id) = workspace_with(doc_with_story(1, "", Vec::new()));
    assert_eq!(insert(&mut ws, id, kind), "");
    run(&mut ws, id, "doc.save_as", json!({ "path": target.to_string_lossy() }));
    (ws, id, target)
}

#[test]
fn filename_after_save_as() {
    let (ws, id, target) = save_as_with(json!("Filename"), "file");
    assert_eq!(story_text(&ws, id), "a.dtool");
    let _ = std::fs::remove_file(&target);
}

#[test]
fn path_after_save_as() {
    let (ws, id, target) = save_as_with(json!("Path"), "path");
    assert_eq!(story_text(&ws, id), target.to_string_lossy());
    let doc = &ws.get(id).expect("context").session.document;
    let reloaded: Document = serde_json::from_value(serde_json::to_value(doc).expect("ser")).expect("de");
    assert_eq!(reloaded.field_info.file_path, doc.field_info.file_path);
    let _ = std::fs::remove_file(&target);
}

#[test]
fn saved_and_created_after_save() {
    let (mut ws, id, target) = save_as_with(json!("Saved"), "saved");
    let info = ws.get(id).expect("context").session.document.field_info.clone();
    let stamp = info.saved.expect("saved stamp");
    assert_eq!(info.created, Some(stamp));
    assert_eq!(story_text(&ws, id), format_time(stamp, info.offset(), ""));
    ws.get_mut(id).expect("context").session.document.field_info.saved = Some(0);
    run(&mut ws, id, "doc.save", json!({}));
    let info = ws.get(id).expect("context").session.document.field_info.clone();
    assert!(info.saved.expect("saved") >= stamp);
    assert_eq!(info.created, Some(stamp));
    assert_eq!(story_text(&ws, id), format_time(info.saved.expect("saved"), info.offset(), ""));
    let _ = std::fs::remove_file(&target);
}

#[test]
fn meta_title_field_and_undo() {
    let (mut ws, id) = workspace_with(doc_with_story(1, "", Vec::new()));
    run(&mut ws, id, "doc.metadata.set", json!({ "title": "Report", "keywords": "a, b" }));
    assert_eq!(insert(&mut ws, id, json!({ "Meta": "Title" })), "Report");
    assert_eq!(insert(&mut ws, id, json!("meta.keywords")), "a, b");
    run(&mut ws, id, "doc.metadata.set", json!({ "title": "Final" }));
    assert_eq!(story_text(&ws, id), "Finala, b");
    let out = run(&mut ws, id, "doc.metadata.query", json!({}));
    assert_eq!(out["metadata"]["title"], "Final");
}

#[test]
fn menu_entries_insert_new_kinds() {
    let (mut ws, id) = workspace_with(doc_with_story(2, "", Vec::new()));
    run(&mut ws, id, "text.insert.fields.saved_date", json!({}));
    run(&mut ws, id, "text.insert.fields.date_and_time", json!({}));
    assert_eq!(story_text(&ws, id).chars().count(), 16);
}

#[test]
fn printed_stamp_on_print_command_name() {
    assert!(dtool_core::longdoc::fields::is_print_command("export.pdf"));
    assert!(dtool_core::longdoc::fields::is_print_command("file.print"));
    assert!(!dtool_core::longdoc::fields::is_print_command("exporter.list"));
}