use dtool_core::document::{Document, Units};
use dtool_core::object::Object;
use dtool_core::resources::{resource_rows, ImageResource};

#[test]
fn placed_dpi_uses_pt_boundary() {
    let mut doc = Document::new([1000.0, 1000.0], Units::Px, 300.0, 1);
    let resource = doc.add_image_resource(ImageResource { pixel_size: [300, 300], dpi: 300.0, ..ImageResource::default() });
    let id = doc.create_object(Object { pos: [0.0, 0.0], size: [300.0, 300.0], ..Object::default() });
    doc.place_image(id, resource).unwrap();
    let rows = resource_rows(&doc, None);
    assert!((rows[0].placed_dpi - 300.0).abs() < 1e-3);
}

#[test]
fn image_place_default_size_is_px_at_document_dpi() {
    let mut session = dtool_core::Session::new(Document::new([1000.0, 1000.0], Units::Mm, 96.0, 1));
    let rgba = vec![255u8; 4 * 300 * 150];
    session
        .run_named("image.place", serde_json::json!({"pos": [0.0, 0.0], "pixel_size": [300, 150], "dpi": 300.0, "rgba": rgba}))
        .unwrap();
    let object = session.document.objects.last().unwrap();
    assert!((object.size[0] - 96.0).abs() < 1e-3);
    assert!((object.size[1] - 48.0).abs() < 1e-3);
}
