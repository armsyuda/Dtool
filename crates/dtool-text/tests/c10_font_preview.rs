use dtool_core::{Command, Document};
use dtool_text::bridge::story_from_core;
use dtool_text::{fonts, story};
#[path = "../src/font_preview.rs"]
mod font_preview;
use font_preview::{cancel_font_preview, font_list, font_preview, previewed_story, run_font_command, FontReply, FONT_LIST, FONT_PREVIEW, FONT_PREVIEW_CANCEL};
use dtool_text::FontLibrary;
use std::path::PathBuf;

fn fonts() -> FontLibrary {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dtool-ui/assets/fonts");
    FontLibrary::from_fixtures(&[root.join("inter/Inter-Regular.ttf"), root.join("pretendard/Pretendard-Regular.ttf")]).expect("fixtures")
}

fn doc() -> Document {
    let mut doc = Document::new([200.0, 200.0], dtool_core::document::Units::Px, 72.0, 1);
    let mut story = dtool_core::Story { id: dtool_core::StoryId(5), text: "Hello world".into(), ..Default::default() };
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = 11u64.into();
    story.ranges[0]["family"] = "Inter".into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
    doc.stories.push(story);
    doc
}

fn command(name: &str, story: u64, start: u64, end: u64, family: &str) -> Command {
    let mut cmd = Command::new(name);
    cmd.params["story"] = story.into();
    cmd.params["range"]["start"] = start.into();
    cmd.params["range"]["end"] = end.into();
    cmd.params["family"] = family.into();
    cmd
}

#[test]
fn c10_font_list_returns_installed_families() {
    let lib = fonts();
    let families = font_list(&lib);
    assert!(families.iter().any(|f| f == "Inter") && families.iter().any(|f| f == "Pretendard"), "{families:?}");
    match run_font_command(&lib, &doc(), &Command::new(FONT_LIST)) {
        Some(Ok(FontReply::List { families: listed })) => assert_eq!(listed, families),
        other => panic!("{other:?}"),
    }
    assert!(run_font_command(&lib, &doc(), &Command::new("text.style")).is_none());
}

#[test]
fn c10_preview_then_cancel_leaves_document_untouched() {
    let lib = fonts();
    let doc = doc();
    let version = doc.version;
    let before = format!("{:?}", doc.stories);
    let core = doc.stories[0].clone();

    let bad = run_font_command(&lib, &doc, &command(FONT_PREVIEW, 5, 0, 5, "No Such Font")).unwrap();
    assert!(bad.is_err());
    assert!(run_font_command(&lib, &doc, &command(FONT_PREVIEW, 99, 0, 5, "Pretendard")).unwrap().is_err());
    assert!(run_font_command(&lib, &doc, &command(FONT_PREVIEW, 5, 3, 50, "Pretendard")).unwrap().is_err());
    let mut missing = Command::new(FONT_PREVIEW);
    missing.params["story"] = 5u64.into();
    assert!(run_font_command(&lib, &doc, &missing).unwrap().is_err());

    match run_font_command(&lib, &doc, &command(FONT_PREVIEW, 5, 0, 5, "pretendard")).unwrap() {
        Ok(FontReply::Preview { preview: Some(p) }) => assert_eq!((p.story, p.start, p.end, p.family.as_str()), (5, 0, 5, "Pretendard")),
        other => panic!("{other:?}"),
    }
    let shown = previewed_story(&story_from_core(&core));
    let first = shown.chars.iter().find(|run| run.range.start == 0).unwrap();
    assert_eq!(first.style.family, "Pretendard");
    assert_eq!(first.range.end, 5);
    assert!(shown.chars.iter().any(|run| run.range.start >= 5 && run.style.family == "Inter"));
    assert_eq!(doc.version, version);
    assert_eq!(format!("{:?}", doc.stories), before);
    assert_eq!(story_from_core(&doc.stories[0]).chars.iter().all(|run| run.style.family == "Inter"), true);

    match run_font_command(&lib, &doc, &Command::new(FONT_PREVIEW_CANCEL)).unwrap() {
        Ok(FontReply::Preview { preview: None }) => {}
        other => panic!("{other:?}"),
    }
    assert!(font_preview().is_none());
    assert!(!cancel_font_preview());
    assert_eq!(previewed_story(&story_from_core(&core)), story_from_core(&core));
    assert_eq!(doc.version, version);
    assert_eq!(format!("{:?}", doc.stories), before);
}