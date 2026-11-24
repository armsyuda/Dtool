use dtool_core::document::{Document, Units};
use dtool_core::geometry::{apply_matrix, box_centre, doc_to_object_local, invert_matrix, object_matrix, object_world_bounds, object_world_quad};
use dtool_core::object::{Object, ObjectId};
use dtool_core::view::zoom_about;
use dtool_core::Session;
use serde_json::json;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3
}

fn bounds_close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| close(*x, *y))
}

fn session_with_rect() -> (Session, ObjectId) {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    let id = session.document.create_object(Object { pos: [10.0, 10.0], size: [100.0, 50.0], ..Object::default() });
    (session, id)
}

#[test]
fn rotation_pivots_at_centre() {
    let object = Object { pos: [10.0, 10.0], size: [100.0, 50.0], rotation_deg: 90.0, ..Object::default() };
    let centre = apply_matrix(&object_matrix(&object), box_centre(&object));
    assert!(close(centre[0], 60.0) && close(centre[1], 35.0));
    assert!(bounds_close(object_world_bounds(&object), [35.0, -15.0, 50.0, 100.0]));
}

#[test]
fn centre_flip_keeps_world_bounds() {
    let (mut session, id) = session_with_rect();
    session.run_named("transform.flip", json!({"ids": [id.0], "axis": "Horizontal", "anchor": "Centre"})).unwrap();
    let object = session.document.get_object(id).unwrap();
    assert_eq!(object.size, [-100.0, 50.0]);
    assert!(bounds_close(object_world_bounds(object), [10.0, 10.0, 100.0, 50.0]));
}

#[test]
fn rotate_90_keeps_size_and_centre() {
    let (mut session, id) = session_with_rect();
    session.run_named("transform.rotate_90", json!({"ids": [id.0], "direction": "Left"})).unwrap();
    let object = session.document.get_object(id).unwrap();
    assert_eq!(object.size, [100.0, 50.0]);
    assert_eq!(object.rotation_deg, -90.0);
    assert_eq!(object.pos, [10.0, 10.0]);
    assert!(bounds_close(object_world_bounds(object), [35.0, -15.0, 50.0, 100.0]));
}

#[test]
fn transform_set_rotation_keeps_box_centre() {
    let (mut session, id) = session_with_rect();
    session.run_named("transform.set", json!({"ids": [id.0], "rotation_deg": 30.0})).unwrap();
    let object = session.document.get_object(id).unwrap();
    assert_eq!(object.pos, [10.0, 10.0]);
    let centre = apply_matrix(&object_matrix(object), box_centre(object));
    assert!(close(centre[0], 60.0) && close(centre[1], 35.0));
}

#[test]
fn invert_matrix_singular_is_none() {
    assert!(invert_matrix(&[1.0, 2.0, 2.0, 4.0, 0.0, 0.0]).is_none());
    assert!(invert_matrix(&[f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]).is_none());
}

#[test]
fn doc_to_object_local_round_trip() {
    let object = Object { pos: [10.0, 20.0], size: [80.0, 40.0], rotation_deg: 30.0, shear_deg: 10.0, ..Object::default() };
    let local = [33.0, 41.0];
    let world = apply_matrix(&object_matrix(&object), local);
    let back = doc_to_object_local(&object, world).unwrap();
    assert!(close(back[0], local[0]) && close(back[1], local[1]));
}

#[test]
fn geometry_rev0_migration_keeps_world_quad() {
    let object = Object { id: ObjectId(1), pos: [10.0, 10.0], size: [100.0, 50.0], rotation_deg: 30.0, ..Object::default() };
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let old_quad = [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]].map(|p: [f32; 2]| [10.0 + p[0] * cos - p[1] * sin, 10.0 + p[0] * sin + p[1] * cos]);
    let mut document = Document::new([400.0, 300.0], Units::Px, 96.0, 1);
    document.objects.push(object);
    document.next_object_id = 2;
    let mut value = serde_json::to_value(&document).unwrap();
    value.as_object_mut().unwrap().remove("geometry_rev");
    let loaded = Document::from_native_json(&value.to_string()).unwrap();
    assert_eq!(loaded.geometry_rev, dtool_core::document::GEOMETRY_REV);
    let quad = object_world_quad(&loaded.objects[0]);
    for (a, b) in quad.iter().zip(old_quad.iter()) {
        assert!(close(a[0], b[0]) && close(a[1], b[1]), "{quad:?} {old_quad:?}");
    }
    let fresh = serde_json::to_value(Document::default()).unwrap();
    assert_eq!(fresh["geometry_rev"], json!(1));
}

#[test]
fn view_zoom_set_matches_zoom_about() {
    let (mut session, _) = session_with_rect();
    session.view.rotation_deg = 30.0;
    session.view.scroll = [12.0, -7.0];
    session.view.zoom = 1.5;
    let mut expected = session.view.clone();
    zoom_about(&mut expected, 3.0, [200.0, 100.0]);
    session.run_named("view.zoom.set", json!({"zoom": 3.0, "around": [200.0, 100.0]})).unwrap();
    assert_eq!(session.view.zoom, expected.zoom);
    assert_eq!(session.view.scroll, expected.scroll);
}
