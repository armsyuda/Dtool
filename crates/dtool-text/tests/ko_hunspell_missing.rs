use dtool_core::document::Units;
use dtool_core::serde_json::json;
use dtool_core::{Command, Document, ObjectId, Session};
use dtool_text::spelling::{self, ko_grammar, KoDictionaryError, Speller};
use std::path::PathBuf;

#[test]
fn missing_or_invalid_korean_dictionary_turns_korean_checking_off_and_leaves_english_alone() {
    let home = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let missing = home.join("hunspell_missing_test");
    let _ = std::fs::remove_dir_all(&missing);
    unsafe { std::env::set_var(spelling::HUNSPELL_DIR_ENV, &missing) };
    assert_eq!(spelling::hunspell_candidate_dirs(), vec![missing.clone()]);
    assert_eq!(spelling::hunspell_dir(), None);
    assert!(!spelling::ko_available());
    assert_eq!(spelling::ko_message_key(), Some(spelling::KO_MISSING_KEY));
    assert_eq!(spelling::KO_MISSING_KEY, "spell.ko.dictionary_missing");

    let speller = Speller::en_us().expect("en_US");
    assert!(!speller.is_correct("recieved"));
    assert!(speller.suggest("recieved").iter().any(|s| s == "received"));
    assert!(speller.is_correct("학교엣서"));
    assert!(speller.suggest("학교엣서").is_empty());
    assert!(spelling::ko_loaded());
    assert!(spelling::ko_dictionary().is_none());
    assert_eq!(spelling::ko_dictionary_error(), Some(KoDictionaryError::Missing(vec![missing.clone()])));
    assert_eq!(spelling::ko_message_key(), Some(spelling::KO_MISSING_KEY));
    let marks: Vec<String> = speller.check_text("I recieved 학교엣서 it").into_iter().map(|m| m.word).collect();
    assert_eq!(marks, vec!["recieved"]);
    assert_eq!(ko_grammar::check_common("몇일 동안").first().map(|i| i.suggestion.as_str()), Some("며칠"));

    assert!(dtool_text::text_host::install());
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": "I recieved 학교엣서 it"}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    let out = session.run(&Command::with_params("text.spell.check", json!({"story": story.0, "grammar": false}))).unwrap();
    assert_eq!(out["available"], json!(true), "{out}");
    assert_eq!(out["ko_available"], json!(false), "{out}");
    assert_eq!(out["ko_message_key"], json!("spell.ko.dictionary_missing"), "{out}");
    let words: Vec<&str> = out["marks"].as_array().unwrap().iter().filter_map(|m| m["word"].as_str()).collect();
    assert_eq!(words, vec!["recieved"], "{out}");

    let root = home.join("hunspell_invalid_test");
    let ko = root.join("ko");
    std::fs::create_dir_all(&ko).unwrap();
    std::fs::write(ko.join("ko.aff"), "").unwrap();
    std::fs::write(ko.join("ko.dic"), "0\n").unwrap();
    let error = spelling::load_ko_dictionary(&[root.clone()]).err().expect("invalid");
    assert!(matches!(error, KoDictionaryError::Invalid(ref dir, _) if *dir == ko), "{error:?}");
    assert_eq!(error.message_key(), spelling::KO_INVALID_KEY);
    let missing = spelling::load_ko_dictionary(&[root.join("nope")]).err().expect("missing");
    assert_eq!(missing.message_key(), spelling::KO_MISSING_KEY);
}
