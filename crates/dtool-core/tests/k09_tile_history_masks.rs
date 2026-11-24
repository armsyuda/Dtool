use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::Object;
use dtool_core::pixel::PixelLayer;
use dtool_core::raster_source::LayerMask;
use dtool_core::selection::PixelSelection;
use dtool_core::tiles::{paint_region, redo_history, undo_history};

fn coverage(width: u32, height: u32, value: u8) -> PixelSelection {
    PixelSelection::from_coverage(width, height, vec![value; (width * height) as usize])
}

#[test]
fn tile_history_undo_restores_layer_masks_and_bumps_version() {
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
    session.document.layer_masks.push(LayerMask {
        object: id,
        coverage: coverage(4, 4, 255),
    });
    let version = session.document.version;
    paint_region(&mut session, id, [0, 0, 1, 1], &[255, 0, 0, 255]).unwrap();
    assert!(session.document.version > version);
    session.document.layer_masks[0].coverage = coverage(4, 4, 10);
    let version = session.document.version;
    undo_history(&mut session).unwrap();
    assert!(session.document.version > version);
    assert_eq!(session.document.layer_masks.len(), 1);
    assert_eq!(session.document.layer_masks[0].object, id);
    assert_eq!(session.document.layer_masks[0].coverage.coverage[0], 255);
    let object = session.document.get_object(id).unwrap();
    let (_, rgba) = dtool_core::raster_source::rgba(&session.document, object).unwrap();
    assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
    redo_history(&mut session).unwrap();
    let object = session.document.get_object(id).unwrap();
    let (_, rgba) = dtool_core::raster_source::rgba(&session.document, object).unwrap();
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(session.document.layer_masks[0].coverage.coverage[0], 255);
}

#[test]
fn paint_region_is_one_document_undo_step_and_memory_still_reports() {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let pixels = vec![255u8; 4 * 4 * 4];
    let id = session.document.create_object(Object {
        pos: [0.0, 0.0],
        size: [4.0, 4.0],
        raster_data: Some(pixels.clone()),
        raster_size: Some((4, 4)),
        ..Object::default()
    });
    session.document.pixel_selection = Some(coverage(4, 4, 7));
    for step in 0..20u8 {
        paint_region(&mut session, id, [0, 0, 1, 1], &[step, 0, 0, 255]).unwrap();
    }
    assert!(dtool_core::history_memory(&session).live_bytes > 0);
    for _ in 0..20 {
        session.run_named("edit.undo", serde_json::json!({})).unwrap();
    }
    assert!(session.document.pixel_layers.get(id).is_none());
    let object = session.document.get_object(id).unwrap();
    assert_eq!(object.raster_data.as_deref(), Some(&pixels[..]));
    assert_eq!(session.document.pixel_selection, Some(coverage(4, 4, 7)));
    assert!(dtool_core::history_memory(&session).history_bytes > 0);
}
