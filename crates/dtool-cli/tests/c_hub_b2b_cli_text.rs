use dtool_core::{Story, StoryId, Workspace};
use serde_json::json;

#[test]
fn cli_font_list_and_text_find_are_answered_headless() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("b2b-cli");
    let session = &mut workspace.get_mut(id).unwrap().session;
    let mut story = Story { id: StoryId(5), text: "red green red".into(), ..Default::default() };
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = 13u64.into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
    session.document.stories.push(story);
    let shared = workspace.into_shared();
    let list = dtool_cli::run_cli_args(&shared, &["run".into(), "font.list".into(), "{}".into()]).unwrap();
    assert!(!list["families"].as_array().expect("families").is_empty());
    assert!(list["recent"].is_array());
    let found = dtool_cli::run_cli_args(&shared, &["text.find".into(), json!({ "pattern": "red", "backwards": true, "from": { "story": 5, "at": 13 } }).to_string()]).unwrap();
    assert_eq!(found["match"], json!({ "story": 5, "start": 10, "end": 13 }));
}