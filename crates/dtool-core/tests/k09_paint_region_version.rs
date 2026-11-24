use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::Object;
use dtool_core::pixel::PixelLayer;
use dtool_core::tiles::paint_region;

fn session_with_layer() -> (Session, dtool_core::object::ObjectId) {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let pixels = vec![255u8; 4 * 4 * 4];
    let id = session.document.create_object(Object {
        pos: [0.0, 0.0],
        size: [4.0, 4.0],
        raster_data: Some(pixels.clone()),
        raster_size: Some((4, 4)),
        ..Object::default()
    });
    session.document.pixel_layers.insert(id, PixelLayer::from_rgba(4, 4, &pixels));
    (session, id)
}

#[test]
fn paint_region_bumps_version_and_rgba_reads_the_edit() {
    let (mut session, id) = session_with_layer();
    let version = session.document.version;
    let generation = session.generation;
    paint_region(&mut session, id, [0, 0, 1, 1], &[255, 0, 0, 255]).unwrap();
    assert!(session.document.version > version);
    assert!(session.generation > generation);
    let object = session.document.get_object(id).unwrap();
    let (_, rgba) = dtool_core::raster_source::rgba(&session.document, object).unwrap();
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
}
