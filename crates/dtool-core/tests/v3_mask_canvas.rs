use dtool_core::{Document, Session};
use dtool_core::canvas_size::{resize_document, turn_canvas, CanvasTurn, ResampleMethod};
use dtool_core::document::Units;
use serde_json::json;

fn document() -> Document {
    let mut session = Session::new(Document::new([2.0, 1.0], Units::Pt, 72.0, 1));
    let id = session.run_named("image.place", json!({"pixel_size":[2,1],"rgba":[255,0,0,255,0,255,0,255],"pos":[0,0]})).unwrap()["id"].as_u64().unwrap();
    session.run_named("layer.mask.set", json!({"id":id,"mask":{"width":2,"height":1,"coverage":[0,255]}})).unwrap();
    session.document
}

#[test]
fn dpi_resize_keeps_mask_ledger_and_cache_dimensions_together() {
    let mut doc = document();
    resize_document(&mut doc, 4, 2, 144.0, ResampleMethod::Nearest).unwrap();
    assert_eq!(doc.pages[0].size, [2.0,1.0]);
    assert_eq!(doc.objects[0].raster_size, Some((4,2)));
    assert_eq!(doc.layer_masks[0].coverage.width, 4);
    assert_eq!(doc.layer_masks[0].coverage.coverage, vec![0,0,255,255,0,0,255,255]);
    let (size, _) = dtool_core::raster_source::rgba(&doc, &doc.objects[0]).unwrap();
    assert_eq!(size,[4,2]);
}

#[test]
fn turns_transform_masks_in_the_same_direction_as_pixels() {
    for turn in [CanvasTurn::RotateLeft, CanvasTurn::RotateRight, CanvasTurn::FlipH, CanvasTurn::FlipV] {
        let mut doc = document();
        let original = doc.clone();
        turn_canvas(&mut doc, turn).unwrap();
        let mask = &doc.layer_masks[0].coverage;
        let layer = doc.pixel_layers.get(doc.objects[0].id).unwrap();
        assert_eq!([mask.width,mask.height],[layer.width,layer.height]);
        let pixels = layer.to_rgba();
        for (pixel, coverage) in pixels.chunks_exact(4).zip(&mask.coverage) {
            assert_eq!(*coverage, if pixel[0] == 255 { 0 } else { 255 });
        }
        assert_eq!(original.layer_masks[0].coverage.coverage,vec![0,255]);
    }
}

#[test]
fn resize_undo_restores_mask_dimensions_and_coverage() {
    let mut session = Session::new(document());
    let before = session.document.layer_masks.clone();
    session.run_named("doc.resize",json!({"w":4,"h":2,"dpi":72})).unwrap();
    assert_eq!(session.document.layer_masks[0].coverage.width,4);
    session.run_named("edit.undo",json!({})).unwrap();
    assert_eq!(session.document.layer_masks,before);
    session.run_named("edit.redo",json!({})).unwrap();
    assert_eq!(session.document.layer_masks[0].coverage.width,4);
}
