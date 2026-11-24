use dtool_core::document::{Document, Units};
use dtool_core::{Command, ObjectId, Session, StoryId};
use dtool_text::frame::layout_doc_flow;
use dtool_text::FontLibrary;
use serde_json::{json, Value};

const PREFERRED: [&str; 6] = ["Segoe UI", "Arial", "Calibri", "Times New Roman", "Noto Sans", "Liberation Serif"];

fn family(lib: &FontLibrary) -> Option<String> {
    let have: Vec<String> = lib.families().into_iter().map(|s| s.to_string()).collect();
    PREFERRED.iter().find_map(|p| have.iter().find(|h| h.eq_ignore_ascii_case(p)).cloned()).or_else(|| have.first().cloned())
}

fn session_with(text: &str, family: &str) -> (Session, StoryId) {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [50, 50, 500, 300], "text": text}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    let len = text.chars().count();
    run(&mut session, "text.style", json!({"story": story.0, "range": {"start": 0, "end": len}, "patch": {"family": family, "size_pt": 24.0}}));
    (session, story)
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap()
}

fn glyphs(lib: &FontLibrary, session: &Session, story: StoryId) -> Vec<(u16, usize)> {
    let flow = layout_doc_flow(lib, &session.document, story).expect("flow");
    flow.layout.per_frame.iter().flatten().flat_map(|line| line.glyphs.iter().map(|g| (g.gid, g.cluster))).collect()
}

#[test]
fn live_all_caps_style_shapes_uppercase_keeps_story_text_and_undoes() {
    let lib = FontLibrary::system();
    let Some(family) = family(&lib) else {
        eprintln!("skip: no font families");
        return;
    };
    let text = "official text";
    let (mut session, story) = session_with(text, &family);
    let plain = glyphs(&lib, &session, story);
    let (upper_session, upper_story) = session_with(&text.to_uppercase(), &family);
    let upper = glyphs(&lib, &upper_session, upper_story);
    run(&mut session, "text.case", json!({"story": story.0, "mode": "style", "op": "all_caps"}));
    let stored = session.document.stories.iter().find(|s| s.id == story).unwrap();
    assert_eq!(stored.text, text);
    let caps = glyphs(&lib, &session, story);
    let gids = |v: &[(u16, usize)]| v.iter().map(|g| g.0).collect::<Vec<_>>();
    assert_eq!(gids(&caps), gids(&upper));
    assert_ne!(gids(&caps), gids(&plain));
    let count = text.chars().count();
    assert!(caps.iter().all(|g| g.1 <= count));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(gids(&glyphs(&lib, &session, story)), gids(&plain));
}

#[test]
fn live_all_caps_on_width_changing_letters_keeps_clusters_in_range() {
    let lib = FontLibrary::system();
    let Some(family) = family(&lib) else {
        eprintln!("skip: no font families");
        return;
    };
    let text = "\u{17f}tra\u{df}e \u{131}i end";
    let (mut session, story) = session_with(text, &family);
    run(&mut session, "text.case", json!({"story": story.0, "mode": "style", "op": "all_caps"}));
    let laid = glyphs(&lib, &session, story);
    assert!(!laid.is_empty());
    let count = text.chars().count();
    let mut last = 0usize;
    for (_, cluster) in &laid {
        assert!(*cluster <= count, "{cluster} > {count}");
        assert!(*cluster >= last);
        last = *cluster;
    }
    assert_eq!(session.document.stories.iter().find(|s| s.id == story).unwrap().text, text);
}
