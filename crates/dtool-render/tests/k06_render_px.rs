use dtool_core::document::{Document, Units};
use dtool_core::object::Object;
use dtool_core::view_state::CanvasFlags;
use dtool_render::build_display_list;

#[test]
fn force_pixel_alignment_rounds_doc_px() {
    for dpi in [72.0, 96.0, 300.0] {
        let mut doc = Document::new([200.0, 200.0], Units::Px, dpi, 1);
        let id = doc.create_object(Object { pos: [10.4, 20.6], size: [30.2, 40.5], ..Object::default() });
        let flags = CanvasFlags { force_pixel_alignment: true, ..CanvasFlags::default() };
        let list = build_display_list(&doc, &flags);
        let item = list.items.iter().find(|item| item.object_id == id).unwrap();
        assert_eq!(item.bounds[0], 10.0, "{dpi}");
        assert_eq!(item.bounds[1], 21.0, "{dpi}");
    }
}
