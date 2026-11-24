use dtool_core::document::{Document, Units};
use dtool_core::geometry::{apply_matrix, box_centre, object_world_quad};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::scene_hit::hit_test;
use dtool_core::view_state::CanvasFlags;
use dtool_render::scene::{build_scene, node_matrix};

#[test]
fn snap_corners_match_rendered_quad() {
    let object = Object { pos: [100.0, 80.0], size: [60.0, 40.0], rotation_deg: 30.0, shear_deg: 10.0, ..Object::default() };
    let m = node_matrix(&object);
    let local = [[100.0, 80.0], [160.0, 80.0], [160.0, 120.0], [100.0, 120.0]];
    for (corner, expected) in local.iter().zip(object_world_quad(&object).iter()) {
        let world = apply_matrix(&m, *corner);
        assert!((world[0] - expected[0]).abs() < 1e-3 && (world[1] - expected[1]).abs() < 1e-3);
    }
}

#[test]
fn hit_test_inside_rotated_object() {
    let mut doc = Document::new([400.0, 400.0], Units::Px, 96.0, 1);
    let id = doc.create_object(Object { pos: [100.0, 100.0], size: [200.0, 20.0], rotation_deg: 90.0, kind: ObjectKind::Shape, fill: dtool_core::paint::paint_from_legacy_str("red").unwrap(), ..Object::default() });
    let scene = build_scene(&doc, &CanvasFlags::default());
    let centre = box_centre(doc.get_object(id).unwrap());
    assert_eq!(hit_test(&scene, &doc, centre, 0.5), Some(id));
    assert_eq!(hit_test(&scene, &doc, [110.0, 105.0], 0.5), None);
}
