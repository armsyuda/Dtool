use dtool_core::document::Units;
use dtool_core::object::ObjectKind;
use dtool_core::scene::{build_scene, Prim};
use dtool_core::{CanvasFlags, Document, ObjectId, Session};
use dtool_export::DocumentExportExt;
use serde_json::json;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn framed(shape: &str, size: [f32; 2]) -> (Session, ObjectId) {
    let mut session = Session::new(Document::new([40.0, 40.0], Units::Px, 72.0, 1));
    let reply = session.run_named("frame.image.add", json!({"shape": shape, "pos": [0, 0], "size": size})).unwrap();
    let id = ObjectId(reply["id"].as_u64().or_else(|| reply["object_id"].as_u64()).unwrap_or_else(|| session.selection.ids[0].0));
    let object = session.document.objects.iter_mut().find(|object| object.id == id).unwrap();
    object.raster_size = Some((2, 2));
    object.raster_data = Some([RED, GREEN, BLUE, WHITE].concat());
    (session, id)
}

fn pixel(session: &Session, x: usize, y: usize) -> [u8; 4] {
    let pixels = session.document.render_rgba_region((0.0, 0.0, 40.0, 40.0), 40, 40);
    let i = (y * 40 + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn frame_image_add_creates_a_real_picture_frame_with_a_clipped_content_quad() {
    let (mut session, id) = framed("rect", [40.0, 20.0]);
    assert!(matches!(session.document.get_object(id).unwrap().kind, ObjectKind::PictureFrame));
    let scene = build_scene(&session.document, &CanvasFlags::default());
    let at = scene.prims.iter().position(|prim| matches!(prim, Prim::Image { object, .. } if *object == id)).unwrap();
    assert_eq!(scene.prims[at - 1], Prim::Clip { object: id, rect: Some([0.0, 0.0, 40.0, 20.0]) });
    assert_eq!(scene.prims[at + 1], Prim::Clip { object: id, rect: None });
    let Prim::Image { quad, .. } = &scene.prims[at] else { unreachable!() };
    assert_eq!(*quad, [[10.0, 0.0], [30.0, 0.0], [30.0, 20.0], [10.0, 20.0]]);
    assert_eq!(pixel(&session, 5, 5)[3], 0);
    assert_eq!(pixel(&session, 12, 5), RED);
    assert_eq!(pixel(&session, 28, 5), GREEN);
    assert_eq!(pixel(&session, 12, 15), BLUE);
    assert_eq!(pixel(&session, 35, 15)[3], 0);

    session.run_named("frame.image.anchor.set", json!({"object": id.0, "anchor": "top_left"})).unwrap();
    assert_eq!(pixel(&session, 5, 5), RED);
    assert_eq!(pixel(&session, 15, 15), WHITE);
    assert_eq!(pixel(&session, 30, 10)[3], 0);

    session.run_named("frame.image.fit.set", json!({"object": id.0, "fit": "max"})).unwrap();
    assert_eq!(pixel(&session, 5, 5), RED);
    assert_eq!(pixel(&session, 35, 5), GREEN);
    assert_eq!(pixel(&session, 35, 19), GREEN);
    assert_eq!(pixel(&session, 5, 25)[3], 0);

    session.run_named("frame.image.content.set", json!({"object": id.0, "offset": [-20.0, 0.0]})).unwrap();
    assert_eq!(pixel(&session, 5, 5), GREEN);
    assert_eq!(pixel(&session, 25, 5)[3], 0);

    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pixel(&session, 5, 5), RED);
}

#[test]
fn ellipse_frame_clips_the_exported_image_to_the_ellipse() {
    let (mut session, id) = framed("ellipse", [40.0, 40.0]);
    session.run_named("frame.image.fit.set", json!({"object": id.0, "fit": "stretch"})).unwrap();
    assert_eq!(pixel(&session, 1, 1)[3], 0);
    assert_eq!(pixel(&session, 38, 38)[3], 0);
    assert_eq!(pixel(&session, 10, 10), RED);
    assert_eq!(pixel(&session, 30, 10), GREEN);
    assert_eq!(pixel(&session, 20, 2), GREEN);
    assert_eq!(pixel(&session, 2, 20), BLUE);
    let mut rect = framed("rect", [40.0, 40.0]).0;
    let rect_id = rect.selection.ids[0];
    rect.run_named("frame.image.fit.set", json!({"object": rect_id.0, "fit": "stretch"})).unwrap();
    assert_eq!(pixel(&rect, 1, 1), RED);
    assert_eq!(pixel(&rect, 38, 38), WHITE);
}