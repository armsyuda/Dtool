use dtool_text::spelling::{self, Speller};
use std::path::PathBuf;

#[test]
fn korean_dictionary_loads_from_the_env_dir() {
    let home = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let root = home.join("hunspell_env_test");
    let ko = root.join("ko");
    std::fs::create_dir_all(&ko).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("hunspell").join("ko");
    for name in ["ko.aff", "ko.dic"] {
        std::fs::copy(source.join(name), ko.join(name)).unwrap();
    }
    unsafe { std::env::set_var(spelling::HUNSPELL_DIR_ENV, &root) };
    assert_eq!(spelling::hunspell_candidate_dirs(), vec![root.clone()]);
    assert_eq!(spelling::hunspell_dir(), Some(root.clone()));
    assert!(!spelling::ko_loaded());
    assert!(spelling::ko_available());
    assert_eq!(spelling::ko_message_key(), None);
    let speller = Speller::en_us().expect("en_US");
    assert!(speller.is_correct("학교"));
    assert!(!speller.is_correct("학교엣서"));
    assert!(spelling::ko_loaded());
    assert_eq!(spelling::ko_dictionary_dir(), Some(ko.as_path()));
    assert_eq!(spelling::ko_dictionary_error(), None);
    assert!(!speller.suggest("학교엣서").is_empty());
    let marks: Vec<String> = speller.check_text("I recieved 학교엣서 it").into_iter().map(|m| m.word).collect();
    assert_eq!(marks, vec!["recieved", "학교엣서"]);
}
