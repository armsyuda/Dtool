use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::{font_generation, text_lines};
use dtool_core::{Command, Document, Session, StoryId};
use dtool_text::font_preview::{font_preview, preview_document, previewed_core_story};
use dtool_text::FontLibrary;

fn art(session: &mut Session, text: &str, family: &str) -> (ObjectId, u64) {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0f64, 20.0].into();
    add.params["text"] = text.into();
    add.params["size_pt"] = 40.0.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    let story = match session.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    let core = session.document.stories.iter_mut().find(|s| s.id == story).unwrap();
    if core.ranges.is_empty() {
        let mut range = Command::new("range").params;
        range["start"] = 0.into();
        range["end"] = (text.chars().count() as u64).into();
        range["size_pt"] = 40.0.into();
        core.ranges.push(range);
    }
    for range in core.ranges.iter_mut() {
        range["family"] = family.into();
    }
    (id, story.0)
}

fn width(session: &Session, id: ObjectId) -> f32 {
    text_lines(&session.document, id).iter().map(|l| l.width).sum()
}

fn preview(session: &mut Session, story: u64, start: u64, end: u64, family: &str) -> Result<dtool_core::serde_json::Value, dtool_core::CommandError> {
    let mut cmd = Command::new("text.font.preview");
    cmd.params["story"] = story.into();
    cmd.params["range"]["start"] = start.into();
    cmd.params["range"]["end"] = end.into();
    cmd.params["family"] = family.into();
    session.run(&cmd)
}

// One test: the preview slot is process-global.
#[test]
fn c10_preview_overlay_relayouts_live_text_and_cancel_restores() {
    assert!(dtool_text::canvas_text::install());
    dtool_text::text_host::install();
    let lib = FontLibrary::shared();
    let families: Vec<String> = lib.families().into_iter().map(str::to_owned).collect();
    if families.len() < 2 {
        return;
    }
    let mut session = Session::new(Document::new([800.0, 400.0], Units::Px, 72.0, 1));
    let listed = session.run(&Command::new("font.list")).unwrap();
    assert_eq!(listed["families"].as_array().unwrap().len(), families.len());
    assert!(listed.get("recent").is_some());

    let text = "Hello preview WWW mmm";
    let n = text.chars().count() as u64;
    let (id, story) = art(&mut session, text, &families[0]);
    let base = width(&session, id);
    assert!(base > 0.0);
    let version = session.document.version;
    let generation = session.generation;
    let stories = format!("{:?}", session.document.stories);

    let mut changed = None;
    for family in &families[1..] {
        let before = font_generation();
        let reply = preview(&mut session, story, 0, n, family).unwrap();
        assert_eq!(reply["preview"]["family"], family.as_str());
        assert!(font_generation() > before, "preview must invalidate display lists");
        let shown = width(&session, id);
        if (shown - base).abs() > 0.5 {
            changed = Some((family.clone(), shown));
            break;
        }
    }
    let Some((family, shown)) = changed else { return };
    assert_ne!(shown, base, "{family}");
    eprintln!("c10 preview checked: {} -> {family} width {base} -> {shown}", families[0]);
    assert_eq!(session.document.version, version);
    assert_eq!(session.generation, generation);
    assert_eq!(format!("{:?}", session.document.stories), stories);

    // Another tab with the same story id never sees this preview.
    let mut other = Session::new(Document::new([800.0, 400.0], Units::Px, 72.0, 1));
    let (other_id, other_story) = art(&mut other, text, &families[0]);
    assert_eq!(other_story, story);
    assert_ne!(other.document.instance, session.document.instance);
    assert!(preview_document(&other.document, StoryId(story)).is_none());
    assert!((width(&other, other_id) - base).abs() < 0.01);
    let reopened: Document = dtool_core::serde_json::from_value(dtool_core::serde_json::to_value(&session.document).unwrap()).unwrap();
    assert!(preview_document(&reopened, StoryId(story)).is_none(), "reopened copy is another document");
    let snapshot = session.document.clone();
    assert!(preview_document(&snapshot, StoryId(story)).is_some(), "render clone keeps identity");

    let view = preview_document(&session.document, StoryId(story)).expect("preview view");
    let patched = previewed_core_story(&session.document, session.document.stories.iter().find(|s| s.id.0 == story).unwrap()).unwrap();
    assert_eq!(view.stories.iter().find(|s| s.id.0 == story).unwrap(), &patched);
    assert!(patched.ranges.iter().all(|r| r["family"] == family.as_str() && r["size_pt"] == 40.0));

    preview(&mut session, story, 0, 5, &family).unwrap();
    let split = previewed_core_story(&session.document, session.document.stories.iter().find(|s| s.id.0 == story).unwrap()).unwrap();
    let head = split.ranges.iter().find(|r| r["start"] == 0 && r["end"] == 5).expect("head run");
    assert_eq!((head["family"].as_str(), head["size_pt"].as_f64()), (Some(family.as_str()), Some(40.0)));
    let tail = split.ranges.iter().find(|r| r["start"] == 5).expect("tail run");
    assert_eq!((tail["family"].as_str(), tail["end"].as_u64()), (Some(families[0].as_str()), Some(n)));
    assert!(preview_document(&session.document, StoryId(story + 1000)).is_none());

    let before = font_generation();
    session.run(&Command::new("text.font.preview.cancel")).unwrap();
    assert!(font_preview().is_none());
    assert!(font_generation() > before);
    assert!((width(&session, id) - base).abs() < 0.01);
    assert!(preview_document(&session.document, StoryId(story)).is_none());
    assert_eq!(session.document.version, version);
    assert_eq!(format!("{:?}", session.document.stories), stories);
}