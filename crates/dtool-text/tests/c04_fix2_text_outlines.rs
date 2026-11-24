use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::text_outline_curve;
use dtool_core::{Command, Document, Session};
use dtool_text::FontLibrary;

fn family() -> Option<String> {
    let lib = FontLibrary::shared();
    let families = lib.families();
    families.iter().find(|f| f.eq_ignore_ascii_case("Arial")).or(families.first()).map(|f| f.to_string())
}

fn art(session: &mut Session, text: &str, family: Option<&str>) -> ObjectId {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0f64, 20.0].into();
    add.params["text"] = text.into();
    add.params["size_pt"] = 40.0.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    if let Some(family) = family {
        let story = match session.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
        let story = session.document.stories.iter_mut().find(|s| s.id == story).unwrap();
        for range in story.ranges.iter_mut() {
            range["family"] = family.into();
        }
        if story.ranges.is_empty() {
            let mut range = Command::new("range").params;
            range["start"] = 0.into();
            range["end"] = (text.chars().count() as u64).into();
            range["family"] = family.into();
            story.ranges.push(range);
        }
    }
    id
}

#[test]
fn art_text_a_has_real_glyph_contours_near_the_text() {
    assert!(dtool_text::canvas_text::install());
    let Some(family) = family() else { return };
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut session, "A", Some(&family));
    let (pos, size, curve) = text_outline_curve(&session.document, id).unwrap();
    assert!(curve.contour.points.len() >= 3);
    assert!(curve.contour.closed);
    assert!(!curve.holes.is_empty());
    assert!(size[0] > 5.0 && size[1] > 10.0, "{size:?}");
    assert!(pos[0] >= 10.0 - 1.0 && pos[1] >= 20.0 - 1.0, "{pos:?}");
    assert!(pos[1] + size[1] <= 20.0 + 60.0, "{pos:?} {size:?}");
    let b = curve.contour.bounds();
    assert!(b[0] >= -0.01 && b[1] >= -0.01);
}

#[test]
fn empty_and_unknown_cases_are_explicit_errors() {
    assert!(dtool_text::canvas_text::install());
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let blank = art(&mut session, "   ", None);
    assert_eq!(text_outline_curve(&session.document, blank).unwrap_err().code, "TEXT_EMPTY");
    assert_eq!(text_outline_curve(&session.document, ObjectId(999)).unwrap_err().code, "NOT_FOUND");
    let shape = session.document.create_object(dtool_core::object::Object::default());
    assert!(text_outline_curve(&session.document, shape).is_err());
}

#[test]
fn missing_font_or_no_provider_is_an_explicit_error() {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut session, "A", Some("No Such Family 0x5f"));
    let none = dtool_core::text_lines::text_outline_from(&session.document, id, |_, _| Some(Vec::new()));
    assert_eq!(none.unwrap_err().code, "FONT_OUTLINE_MISSING");
    let degenerate = dtool_core::text_lines::text_outline_from(&session.document, id, |_, _| Some(vec![vec![[0.0, 0.0], [1.0, 1.0]]]));
    assert_eq!(degenerate.unwrap_err().code, "FONT_OUTLINE_MISSING");
    let unavailable = dtool_core::text_lines::text_outline_from(&session.document, id, |_, _| None);
    assert_eq!(unavailable.unwrap_err().code, "TEXT_OUTLINES_UNAVAILABLE");
}
