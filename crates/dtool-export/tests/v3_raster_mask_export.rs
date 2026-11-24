use dtool_core::{Document, Session, ObjectId};
use dtool_core::document::Units;
use dtool_export::DocumentExportExt;
use serde_json::json;

#[test]
fn raster_export_reads_edited_pixels_and_separate_mask() {
    let mut session = Session::new(Document::new([2.0,1.0],Units::Px,72.0,1));
    let id = ObjectId(session.run_named("image.place",json!({"pixel_size":[2,1],"rgba":[255,0,0,255,0,255,0,255],"pos":[0,0]})).unwrap()["id"].as_u64().unwrap());
    session.document.pixel_layers.get_mut(id).unwrap().set(1,0,[3,4,5,200]);
    session.run_named("layer.mask.set",json!({"id":id.0,"mask":{"width":2,"height":1,"coverage":[0,128]}})).unwrap();
    let pixels = session.document.render_rgba_region((0.0,0.0,2.0,1.0),2,1);
    assert_eq!(pixels[3],0);
    assert_eq!(&pixels[4..], &[3,4,5,100]);
    assert_eq!(session.document.pixel_layers.get(id).unwrap().get(1,0), [3,4,5,200]);
}
