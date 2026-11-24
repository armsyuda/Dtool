use dtool_core::document::{Document, SetupMargins, Units};

#[test]
fn native_save_open_keeps_doc_setup() {
    let mut doc = Document::new([595.0, 842.0], Units::Pt, 300.0, 3);
    doc.setup.landscape = true;
    doc.setup.facing_pages = false;
    doc.setup.arrange_vertical = true;
    doc.setup.start_on_right = false;
    doc.setup.reflow_pages = true;
    doc.setup.transparent_background = true;
    doc.setup.margins = SetupMargins { enabled: true, left: 10.0, right: 12.0, top: 14.0, bottom: 16.0, linked: false };
    doc.setup.bleed = SetupMargins { enabled: true, left: 3.0, right: 3.0, top: 3.0, bottom: 3.0, linked: true };
    let path = std::env::temp_dir().join(format!("dtool_f2_setup_{}.dtool", std::process::id()));
    dtool_io::native::save(&doc, &path).unwrap();
    let loaded = dtool_io::native::open(&path).unwrap();
    assert_eq!(loaded.setup, doc.setup);
    assert_eq!(loaded.pages.len(), 3);
}