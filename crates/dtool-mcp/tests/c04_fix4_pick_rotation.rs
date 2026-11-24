use dtool_core::commands::colour_pick::PickMode;
use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::{Command, Document, Session};
use dtool_render::pick_sampler::pick;
use dtool_text::canvas_text::text_outlines_with;
use dtool_text::FontLibrary;
use serde_json::json;

fn setup() -> Option<(Session, ObjectId)> {
    assert!(dtool_render::scene::install());
    assert!(dtool_text::canvas_text::install());
    let lib = FontLibrary::shared();
    let families = lib.families();
    let family = families.iter().find(|f| f.eq_ignore_ascii_case("Arial")).or(families.first())?.to_string();
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = ObjectId(s.run(&Command::with_params("text.art.add", json!({"pos": [150.0, 150.0], "text": "I", "size_pt": 60.0}))).unwrap()["id"].as_u64().unwrap());
    let story = match s.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    s.document.stories.iter_mut().find(|x| x.id == story).unwrap().ranges = vec![json!({"start": 0, "end": 1, "family": family, "size_pt": 60.0})];
    Some((s, id))
}

fn bounds(contours: &[Vec<[f32; 2]>]) -> [f32; 4] {
    contours.iter().flatten().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
}

fn turn(p: [f32; 2], c: [f32; 2]) -> [f32; 2] {
    [c[0] - (p[1] - c[1]), c[1] + (p[0] - c[0])]
}

fn rgba(doc: &Document, p: [f32; 2]) -> [u8; 4] {
    let c = pick(doc, p[0], p[1], PickMode::Point, None).unwrap();
    [c.r, c.g, c.b, c.a]
}

#[test]
fn pick_follows_the_rotated_glyph_and_not_the_unrotated_one() {
    let Some((mut s, id)) = setup() else { return };
    let b = bounds(&text_outlines_with(&FontLibrary::shared(), &s.document, id).unwrap());
    let stem = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
    let top = [stem[0], b[1] + (b[3] - b[1]) * 0.1];
    let ink = rgba(&s.document, stem);
    assert_eq!(rgba(&s.document, top), ink);
    let paper = rgba(&s.document, [20.0, 20.0]);
    assert_ne!(ink, paper);
    let centre = dtool_core::geometry::box_centre(s.document.get_object(id).unwrap());
    s.document.find_object_mut(id.0).unwrap().rotation_deg = 90.0;
    let before = serde_json::to_string(&s.document).unwrap();
    assert_eq!(rgba(&s.document, turn(stem, centre)), ink);
    assert_eq!(rgba(&s.document, turn(top, centre)), ink);
    assert_eq!(rgba(&s.document, top), paper);
    assert_eq!(serde_json::to_string(&s.document).unwrap(), before);
}