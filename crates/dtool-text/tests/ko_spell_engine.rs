use dtool_core::document::Units;
use dtool_core::serde_json::{json, Value};
use dtool_core::{Command, Document, ObjectId, Session, StoryId};
use dtool_text::frame::layout_doc_flow;
use dtool_text::spell_live::{story_underlines, underlines_in_lines, LiveSpell};
use dtool_text::spelling::Speller;
use dtool_text::text_host::run;
use dtool_text::FontLibrary;
use std::time::{Duration, Instant};

const KO_FONTS: [&str; 4] = ["Malgun Gothic", "맑은 고딕", "Noto Sans KR", "Noto Sans CJK KR"];

fn live(text: &str) -> (Session, StoryId, ObjectId) {
    let mut session = Session::new(Document::new([800.0, 800.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [30, 40, 700, 700], "text": text}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    (session, story, ObjectId(frame))
}

#[test]
fn correct_eojeol_with_josa_pass() {
    let speller = Speller::en_us().expect("en_US");
    for word in ["학교", "학교에서", "사람이", "먹었다", "갔습니다", "안녕하세요", "아름다운", "사람들"] {
        assert!(speller.is_correct(word), "{word}");
    }
    assert!(speller.is_correct("received"));
    assert!(!speller.is_correct("recieved"));
}

#[test]
fn misspelled_eojeol_is_flagged_with_suggestions() {
    let speller = Speller::en_us().expect("en_US");
    assert!(!speller.is_correct("학교엣서"));
    assert!(speller.suggest("학교엣서").iter().any(|s| s == "학교에서"));
    assert!(!speller.is_correct("먹었닿"));
    assert!(speller.suggest("먹었닿").iter().any(|s| s == "먹었다"));
    let marks: Vec<(usize, usize, String)> = speller.check_text("나는 학교엣서 recieved 책을 읽었다").into_iter().map(|m| (m.start, m.end, m.word)).collect();
    assert_eq!(marks, vec![(3, 7, "학교엣서".to_string()), (8, 16, "recieved".to_string())]);
}

#[test]
fn dictionary_only_commands_mark_and_suggest_korean() {
    let (mut session, story, _) = live("나는 학교엣서 공부했다");
    let (version, steps) = (session.document.version, session.undo.applied_steps());
    let mut host = |name: &str, params: Value| run(&mut session, &Command::with_params(name, params)).expect("routed").unwrap();
    let reply = host("text.spell.check", json!({"story": story.0, "grammar": false}));
    assert_eq!(reply["marks"], json!([{"story": story.0, "start": 3, "end": 7, "word": "학교엣서"}]));
    let sugg = host("text.spell.suggest", json!({"word": "학교엣서"}));
    assert_eq!(sugg["correct"], json!(false));
    assert!(sugg["suggestions"].as_array().unwrap().iter().any(|s| s == "학교에서"));
    let replaced = host("text.spell.replace", json!({"story": story.0, "start": 3, "end": 7, "word": "학교엣서", "with": "학교에서"}));
    let _ = replaced;
    let after = host("text.spell.check", json!({"story": story.0, "grammar": false}));
    assert_eq!(after["marks"], json!([]));
    assert_eq!(session.undo.applied_steps(), steps + 1);
    assert!(session.document.version > version);
}

#[test]
fn dictionary_only_live_underline_marks_korean_typo_in_the_frame() {
    let lib = FontLibrary::system();
    let have: Vec<String> = lib.families().into_iter().map(|s| s.to_string()).collect();
    let Some(family) = KO_FONTS.iter().find_map(|p| have.iter().find(|h| h.eq_ignore_ascii_case(p)).cloned()) else { eprintln!("skip: no Korean font"); return };
    let text = "나는 학교엣서 공부했다";
    let (mut session, story, frame) = live(text);
    session.run(&Command::with_params("text.style", json!({"story": story.0, "range": {"start": 0, "end": text.chars().count()}, "patch": {"family": family, "size_pt": 18.0}}))).unwrap();
    let version = session.document.version;
    let mut spell = LiveSpell::en_us().with_settle(Duration::ZERO).with_grammar(false);
    let t0 = Instant::now();
    spell.note_edit(story, t0);
    spell.tick(&session, t0);
    let marks = spell.marks(story).to_vec();
    assert_eq!(marks.iter().map(|m| m.word.as_str()).collect::<Vec<_>>(), vec!["학교엣서"]);
    let lines: Vec<_> = layout_doc_flow(&lib, &session.document, story).expect("flow").layout.per_frame.into_iter().flatten().collect();
    let local = underlines_in_lines(&marks, &lines);
    assert_eq!(local.len(), 1);
    assert!(local[0].x1 > local[0].x0 + 10.0, "{:?}", local[0]);
    let world = story_underlines(&lib, &session.document, story, &marks);
    assert_eq!(world.len(), 1);
    assert_eq!(world[0].0, frame);
    let (s, w) = spell.suggestions_at(story, 4).expect("suggestions at the mark");
    assert_eq!(s.word, "학교엣서");
    assert!(w.iter().any(|x| x == "학교에서"));
    spell.ignore(&session, "학교엣서");
    assert!(spell.marks(story).is_empty());
    assert_eq!(session.document.version, version);
}