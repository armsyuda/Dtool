use dtool_core::document::Units;
use dtool_core::serde_json::{json, Value};
use dtool_core::{Command, Document, ObjectId, Session, StoryId};
use dtool_text::text_host::{can_execute, run};

fn live(text: &str) -> (Session, StoryId) {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": text}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    (session, story)
}

fn host(session: &mut Session, name: &str, params: Value) -> Result<Value, dtool_core::CommandError> {
    run(session, &Command::with_params(name, params)).expect("spell command routed by text_host")
}

fn text(session: &Session, story: StoryId) -> String {
    session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

#[test]
fn check_marks_live_frame_story_without_writing_the_document() {
    let (mut session, story) = live("I recieved the parcel");
    let (version, steps) = (session.document.version, session.undo.applied_steps());
    let reply = host(&mut session, "text.spell.check", json!({"story": story.0})).unwrap();
    assert_eq!(reply["available"], json!(true));
    assert_eq!(reply["marks"], json!([{"story": story.0, "start": 2, "end": 10, "word": "recieved"}]));
    let all = host(&mut session, "text.spell.check", json!({})).unwrap();
    assert_eq!(all["marks"].as_array().unwrap().len(), 1);
    assert_eq!((session.document.version, session.undo.applied_steps()), (version, steps));
    assert!(host(&mut session, "text.spell.check", json!({"story": 999})).is_err());
}

#[test]
fn suggest_lists_the_correction() {
    let (mut session, _) = live("x");
    let reply = host(&mut session, "text.spell.suggest", json!({"word": "recieved"})).unwrap();
    assert_eq!(reply["correct"], json!(false));
    assert!(reply["suggestions"].as_array().unwrap().iter().any(|s| s == "received"));
    assert!(host(&mut session, "text.spell.suggest", json!({})).is_err());
}

#[test]
fn replace_is_one_undo_and_stale_targets_fail() {
    let (mut session, story) = live("I recieved the parcel");
    let steps = session.undo.applied_steps();
    host(&mut session, "text.spell.replace", json!({"story": story.0, "start": 2, "end": 10, "word": "recieved", "with": "received"})).unwrap();
    assert_eq!(text(&session, story), "I received the parcel");
    assert_eq!(session.undo.applied_steps(), steps + 1);
    let stale = host(&mut session, "text.spell.replace", json!({"story": story.0, "start": 2, "end": 10, "word": "recieved", "with": "x"}));
    assert_eq!(stale.unwrap_err().code, "SPELL_STALE");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(text(&session, story), "I recieved the parcel");
}

#[test]
fn ignore_clears_marks_without_a_document_write() {
    let (mut session, story) = live("the qwzzxplorb word");
    let before = host(&mut session, "text.spell.check", json!({"story": story.0})).unwrap();
    assert_eq!(before["marks"][0]["word"], json!("qwzzxplorb"));
    let version = session.document.version;
    host(&mut session, "text.spell.ignore", json!({"word": "Qwzzxplorb", "scope": "session"})).unwrap();
    let after = host(&mut session, "text.spell.check", json!({"story": story.0})).unwrap();
    assert_eq!(after["marks"], json!([]));
    assert_eq!(session.document.version, version);
}

#[test]
fn can_execute_follows_params_and_document_state() {
    let (session, story) = live("x");
    let can = |name: &str, params: Value| can_execute(&session, &Command::with_params(name, params));
    assert_eq!(can("text.spell.check", json!({})), Some(true));
    assert_eq!(can("text.spell.suggest", json!({"word": ""})), Some(false));
    assert_eq!(can("text.spell.replace", json!({"story": story.0, "start": 0, "end": 1, "word": "x"})), Some(false));
    assert_eq!(can("text.spell.replace", json!({"story": story.0, "start": 0, "end": 1, "word": "x", "with": "y"})), Some(true));
    let closed = Session::default();
    assert_eq!(can_execute(&closed, &Command::new("text.spell.check")), Some(closed.has_document));
}
