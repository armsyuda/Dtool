use dtool_core::commands::picture_frame::{frame_geometry, FrameShape};
use dtool_core::document::Units;
use dtool_core::scene::{build_scene, Prim};
use dtool_core::{CanvasFlags, Document, ObjectId, Session};
use dtool_export::DocumentExportExt;
use serde_json::json;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn framed(shape: &str) -> (Session, ObjectId) {
    let mut session = Session::new(Document::new([40.0, 40.0], Units::Px, 72.0, 1));
    let reply = session.run_named("frame.image.add", json!({"shape": shape, "pos": [0, 0], "size": [40.0, 40.0]})).unwrap();
    let id = ObjectId(reply["id"].as_u64().or_else(|| reply["object_id"].as_u64()).unwrap_or_else(|| session.selection.ids[0].0));
    let object = session.document.objects.iter_mut().find(|object| object.id == id).unwrap();
    object.raster_size = Some((2, 2));
    object.raster_data = Some([RED, GREEN, BLUE, WHITE].concat());
    session.run_named("frame.image.fit.set", json!({"object": id.0, "fit": "stretch"})).unwrap();
    (session, id)
}

fn pixel(session: &Session, x: usize, y: usize) -> [u8; 4] {
    let pixels = session.document.render_rgba_region((0.0, 0.0, 40.0, 40.0), 40, 40);
    let i = (y * 40 + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn image_quad(session: &Session, id: ObjectId) -> [[f32; 2]; 4] {
    let scene = build_scene(&session.document, &CanvasFlags::default());
    scene.prims.iter().find_map(|prim| match prim { Prim::Image { object, quad, .. } if *object == id => Some(*quad), _ => None }).unwrap()
}

fn close(a: [[f32; 2]; 4], b: [[f32; 2]; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(p, q)| (p[0] - q[0]).abs() < 1e-3 && (p[1] - q[1]).abs() < 1e-3)
}

#[test]
fn c09c_content_rotation_turns_the_scene_quad_and_the_exported_pixels() {
    let (mut session, id) = framed("rect");
    assert_eq!(pixel(&session, 10, 10), RED);
    session.run_named("frame.image.content.set", json!({"object": id.0, "rotation": 90.0})).unwrap();
    assert!(close(image_quad(&session, id), [[40.0, 0.0], [40.0, 40.0], [0.0, 40.0], [0.0, 0.0]]), "{:?}", image_quad(&session, id));
    assert_eq!(pixel(&session, 10, 10), BLUE);
    assert_eq!(pixel(&session, 30, 10), RED);
    assert_eq!(pixel(&session, 30, 30), GREEN);
    assert_eq!(pixel(&session, 10, 30), WHITE);

    session.run_named("frame.image.content.set", json!({"object": id.0, "rotation": 45.0})).unwrap();
    let h = 20.0 * std::f32::consts::SQRT_2;
    assert!(close(image_quad(&session, id), [[20.0, 20.0 - h], [20.0 + h, 20.0], [20.0, 20.0 + h], [20.0 - h, 20.0]]), "{:?}", image_quad(&session, id));
    for (x, y) in [(1, 1), (38, 1), (1, 38), (38, 38)] {
        assert_eq!(pixel(&session, x, y)[3], 0, "corner {x},{y}");
    }
    assert_eq!(pixel(&session, 20, 3), RED);
    assert_eq!(pixel(&session, 36, 20), GREEN);
    assert_eq!(pixel(&session, 20, 36), WHITE);
    assert_eq!(pixel(&session, 3, 20), BLUE);

    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pixel(&session, 10, 10), BLUE);
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pixel(&session, 10, 10), RED);
}

#[test]
fn c09c_rotated_content_stays_inside_the_ellipse_frame() {
    let (mut session, id) = framed("ellipse");
    session.run_named("frame.image.content.set", json!({"object": id.0, "rotation": 90.0})).unwrap();
    for (x, y) in [(1, 1), (38, 1), (1, 38), (38, 38)] {
        assert_eq!(pixel(&session, x, y)[3], 0, "corner {x},{y}");
    }
    assert_eq!(pixel(&session, 12, 10), BLUE);
    assert_eq!(pixel(&session, 28, 10), RED);
    assert_eq!(pixel(&session, 28, 30), GREEN);
    assert_eq!(pixel(&session, 12, 30), WHITE);
    let geometry = frame_geometry(&session.document, session.document.get_object(id).unwrap()).unwrap();
    assert_eq!(geometry.shape, FrameShape::Ellipse);
    assert_eq!(geometry.rotation, 90.0);
    let quad = image_quad(&session, id);
    let polygon = geometry.image_polygon(&quad);
    assert!(polygon.len() >= 32);
    for (p, uv) in &polygon {
        let d = ((p[0] - 20.0) / 20.0).powi(2) + ((p[1] - 20.0) / 20.0).powi(2);
        assert!(d <= 1.0 + 1e-3, "{p:?}");
        assert!((0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]));
    }
    assert!(!polygon.iter().any(|(p, _)| p[0] < 3.0 && p[1] < 3.0));
}

#[test]
fn c09c_rect_frame_polygon_is_the_visible_part_of_the_content() {
    let (mut session, id) = framed("rect");
    session.run_named("frame.image.fit.set", json!({"object": id.0, "fit": "min"})).unwrap();
    session.run_named("frame.image.content.set", json!({"object": id.0, "scale": [2.0, 0.5]})).unwrap();
    let geometry = frame_geometry(&session.document, session.document.get_object(id).unwrap()).unwrap();
    let quad = image_quad(&session, id);
    let polygon = geometry.image_polygon(&quad);
    let xs: Vec<f32> = polygon.iter().map(|(p, _)| p[0]).collect();
    let ys: Vec<f32> = polygon.iter().map(|(p, _)| p[1]).collect();
    assert!(xs.iter().all(|x| (-1e-3..=40.001).contains(x)), "{polygon:?}");
    assert!(ys.iter().all(|y| (9.999..=30.001).contains(y)), "{polygon:?}");
    assert!(polygon.iter().any(|(_, uv)| (uv[0] - 0.25).abs() < 1e-3));
    assert!(polygon.iter().any(|(_, uv)| (uv[0] - 0.75).abs() < 1e-3));
    assert_eq!(pixel(&session, 1, 1)[3], 0);
    assert_eq!(pixel(&session, 1, 12), RED);
    assert_eq!(pixel(&session, 38, 28), WHITE);
}