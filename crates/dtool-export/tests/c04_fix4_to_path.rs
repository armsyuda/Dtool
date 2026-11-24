use dtool_core::document::Units;
use dtool_core::geometry::object_world_bounds;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::text_outline_curve;
use dtool_core::{Command, Document, Session};
use dtool_text::FontLibrary;
use serde_json::json;

fn setup(deg: f32) -> Option<(Session, ObjectId)> {
    assert!(dtool_text::canvas_text::install());
    let lib = FontLibrary::shared();
    let families = lib.families();
    let family = families.iter().find(|f| f.eq_ignore_ascii_case("Arial")).or(families.first())?.to_string();
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = ObjectId(s.run(&Command::with_params("text.art.add", json!({"pos": [80.0, 100.0], "text": "Hiy", "size_pt": 40.0}))).unwrap()["id"].as_u64().unwrap());
    let story = match s.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    s.document.stories.iter_mut().find(|x| x.id == story).unwrap().ranges = vec![json!({"start": 0, "end": 3, "family": family, "size_pt": 40.0})];
    s.document.find_object_mut(id.0).unwrap().rotation_deg = deg;
    Some((s, id))
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-2)
}

fn convert_matches_outline(deg: f32) {
    let Some((mut s, id)) = setup(deg) else { return };
    let (pos, size, _) = text_outline_curve(&s.document, id).unwrap();
    let expected = [pos[0], pos[1], size[0], size[1]];
    let before = serde_json::to_value(s.document.get_object(id).unwrap()).unwrap();
    s.run(&Command::with_params("text.to_path", json!({"id": id.0}))).unwrap();
    let curve = s.document.get_object(id).unwrap();
    assert!(matches!(curve.kind, ObjectKind::Curve(_)));
    assert_eq!(curve.rotation_deg, 0.0);
    assert!(near(object_world_bounds(curve), expected), "{:?} {expected:?}", object_world_bounds(curve));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(serde_json::to_value(s.document.get_object(id).unwrap()).unwrap(), before);
    s.run(&Command::new("edit.redo")).unwrap();
    assert!(near(object_world_bounds(s.document.get_object(id).unwrap()), expected));
}

#[test]
fn rotated_text_to_path_is_not_rotated_twice_and_undo_restores_rotation() {
    convert_matches_outline(90.0);
    convert_matches_outline(35.0);
}

#[test]
fn unrotated_text_to_path_keeps_the_outline_box() {
    convert_matches_outline(0.0);
}