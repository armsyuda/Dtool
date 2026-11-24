use dtool_core::document::Units;
use dtool_core::serde_json::json;
use dtool_core::{Command, Document, ObjectId, Session};
use dtool_text::spelling::ko_grammar;

fn kiwi_or_skip() -> bool {
    if ko_grammar::kiwi_dir().is_some() {
        return true;
    }
    assert!(std::env::var_os("DTOOL_KIWI_REQUIRED").is_none(), "Kiwi bundle missing in {:?}", ko_grammar::candidate_dirs());
    eprintln!("KIWI_SKIP bundle not found in {:?}", ko_grammar::candidate_dirs());
    false
}

fn story_text(session: &Session, story: dtool_core::StoryId) -> String {
    session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

#[test]
fn spell_check_route_reports_korean_issues_and_replace_applies_them() {
    if !kiwi_or_skip() {
        return;
    }
    assert!(dtool_text::text_host::install());
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": "아버지가방에들어가신다"}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    let version = session.document.version;
    let out = session.run(&Command::with_params("text.spell.check", json!({"story": story.0}))).unwrap();
    assert_eq!(session.document.version, version);
    assert_eq!(out["grammar_available"], json!(true), "{out}");
    let issues = out["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1, "{out}");
    let issue = &issues[0];
    assert_eq!(issue["kind"], json!("spacing"));
    assert_eq!(issue["label_key"], json!("spell.issue.spacing"));
    assert_eq!((issue["start"].as_u64(), issue["end"].as_u64()), (Some(0), Some(11)));
    assert_eq!(issue["suggestion"], json!("아버지가 방에 들어가신다"));
    assert!(out["marks"].as_array().unwrap().iter().all(|m| m["word"] != json!("아버지가방에들어가신다")), "{out}");
    let off = session.run(&Command::with_params("text.spell.check", json!({"story": story.0, "grammar": false}))).unwrap();
    assert!(off["issues"].as_array().unwrap().is_empty());
    session.run(&Command::with_params("text.spell.replace", json!({"story": story.0, "start": issue["start"], "end": issue["end"], "word": issue["word"], "with": issue["suggestion"]}))).unwrap();
    assert_eq!(story_text(&session, story), "아버지가 방에 들어가신다");
    let again = session.run(&Command::with_params("text.spell.check", json!({"story": story.0}))).unwrap();
    assert!(again["issues"].as_array().unwrap().is_empty(), "{again}");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&session, story), "아버지가방에들어가신다");
}

#[test]
fn spell_check_route_reports_grammar_hint() {
    if !kiwi_or_skip() {
        return;
    }
    assert!(dtool_text::text_host::install());
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": "정말 잘했따"}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    let out = session.run(&Command::with_params("text.spell.check", json!({}))).unwrap();
    let issues = out["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1, "{out}");
    assert_eq!(issues[0]["story"], json!(story.0));
    assert_eq!(issues[0]["kind"], json!("grammar"));
    assert_eq!((issues[0]["start"].as_u64(), issues[0]["end"].as_u64()), (Some(3), Some(6)));
    assert_eq!(issues[0]["suggestion"], json!("잘했다"));
}
