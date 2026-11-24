use dtool_core::document::{Document, Units};
use dtool_core::commands::text_host::{clear_recent_fonts, push_recent_font, recent_fonts, RECENT_FONTS_MAX};
use dtool_core::{Command, Session, Story, StoryId};
use serde_json::json;

#[test]
fn text_style_family_pushes_app_global_recent_fonts() {
    clear_recent_fonts();
    let mut doc = Document::new([400.0, 300.0], Units::Px, 72.0, 1);
    let mut story = Story { id: StoryId(2), text: "Hello".into(), ..Default::default() };
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = 5u64.into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
    doc.stories.push(story);
    let mut session = Session::new(doc);
    session.run(&Command::with_params("text.style", json!({ "story": 2, "range": { "start": 0, "end": 5 }, "patch": { "family": "Arial" } }))).unwrap();
    session.run(&Command::with_params("text.style", json!({ "story": 2, "range": { "start": 0, "end": 2 }, "patch": { "family": "Malgun Gothic" } }))).unwrap();
    session.run(&Command::with_params("text.style", json!({ "story": 2, "range": { "start": 0, "end": 2 }, "patch": { "family": "arial" } }))).unwrap();
    assert_eq!(recent_fonts(), vec!["arial".to_string(), "Malgun Gothic".to_string()]);
    assert!(session.run(&Command::with_params("text.style", json!({ "story": 99, "range": { "start": 0, "end": 1 }, "patch": { "family": "Nope" } }))).is_err());
    assert!(!recent_fonts().iter().any(|f| f == "Nope"));
    for i in 0..(RECENT_FONTS_MAX + 3) { push_recent_font(&format!("F{i}")); }
    assert_eq!(recent_fonts().len(), RECENT_FONTS_MAX);
    assert_eq!(recent_fonts()[0], format!("F{}", RECENT_FONTS_MAX + 2));
}