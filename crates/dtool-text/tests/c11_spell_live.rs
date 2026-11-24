use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::{Command, Document, Session, StoryId};
use dtool_text::spell_live::{squiggle, underlines, LiveSpell};
use dtool_text::spelling::Speller;
use std::time::{Duration, Instant};

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

fn type_text(session: &mut Session, story: StoryId, at: usize, text: &str) {
    let mut insert = Command::new("text.insert");
    insert.params["story"] = story.0.into();
    insert.params["at"] = at.into();
    insert.params["text"] = text.into();
    session.run(&insert).unwrap();
}

fn words(live: &LiveSpell, story: StoryId) -> Vec<String> {
    live.marks(story).iter().map(|m| m.word.clone()).collect()
}

#[test]
fn typo_marked_after_settle_and_correct_word_not() {
    let (mut session, story) = session_with("I ");
    let mut live = LiveSpell::en_us().with_settle(Duration::from_millis(300));
    assert!(live.available());
    let t0 = Instant::now();
    type_text(&mut session, story, 2, "recieved received it");
    live.note_edit(story, t0);
    let version = session.document.version;
    let undo = session.undo.applied_steps();
    assert!(!live.tick(&session, t0 + Duration::from_millis(100)));
    assert!(live.marks(story).is_empty());
    assert!(live.tick(&session, t0 + Duration::from_millis(400)));
    assert_eq!(words(&live, story), vec!["recieved"]);
    assert_eq!(live.marks(story)[0].start, 2);
    assert!(!live.is_stale(&session, story));
    assert_eq!(session.document.version, version);
    assert_eq!(session.undo.applied_steps(), undo);
    let (item, list) = live.suggestions_at(story, 4).unwrap();
    assert_eq!(item.word, "recieved");
    assert!(list.iter().any(|s| s == "received"));
    assert!(live.suggestions_at(story, 12).is_none());
}

#[test]
fn ignore_clears_mark_without_document_change() {
    let (session, story) = session_with("I recieved it");
    let mut live = LiveSpell::en_us();
    live.refresh(&session, story);
    assert_eq!(words(&live, story), vec!["recieved"]);
    let version = session.document.version;
    live.ignore(&session, "Recieved");
    assert!(live.marks(story).is_empty());
    assert_eq!(session.document.version, version);
}

#[test]
fn correct_hangul_is_not_marked() {
    let (session, story) = session_with("\u{d559}\u{ad50} \u{c544}\u{b984}\u{b2e4}\u{c6b4}");
    let mut live = LiveSpell::en_us();
    live.refresh(&session, story);
    assert!(live.marks(story).is_empty());
}

#[test]
fn no_dictionary_means_no_marks() {
    let (session, story) = session_with("I recieved it");
    let version = session.document.version;
    let mut live = LiveSpell::new(Speller::from_data("", "").ok());
    assert!(!live.available());
    let t0 = Instant::now();
    live.note_edit(story, t0);
    live.tick(&session, t0 + Duration::from_secs(1));
    live.ignore(&session, "recieved");
    assert!(live.marks(story).is_empty());
    assert!(live.suggestions_at(story, 4).is_none());
    assert_eq!(session.document.version, version);
}

#[test]
fn replace_uses_one_undo_and_clears_mark() {
    let (mut session, story) = session_with("I recieved it");
    let mut live = LiveSpell::en_us();
    live.refresh(&session, story);
    let undo = session.undo.applied_steps();
    live.replace_at(&mut session, story, 3, "received").unwrap();
    assert_eq!(session.undo.applied_steps(), undo + 1);
    assert!(live.marks(story).is_empty());
}

#[test]
fn underline_geometry_splits_lines() {
    let (session, story) = session_with("ab recieved");
    let mut live = LiveSpell::en_us();
    live.refresh(&session, story);
    let mut carets: Vec<[f32; 2]> = (0..=11).map(|i| [i as f32 * 10.0, 20.0]).collect();
    let lines = underlines(live.marks(story), &carets, 12.0);
    assert_eq!(lines.len(), 1);
    assert_eq!((lines[0].x0, lines[0].x1, lines[0].y), (30.0, 110.0, 20.0));
    for (k, c) in carets.iter_mut().enumerate().skip(7) {
        *c = [(k - 7) as f32 * 10.0, 34.0];
    }
    let lines = underlines(live.marks(story), &carets, 12.0);
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].start, lines[0].end), (3, 7));
    assert_eq!((lines[1].start, lines[1].end), (7, 11));
    assert_eq!(lines[1].y, 34.0);
    let pts = squiggle(&lines[0], 1.5, 4.0);
    assert!(pts.len() > 2);
    assert_eq!(pts[0], [lines[0].x0, lines[0].y]);
}
