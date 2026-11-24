use dtool_core::document::{Document, Units};

#[test]
fn artboard_module_is_reachable_from_the_crate() {
    let doc = Document::new([800.0, 600.0], Units::Px, 72.0, 1);
    assert!(dtool_core::artboard::imported_artboard_candidates(&doc).is_empty());
}
