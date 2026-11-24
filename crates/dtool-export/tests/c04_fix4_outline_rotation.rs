use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::text_outline_curve;
use dtool_core::{Command, Document, Session};
use dtool_text::canvas_text::text_outlines_with;
use dtool_text::FontLibrary;
use serde_json::json;

fn family() -> Option<String> {
    let lib = FontLibrary::shared();
    let families = lib.families();
    families.iter().find(|f| f.eq_ignore_ascii_case("Arial")).or(families.first()).map(|f| f.to_string())
}

fn style(session: &mut Session, id: ObjectId, family: &str) {
    let story = dtool_core::frame::frame_story(&session.document, id).or_else(|| match session.document.get_object(id).unwrap().kind {
        ObjectKind::ArtText { story } => Some(story),
        _ => None,
    }).unwrap();
    let story = session.document.stories.iter_mut().find(|s| s.id == story).unwrap();
    story.ranges = vec![json!({"start": 0, "end": story.text.chars().count(), "family": family, "size_pt": 40.0})];
}

fn art(session: &mut Session, text: &str, family: &str) -> ObjectId {
    let id = ObjectId(session.run(&Command::with_params("text.art.add", json!({"pos": [60.0, 80.0], "text": text, "size_pt": 40.0}))).unwrap()["id"].as_u64().unwrap());
    style(session, id, family);
    id
}

fn frame(session: &mut Session, text: &str, family: &str) -> ObjectId {
    let id = ObjectId(session.run(&Command::with_params("text.frame.add", json!({"rect": [40, 60, 240, 120], "text": text}))).unwrap()["id"].as_u64().unwrap());
    style(session, id, family);
    id
}

fn rotate(session: &mut Session, id: ObjectId, deg: f32) {
    session.document.find_object_mut(id.0).unwrap().rotation_deg = deg;
}

fn rotated(points: &[Vec<[f32; 2]>], centre: [f32; 2], deg: f32) -> Vec<Vec<[f32; 2]>> {
    let (sin, cos) = deg.to_radians().sin_cos();
    points.iter().map(|c| c.iter().map(|p| {
        let [x, y] = [p[0] - centre[0], p[1] - centre[1]];
        [centre[0] + cos * x - sin * y, centre[1] + sin * x + cos * y]
    }).collect()).collect()
}

fn bounds(contours: &[Vec<[f32; 2]>]) -> [f32; 4] {
    contours.iter().flatten().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
}

fn close(a: &[Vec<[f32; 2]>], b: &[Vec<[f32; 2]>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| (p[0] - q[0]).abs() < 1e-3 && (p[1] - q[1]).abs() < 1e-3))
}

fn check_rotation(session: &mut Session, id: ObjectId) {
    let lib = FontLibrary::shared();
    let plain = text_outlines_with(&lib, &session.document, id).unwrap();
    assert!(!plain.is_empty());
    let centre = dtool_core::geometry::box_centre(session.document.get_object(id).unwrap());
    rotate(session, id, 90.0);
    let turned = text_outlines_with(&lib, &session.document, id).unwrap();
    assert!(close(&turned, &rotated(&plain, centre, 90.0)));
    let (p, t) = (bounds(&plain), bounds(&turned));
    assert!(((t[2] - t[0]) - (p[3] - p[1])).abs() < 1e-2 && ((t[3] - t[1]) - (p[2] - p[0])).abs() < 1e-2, "{p:?} {t:?}");
    assert!(dtool_text::canvas_text::install());
    let (pos, size, _) = text_outline_curve(&session.document, id).unwrap();
    assert!((pos[0] - t[0]).abs() < 1e-2 && (pos[1] - t[1]).abs() < 1e-2, "{pos:?} {t:?}");
    assert!((size[0] - (t[2] - t[0])).abs() < 1e-2 && (size[1] - (t[3] - t[1])).abs() < 1e-2);
    rotate(session, id, 0.0);
    assert_eq!(text_outlines_with(&lib, &session.document, id).unwrap(), plain);
}

#[test]
fn art_text_rotated_90_puts_outlines_in_the_rotated_world_box() {
    let Some(family) = family() else { return };
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut s, "Hiy", &family);
    check_rotation(&mut s, id);
}

#[test]
fn frame_text_rotated_90_puts_outlines_in_the_rotated_world_box() {
    let Some(family) = family() else { return };
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = frame(&mut s, "Frame", &family);
    check_rotation(&mut s, id);
}

#[test]
fn rotation_composes_with_typographic_scale_and_keeps_the_box_centre() {
    let Some(family) = family() else { return };
    let lib = FontLibrary::shared();
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut s, "W", &family);
    let story = match s.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    s.document.stories.iter_mut().find(|x| x.id == story).unwrap().ranges[0]["h_scale"] = 2.0.into();
    let plain = text_outlines_with(&lib, &s.document, id).unwrap();
    let centre = dtool_core::geometry::box_centre(s.document.get_object(id).unwrap());
    rotate(&mut s, id, 30.0);
    assert!(close(&text_outlines_with(&lib, &s.document, id).unwrap(), &rotated(&plain, centre, 30.0)));
}