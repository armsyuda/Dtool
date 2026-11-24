use dtool_core::document::Units;
use dtool_core::serde_json::json;
use dtool_core::{Command, Document, ObjectId, Session, StoryId};
use dtool_text::spell_live::LiveSpell;
use dtool_text::spelling::ko_grammar::{self, IssueKind};
use std::time::{Duration, Instant};

fn kiwi_or_skip() -> bool {
    if ko_grammar::kiwi_dir().is_some() {
        return true;
    }
    assert!(std::env::var_os("DTOOL_KIWI_REQUIRED").is_none(), "Kiwi bundle missing in {:?}", ko_grammar::candidate_dirs());
    eprintln!("KIWI_SKIP bundle not found in {:?}", ko_grammar::candidate_dirs());
    false
}

fn add_frame(session: &mut Session, text: &str) -> StoryId {
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": text}))).unwrap()["id"].as_u64().unwrap();
    dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap()
}

#[test]
fn english_never_loads_kiwi_and_korean_live_issues_arrive_after_warm() {
    if !kiwi_or_skip() {
        return;
    }
    assert!(dtool_text::text_host::install());
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let english = add_frame(&mut session, "I recieved it");
    let mut live = LiveSpell::en_us().with_settle(Duration::ZERO);
    live.note_edit(english, Instant::now());
    live.tick(&session, Instant::now());
    assert_eq!(live.marks(english).len(), 1);
    session.run(&Command::with_params("text.spell.check", json!({}))).unwrap();
    assert!(!ko_grammar::kiwi_loaded(), "English text must not load Kiwi");

    let korean = add_frame(&mut session, "우리는내일만나기로했다");
    live.note_edit(korean, Instant::now());
    live.tick(&session, Instant::now());
    let deadline = Instant::now() + Duration::from_secs(120);
    while live.issues(korean).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        live.tick(&session, Instant::now());
    }
    assert!(ko_grammar::kiwi_ready(), "{:?}", ko_grammar::load_error());
    let issues = live.issues(korean).to_vec();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].kind, IssueKind::Spacing);
    assert_eq!(issues[0].suggestion, "우리는 내일 만나기로 했다");
    assert!(live.marks(korean).iter().all(|m| !issues[0].covers(m.start, m.end)));
    assert_eq!(live.issue_marks(korean).len(), 1);
    let (mark, list) = live.suggestions_at(korean, 2).unwrap();
    assert_eq!((mark.start, mark.end), (0, 11));
    assert_eq!(list, vec!["우리는 내일 만나기로 했다".to_string()]);
    live.replace_at(&mut session, korean, 2, &list[0]).unwrap();
    assert_eq!(session.document.stories.iter().find(|s| s.id == korean).unwrap().text, "우리는 내일 만나기로 했다");
    assert!(live.issues(korean).is_empty());
    assert_eq!(live.marks(english).len(), 1);
}
