#[allow(dead_code)]
#[path = "../src/autocorrect.rs"]
mod autocorrect;

use autocorrect::{expand_after_insert, is_trigger, pending_expansion, AutoCorrect, Expansion};
use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::prefs::Prefs;
use dtool_core::{Command, Document, Session, StoryId};

fn enabled() -> Prefs {
    let mut prefs = Prefs::default();
    prefs.autocorrect_enabled = true;
    prefs.ac_replace_while_typing = true;
    prefs
}

fn session_with(text: &str) -> (Session, StoryId) {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0f64, 20.0].into();
    add.params["text"] = text.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    let story = match session.document.get_object(id).unwrap().kind {
        ObjectKind::ArtText { story } => story,
        _ => unreachable!(),
    };
    (session, story)
}

fn type_char(session: &mut Session, story: StoryId, at: usize, ch: &str) {
    let mut insert = Command::new("text.insert");
    insert.params["story"] = story.0.into();
    insert.params["at"] = at.into();
    insert.params["text"] = ch.into();
    session.run(&insert).unwrap();
}

fn story_text(session: &Session, story: StoryId) -> String {
    session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

#[test]
fn prefs_sample_pair_expands_as_one_undo() {
    let prefs = enabled();
    assert!(prefs.ac_replacements.iter().any(|rule| rule.replace == "(c)" && rule.with == "\u{a9}"));
    let (mut session, story) = session_with("Made (c)");
    type_char(&mut session, story, 8, " ");
    let depth = session.undo.applied_steps();
    assert!(expand_after_insert(&mut session, story, 9, " ", &prefs).unwrap());
    assert_eq!(story_text(&session, story), "Made \u{a9} ");
    assert_eq!(session.undo.applied_steps(), depth + 1);
    let mut undo = Command::new("edit.undo");
    undo.params = Default::default();
    session.run(&undo).unwrap();
    assert_eq!(story_text(&session, story), "Made (c) ");
}

#[test]
fn punctuation_trigger_and_symbol_suffix() {
    let config = AutoCorrect::from_prefs(&enabled()).unwrap();
    assert_eq!(pending_expansion("x(tm).", 6, &config), Some(Expansion { start: 1, end: 5, with: "\u{2122}".into() }));
    assert_eq!(pending_expansion("wait... ", 8, &config), Some(Expansion { start: 4, end: 7, with: "\u{2026}".into() }));
    assert_eq!(pending_expansion("abc ", 4, &config), None);
    assert_eq!(pending_expansion("(c)", 3, &config), None);
}

#[test]
fn disabled_no_match_and_escape_leave_story() {
    let (mut session, story) = session_with("Made (c)");
    type_char(&mut session, story, 8, " ");
    let version = session.document.version;
    let depth = session.undo.applied_steps();
    let mut off = enabled();
    off.autocorrect_enabled = false;
    assert!(AutoCorrect::from_prefs(&off).is_none());
    assert!(!expand_after_insert(&mut session, story, 9, " ", &off).unwrap());
    let mut not_typing = enabled();
    not_typing.ac_replace_while_typing = false;
    assert!(!expand_after_insert(&mut session, story, 9, " ", &not_typing).unwrap());
    assert!(!is_trigger("\u{1b}"));
    assert!(!expand_after_insert(&mut session, story, 9, "\u{1b}", &enabled()).unwrap());
    let (mut plain, plain_story) = session_with("hello");
    type_char(&mut plain, plain_story, 5, " ");
    let plain_version = plain.document.version;
    assert!(!expand_after_insert(&mut plain, plain_story, 6, " ", &enabled()).unwrap());
    assert_eq!(plain.document.version, plain_version);
    assert_eq!(story_text(&session, story), "Made (c) ");
    assert_eq!(session.document.version, version);
    assert_eq!(session.undo.applied_steps(), depth);
}

#[test]
fn settings_toggle_does_not_dirty_document() {
    let (session, _) = session_with("x");
    let version = session.document.version;
    let depth = session.undo.applied_steps();
    let mut prefs = Prefs::default();
    prefs.autocorrect_enabled = !prefs.autocorrect_enabled;
    prefs.ac_replace_while_typing = !prefs.ac_replace_while_typing;
    assert_eq!(session.document.version, version);
    assert_eq!(session.undo.applied_steps(), depth);
}
