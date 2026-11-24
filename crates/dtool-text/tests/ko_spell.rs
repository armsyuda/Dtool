use dtool_text::spelling::{checkable, script, words, Script, Speller};

#[test]
fn hangul_eojeol_is_checkable_and_tokenized_by_spaces() {
    assert_eq!(script("학교에서"), Some(Script::Hangul));
    assert_eq!(script("received"), Some(Script::Latin));
    assert_eq!(script("don't"), Some(Script::Latin));
    assert_eq!(script("2023년"), None);
    assert_eq!(script("학교abc"), None);
    assert_eq!(script("ㅋㅋ"), None);
    assert!(checkable("사람이"));
    let found: Vec<String> = words("나는 학교에서, 친구를 만났다.").into_iter().map(|w| w.word).collect();
    assert_eq!(found, vec!["나는", "학교에서", "친구를", "만났다"]);
    let item = &words("I 학교엣서 go")[1];
    assert_eq!((item.start, item.end), (2, 6));
}

#[test]
fn english_checking_is_unchanged() {
    let speller = Speller::en_us().expect("en_US");
    assert!(speller.is_correct("received"));
    assert!(!speller.is_correct("recieved"));
    assert!(speller.suggest("recieved").iter().any(|s| s == "received"));
    let marks: Vec<String> = speller.check_text("I recieved 2023년 it").into_iter().map(|m| m.word).collect();
    assert_eq!(marks, vec!["recieved"]);
}

#[test]
fn bundled_korean_dictionary_ships_with_its_license() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/hunspell/ko");
    let aff = std::fs::read_to_string(dir.join("ko.aff")).unwrap();
    let dic = std::fs::read_to_string(dir.join("ko.dic")).unwrap();
    assert!(aff.contains("VERSION hunspell-dict-ko 0.7.94"));
    assert!(aff.contains("FLAG num"));
    assert!(dic.lines().next().and_then(|l| l.trim().parse::<usize>().ok()).unwrap_or(0) > 100_000);
    let license = std::fs::read_to_string(dir.join("LICENSE.md")).unwrap();
    assert!(license.contains("GNU General Public License"));
    assert!(std::fs::read_to_string(dir.join("LICENSE.GPL-3")).unwrap().contains("GNU GENERAL PUBLIC LICENSE"));
    assert!(dir.join("README.md").exists());
}