use dtool_core::document::Units;
use dtool_core::serde_json::{json, Value};
use dtool_core::{Command, Document, ObjectId, Session, StoryId};
use dtool_text::spelling::{append_user_word, read_user_words, user_dictionary_path, Speller};
use dtool_text::text_host::run;
use std::path::PathBuf;

fn pinned_appdata() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?);
    let text = dir.to_string_lossy().to_lowercase();
    (text.contains("dtool_build") && text.contains("test_home")).then_some(dir)
}

fn live(text: &str) -> (Session, StoryId) {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": text}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    (session, story)
}

fn host(session: &mut Session, name: &str, params: Value) -> Value {
    run(session, &Command::with_params(name, params)).expect("routed").unwrap()
}

#[test]
fn add_word_writes_the_user_dictionary_under_the_test_appdata_and_unmarks_the_word() {
    let Some(appdata) = pinned_appdata() else {
        eprintln!("skip: APPDATA is not a dtool_build test_home; refusing to touch the real profile");
        return;
    };
    let path = user_dictionary_path().unwrap();
    assert!(path.starts_with(&appdata), "{path:?} not under {appdata:?}");
    assert!(path.starts_with(appdata.join("DTool")));
    let (mut session, story) = live("the zqxvorbelt word");
    let version = session.document.version;
    let before = host(&mut session, "text.spell.check", json!({"story": story.0}));
    let already = read_user_words(&path).iter().any(|w| w == "zqxvorbelt");
    if !already {
        assert_eq!(before["marks"][0]["word"], json!("zqxvorbelt"));
    }
    let reply = host(&mut session, "text.spell.add_word", json!({"word": "zqxvorbelt"}));
    assert_eq!(reply["added"], json!(!already));
    assert_eq!(PathBuf::from(reply["path"].as_str().unwrap()), path);
    assert!(path.is_file());
    assert!(read_user_words(&path).iter().any(|w| w == "zqxvorbelt"));
    let after = host(&mut session, "text.spell.check", json!({"story": story.0}));
    assert_eq!(after["marks"], json!([]));
    assert_eq!(host(&mut session, "text.spell.add_word", json!({"word": "zqxvorbelt"}))["added"], json!(false));
    assert!(Speller::en_us().unwrap().is_correct("zqxvorbelt"));
    assert_eq!(session.document.version, version);
}

#[test]
fn append_user_word_rejects_blank_and_multiword_and_dedupes_case_insensitively() {
    let Some(appdata) = pinned_appdata() else {
        eprintln!("skip: APPDATA is not a dtool_build test_home");
        return;
    };
    let path = appdata.join("DTool").join("c11_spell_user_dict_unit.txt");
    assert!(append_user_word(&path, "  ").is_err());
    assert!(append_user_word(&path, "two words").is_err());
    let first = append_user_word(&path, "Wbqlorn").unwrap();
    assert!(!append_user_word(&path, "wbqlorn").unwrap());
    assert!(first || read_user_words(&path).iter().any(|w| w.eq_ignore_ascii_case("wbqlorn")));
    let mut speller = Speller::en_us().unwrap();
    assert!(!speller.is_correct("wbqlorn"));
    speller.add_user_word("Wbqlorn");
    assert!(speller.is_correct("wbqlorn") && speller.has_user_word("WBQLORN"));
}
