use dtool_core::document::Units;
use dtool_core::serde_json::{self, json, Value};
use dtool_core::{Command, Document, ObjectId, Session, StoryId};
use dtool_text::spell_live::LiveSpell;
use std::time::{Duration, Instant};

fn live(text: &str) -> (Session, StoryId) {
    assert!(dtool_text::text_host::install());
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": text}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    (session, story)
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap()
}

fn marked(session: &mut Session, story: StoryId) -> Vec<String> {
    run(session, "text.spell.check", json!({"story": story.0}))["marks"].as_array().unwrap().iter().map(|m| m["word"].as_str().unwrap().to_string()).collect()
}

#[test]
fn document_ignore_is_one_undo_step_and_only_affects_that_document() {
    let (mut a, sa) = live("the vbrzzqlet word");
    let (mut b, sb) = live("another vbrzzqlet here");
    assert_eq!(marked(&mut a, sa), vec!["vbrzzqlet"]);
    let steps = a.undo.applied_steps();
    let version = a.document.version;
    let reply = run(&mut a, "text.spell.ignore", json!({"word": "Vbrzzqlet"}));
    assert_eq!(reply["scope"], json!("document"));
    assert_eq!(a.undo.applied_steps(), steps + 1);
    assert!(a.document.version > version);
    assert_eq!(a.document.spell_ignore, vec!["vbrzzqlet".to_string()]);
    assert!(marked(&mut a, sa).is_empty());
    assert_eq!(marked(&mut b, sb), vec!["vbrzzqlet"]);
    run(&mut a, "edit.undo", json!({}));
    assert!(a.document.spell_ignore.is_empty());
    assert_eq!(marked(&mut a, sa), vec!["vbrzzqlet"]);
    run(&mut a, "edit.redo", json!({}));
    assert!(marked(&mut a, sa).is_empty());
    assert!(!a.can_execute(&Command::with_params("text.spell.ignore.restore", json!({"list": []}))));
}

#[test]
fn document_ignore_list_survives_native_save_and_open() {
    let (mut a, sa) = live("the qqmorbix word");
    run(&mut a, "text.spell.ignore", json!({"word": "qqmorbix"}));
    let saved = serde_json::to_string(&a.document).unwrap();
    assert!(saved.contains("spell_ignore"));
    let reopened = Document::from_native_json(&saved).unwrap();
    assert_eq!(reopened.spell_ignore, vec!["qqmorbix".to_string()]);
    let mut b = Session::new(reopened);
    assert!(marked(&mut b, sa).is_empty());
    let plain = serde_json::to_string(&Session::new(Document::new([10.0, 10.0], Units::Px, 72.0, 1)).document).unwrap();
    assert!(!plain.contains("spell_ignore"));
    assert!(Document::from_native_json(&plain).unwrap().spell_ignore.is_empty());
}

#[test]
fn live_spell_honours_the_document_ignore_list() {
    let (mut a, sa) = live("a pxlqorvent b");
    let mut spell = LiveSpell::en_us().with_settle(Duration::ZERO);
    let t0 = Instant::now();
    spell.note_edit(sa, t0);
    spell.tick(&a, t0);
    assert_eq!(spell.marks(sa).len(), 1);
    run(&mut a, "text.spell.ignore", json!({"word": "pxlqorvent"}));
    spell.refresh(&a, sa);
    assert!(spell.marks(sa).is_empty());
}

#[test]
fn session_scope_ignore_keeps_the_document_clean() {
    let (mut a, sa) = live("the zzvorpmix word");
    let version = a.document.version;
    let reply = run(&mut a, "text.spell.ignore", json!({"word": "zzvorpmix", "scope": "session"}));
    assert_eq!(reply["ignored"], json!("zzvorpmix"));
    assert!(a.document.spell_ignore.is_empty());
    assert_eq!(a.document.version, version);
    assert!(marked(&mut a, sa).is_empty());
}
