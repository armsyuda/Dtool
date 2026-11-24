use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_protocol::Command;
use serde_json::{json, Value};
use dtool_core::text_styles::TextStyleStore;
use dtool_core::commands::text_styles_cmd::{run_with, APPLY_NAMED, CLEAR_OVERRIDE, DEFINE};

const ID: u64 = 80;

fn session(text: &str, ranges: Vec<Value>) -> Session {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    session.document.stories.push(Story { id: StoryId(ID), text: text.to_string(), ranges, specials: Vec::new(), paras: Vec::new() });
    session
}

fn story(session: &Session) -> Story {
    session.document.stories.iter().find(|s| s.id.0 == ID).cloned().unwrap()
}

fn at(session: &Session, pos: usize, key: &str) -> Value {
    story(session).ranges.iter().find(|r| r["start"].as_u64().unwrap() as usize <= pos && r["end"].as_u64().unwrap() as usize > pos).and_then(|r| r.get(key).cloned()).unwrap_or(Value::Null)
}

fn store() -> (Session, TextStyleStore) {
    let mut s = session("hello world", vec![json!({"start": 0, "end": 11, "size_pt": 12.0})]);
    let mut store = TextStyleStore::default();
    run_with(&mut s, &mut store, &Command::with_params(DEFINE, json!({"id": "Base", "props": {"family": "A", "size_pt": 14.0}}))).unwrap().unwrap();
    run_with(&mut s, &mut store, &Command::with_params(DEFINE, json!({"id": "Head", "based_on": "Base", "props": {"weight": 700}}))).unwrap().unwrap();
    (s, store)
}

fn run(s: &mut Session, store: &mut TextStyleStore, name: &str, params: Value) -> Result<Value, dtool_protocol::CommandError> {
    run_with(s, store, &Command::with_params(name, params)).unwrap()
}

fn undo(s: &mut Session) {
    s.run(&Command::new("edit.undo")).unwrap();
}

#[test]
fn apply_named_then_clear_override_is_one_undo_each_and_based_on_resolves() {
    let (mut s, mut store) = store();
    let original = story(&s);
    let reply = run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 5}, "id": "Head"})).unwrap();
    assert_eq!(reply["changes"][0]["name"], APPLY_NAMED);
    let applied = story(&s);
    assert_eq!(at(&s, 1, "char_style"), json!("Head"));
    assert_eq!((at(&s, 1, "family"), at(&s, 1, "size_pt"), at(&s, 1, "weight")), (json!("A"), json!(14.0), json!(700)));
    assert_eq!((at(&s, 7, "char_style"), at(&s, 7, "size_pt")), (Value::Null, json!(12.0)));

    s.run(&Command::with_params("text.style", json!({"story": ID, "range": {"start": 0, "end": 5}, "patch": {"size_pt": 30.0}}))).unwrap();
    let edited = story(&s);
    run(&mut s, &mut store, CLEAR_OVERRIDE, json!({"story": ID, "range": {"start": 0, "end": 5}, "keys": ["size_pt"]})).unwrap();
    assert_eq!(at(&s, 1, "size_pt"), json!(14.0));
    assert_eq!(at(&s, 1, "char_style"), json!("Head"));
    let cleared = story(&s);

    undo(&mut s);
    assert_eq!(story(&s), edited);
    undo(&mut s);
    assert_eq!(story(&s), applied);
    undo(&mut s);
    assert_eq!(story(&s), original);
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(story(&s), applied);
    s.run(&Command::new("edit.redo")).unwrap();
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(story(&s), cleared);
}

#[test]
fn clear_all_overrides_restores_style_values() {
    let (_, mut store) = store();
    let mut s = session("hello", vec![json!({"start": 0, "end": 5, "char_style": "Head", "family": "A", "size_pt": 14.0, "weight": 300, "style_overrides": {"weight": 300}})]);
    let original = story(&s);
    run(&mut s, &mut store, CLEAR_OVERRIDE, json!({"story": ID, "range": {"start": 0, "end": 5}})).unwrap();
    assert_eq!(at(&s, 0, "weight"), json!(700));
    assert_eq!(at(&s, 0, "style_overrides"), Value::Null);
    undo(&mut s);
    assert_eq!(story(&s), original);

    run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 5}, "id": "Base", "keep_overrides": true})).unwrap();
    assert_eq!((at(&s, 0, "char_style"), at(&s, 0, "weight")), (json!("Base"), json!(300)));
    assert_eq!(at(&s, 0, "style_overrides"), json!({"weight": 300}));
    run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 5}, "id": "Base"})).unwrap();
    assert_eq!((at(&s, 0, "weight"), at(&s, 0, "style_overrides")), (Value::Null, Value::Null));
}

#[test]
fn unknown_id_is_not_found_and_story_unchanged() {
    let (mut s, mut store) = store();
    let original = story(&s);
    let generation = s.generation;
    let error = run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 5}, "id": "Nope"})).unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");
    assert_eq!(story(&s), original);
    assert_eq!(s.generation, generation);
    let error = run(&mut s, &mut store, APPLY_NAMED, json!({"story": 999, "range": {"start": 0, "end": 1}, "id": "Head"})).unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");
    let error = run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 50}, "id": "Head"})).unwrap_err();
    assert_eq!(error.code, "INVALID_PARAMS");
    assert_eq!(story(&s), original);
}

#[test]
fn noop_does_not_bump_version() {
    let (mut s, mut store) = store();
    run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 5}, "id": "Head"})).unwrap();
    let (generation, version) = (s.generation, s.document.version);
    let reply = run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 0, "end": 5}, "id": "Head"})).unwrap();
    assert_eq!(reply["changes"], json!([]));
    run(&mut s, &mut store, CLEAR_OVERRIDE, json!({"story": ID, "range": {"start": 6, "end": 11}})).unwrap();
    assert_eq!((s.generation, s.document.version), (generation, version));
}

#[test]
fn paragraph_style_snaps_to_paragraph_and_define_rejects_cycle() {
    let mut s = session("ab\ncd", Vec::new());
    let mut store = TextStyleStore::default();
    run(&mut s, &mut store, DEFINE, json!({"id": "P", "kind": "paragraph", "props": {"align": "Center"}})).unwrap();
    run(&mut s, &mut store, APPLY_NAMED, json!({"story": ID, "range": {"start": 1, "end": 1}, "id": "P", "kind": "paragraph"})).unwrap();
    let paras = story(&s).paras;
    assert_eq!(paras, vec![json!({"start": 0, "end": 3, "para_style": "P", "align": "Center"})]);
    run(&mut s, &mut store, DEFINE, json!({"id": "Q", "kind": "paragraph", "based_on": "P"})).unwrap();
    let before = store.clone();
    let error = run(&mut s, &mut store, DEFINE, json!({"id": "P", "kind": "paragraph", "based_on": "Q"})).unwrap_err();
    assert_eq!(error.code, "STYLE_CYCLE");
    assert_eq!(store, before);
}