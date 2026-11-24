use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::{Command, Document, Session, StoryId};
use dtool_text::spelling::{replace, words, Misspelling, Speller};
use std::sync::OnceLock;

fn speller() -> &'static Speller {
    static SPELLER: OnceLock<Speller> = OnceLock::new();
    SPELLER.get_or_init(|| Speller::en_us().expect("en_US dictionary"))
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

fn story_text(session: &Session, story: StoryId) -> String {
    session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

#[test]
fn known_typo_is_reported_and_dictionary_words_are_not() {
    let (session, story) = session_with("The quick brown fox recieved teh letter, didn't it? \u{d55c}\u{ae00} 2024 ABC");
    let found = speller().check_story(&session, story).unwrap();
    let flagged: Vec<&str> = found.iter().map(|m| m.word.as_str()).collect();
    assert_eq!(flagged, vec!["recieved", "teh"]);
    assert_eq!(found[0], Misspelling { start: 20, end: 28, word: "recieved".into() });
    for word in ["quick", "letter", "didn't", "received", "The"] {
        assert!(speller().is_correct(word), "{word}");
    }
    assert!(speller().suggest("recieved").iter().any(|s| s == "received"));
}

#[test]
fn replace_is_one_undo() {
    let (mut session, story) = session_with("I recieved it");
    let item = speller().check_story(&session, story).unwrap().remove(0);
    let depth = session.undo.applied_steps();
    replace(&mut session, story, &item, "received").unwrap();
    assert_eq!(story_text(&session, story), "I received it");
    assert_eq!(session.undo.applied_steps(), depth + 1);
    assert!(speller().check_story(&session, story).unwrap().is_empty());
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&session, story), "I recieved it");
    assert!(replace(&mut session, story, &Misspelling { start: 0, end: 1, word: "x".into() }, "y").is_err());
    assert_eq!(story_text(&session, story), "I recieved it");
}

#[test]
fn ignore_hides_a_word() {
    let mut speller = Speller::en_us().unwrap();
    assert_eq!(speller.check_text("dtool rocks").len(), 1);
    speller.ignore("DTool");
    assert!(speller.check_text("dtool rocks").is_empty());
    assert_eq!(words("'quoted' it\u{2019}s").iter().map(|w| w.word.as_str()).collect::<Vec<_>>(), vec!["quoted", "it's"]);
}

#[test]
fn missing_dictionary_data_leaves_story() {
    let (session, story) = session_with("I recieved it");
    let version = session.document.version;
    assert!(Speller::from_data("", "").is_err());
    assert!(Speller::from_data(dtool_text::spelling::EN_US_AFF, "").is_err());
    assert!(Speller::from_data("", dtool_text::spelling::EN_US_DIC).is_err());
    assert_eq!(story_text(&session, story), "I recieved it");
    assert_eq!(session.document.version, version);
}
