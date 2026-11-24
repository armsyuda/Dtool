#[path = "../src/spelling.rs"]
#[allow(dead_code)]
mod spelling;
#[path = "../src/spelling_panel.rs"]
#[allow(dead_code)]
mod spelling_panel;

use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::{Command, Document, Session, StoryId};
use spelling::Speller;
use spelling_panel::{SpellPanel, SpellScope};

fn add_story(session: &mut Session, text: &str, y: f64) -> StoryId {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0f64, y].into();
    add.params["text"] = text.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    match session.document.get_object(id).unwrap().kind {
        ObjectKind::ArtText { story } => story,
        _ => unreachable!(),
    }
}

fn new_session() -> Session {
    Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1))
}

fn story_text(session: &Session, story: StoryId) -> String {
    session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

#[test]
fn lists_issue_with_suggestion() {
    let mut session = new_session();
    let story = add_story(&mut session, "I recieved it \u{d55c}\u{ae00}", 20.0);
    let mut panel = SpellPanel::en_us(SpellScope::Story(story));
    panel.check(&session);
    assert_eq!(panel.rows(), vec!["recieved"]);
    assert_eq!(panel.selected, Some(0));
    assert!(panel.suggestions.iter().any(|s| s == "received"));
    assert!(panel.error.is_none());
}

#[test]
fn replace_is_one_undo_and_undo_restores() {
    let mut session = new_session();
    let story = add_story(&mut session, "I recieved it", 20.0);
    let mut panel = SpellPanel::en_us(SpellScope::Story(story));
    panel.check(&session);
    let depth = session.undo.applied_steps();
    panel.replace(&mut session, "received").unwrap();
    assert_eq!(story_text(&session, story), "I received it");
    assert_eq!(session.undo.applied_steps(), depth + 1);
    assert!(panel.issues.is_empty());
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&session, story), "I recieved it");
    panel.check(&session);
    assert_eq!(panel.rows(), vec!["recieved"]);
}

#[test]
fn stale_range_returns_spell_stale_without_write() {
    let mut session = new_session();
    let story = add_story(&mut session, "I recieved it", 20.0);
    let mut panel = SpellPanel::en_us(SpellScope::Story(story));
    panel.check(&session);
    let mut insert = Command::new("text.insert");
    insert.params["story"] = story.0.into();
    insert.params["at"] = 0usize.into();
    insert.params["text"] = "Oh ".into();
    session.run(&insert).unwrap();
    let version = session.document.version;
    let err = panel.replace(&mut session, "received").unwrap_err();
    assert_eq!(err.code, "SPELL_STALE");
    assert_eq!(session.document.version, version);
    assert_eq!(story_text(&session, story), "Oh I recieved it");
    assert_eq!(panel.error.as_deref(), Some("SPELL_STALE"));
    assert_eq!(panel.issues[0].item.start, 5);
}

#[test]
fn ignore_keeps_version_and_hides_word() {
    let mut session = new_session();
    let a = add_story(&mut session, "I recieved it", 20.0);
    add_story(&mut session, "recieved twice, teh end", 60.0);
    let mut panel = SpellPanel::en_us(SpellScope::Document);
    panel.check(&session);
    assert_eq!(panel.rows(), vec!["recieved", "recieved", "teh"]);
    assert_eq!(panel.issues[0].story, a);
    let version = session.document.version;
    panel.ignore(&session).unwrap();
    assert_eq!(session.document.version, version);
    assert_eq!(panel.rows(), vec!["teh"]);
    panel.check(&session);
    assert_eq!(panel.rows(), vec!["teh"]);
}

#[test]
fn missing_dictionary_reports_and_does_not_mutate() {
    let mut session = new_session();
    let story = add_story(&mut session, "I recieved it", 20.0);
    let version = session.document.version;
    let mut panel = SpellPanel::new(Speller::from_data("", ""), SpellScope::Story(story));
    assert!(!panel.available());
    assert_eq!(panel.error.as_deref(), Some("SPELL_DICTIONARY_UNAVAILABLE"));
    panel.check(&session);
    assert!(panel.issues.is_empty());
    assert_eq!(panel.replace(&mut session, "received").unwrap_err().code, "SPELL_DICTIONARY_UNAVAILABLE");
    assert!(panel.ignore(&session).is_err());
    assert_eq!(session.document.version, version);
    assert_eq!(story_text(&session, story), "I recieved it");
}
