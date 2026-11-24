use dtool_core::document::{Document, Units};
use dtool_core::geometry::{apply_matrix, object_matrix, object_world_bounds, object_world_quad};
use dtool_core::grid::Grid;
use dtool_core::guides::Guides;
use dtool_core::object::{Object, ObjectId};
use dtool_core::slice::source_bounds;
use dtool_core::snap_candidates::{snap_candidates, CandidateKind, SnapDocument};
use dtool_core::snap_prefs::SnapPrefs;
use dtool_core::table::geometry::to_local;
use dtool_core::ViewState;
use serde_json::json;

fn skewed_doc() -> (Document, ObjectId) {
    let mut doc = Document::new([400.0, 400.0], Units::Px, 96.0, 1);
    let id = doc.create_object(Object { pos: [100.0, 80.0], size: [60.0, 40.0], rotation_deg: 30.0, shear_deg: 10.0, ..Object::default() });
    (doc, id)
}

fn corner_prefs() -> SnapPrefs {
    let mut value = serde_json::to_value(SnapPrefs::default()).unwrap();
    for field in value.as_object_mut().unwrap().values_mut() {
        if field.is_boolean() { *field = json!(false); }
    }
    value["enabled"] = json!(true);
    value["object_snapping"] = json!(true);
    value["snap_to_shape_key_points"] = json!(true);
    value["screen_tolerance"] = json!(2);
    value["max_candidates"] = json!(64);
    serde_json::from_value(value).unwrap()
}

#[test]
fn snap_corners_match_object_matrix_quad() {
    let (doc, id) = skewed_doc();
    let object = doc.get_object(id).unwrap().clone();
    let m = object_matrix(&object);
    let local = [[100.0, 80.0], [160.0, 80.0], [160.0, 120.0], [100.0, 120.0]];
    let quad = object_world_quad(&object);
    let guides = Guides::default();
    let grid = Grid::default();
    let snap = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    let view = ViewState::default();
    for (corner, expected) in local.iter().zip(quad.iter()) {
        let world = apply_matrix(&m, *corner);
        assert!((world[0] - expected[0]).abs() < 1e-3 && (world[1] - expected[1]).abs() < 1e-3);
        let found = snap_candidates(&snap, &view, &corner_prefs(), world);
        assert!(found.iter().any(|c| c.kind == CandidateKind::ObjectGeometry && (c.point[0] - world[0]).abs() < 1e-3 && (c.point[1] - world[1]).abs() < 1e-3), "{world:?}");
    }
}

#[test]
fn slice_bounds_equal_world_bounds() {
    let (doc, id) = skewed_doc();
    let bounds = source_bounds(&doc, id).unwrap();
    let [x, y, w, h] = object_world_bounds(doc.get_object(id).unwrap());
    let expected = [x, y, x + w, y + h];
    assert!(bounds.iter().zip(expected.iter()).all(|(a, b)| (a - b).abs() < 1e-3), "{bounds:?} {expected:?}");
}

#[test]
fn table_to_local_singular_is_none() {
    assert_eq!(to_local([0.0, 0.0, 0.0, 0.0, 5.0, 5.0], [1.0, 1.0]), None);
    assert_eq!(to_local([f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0], [1.0, 1.0]), None);
    assert_eq!(to_local([1.0, 0.0, 0.0, 1.0, 10.0, 20.0], [11.0, 22.0]), Some([1.0, 2.0]));
}
