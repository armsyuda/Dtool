use dtool_core::document::{Document, Units};
use dtool_export::{ExportArea, ExportControl, ExportFormat, ExportRequest};
use dtool_export::pdf::{export_pdf, PdfOptions};

#[test]
fn pdf_all_pages_and_spreads_have_actual_page_counts() {
    let mut doc = Document::new([32.0, 24.0], Units::Px, 72.0, 4);
    doc.setup.facing_pages = true;
    doc.setup.start_on_right = true;
    let dir = std::env::temp_dir().join(format!("c13_pdf_pages_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (area, expected, name) in [(ExportArea::AllPages, 4, "pages"), (ExportArea::AllSpreads, 3, "spreads"), (ExportArea::Page(2), 1, "page")] {
        let path = dir.join(format!("{name}.pdf"));
        let request = ExportRequest { format: ExportFormat::Pdf, area, pages: None, path: path.clone(), preset: None, options: serde_json::Value::Null };
        export_pdf(&doc, &request, &PdfOptions::default(), &ExportControl::new()).unwrap();
        let admitted = dtool_io::admit(&path, dtool_io::ImportFormat::Pdf).unwrap();
        assert_eq!(admitted.pages as usize, expected, "{name}");
        assert!(admitted.supported, "{name}: {:?}", admitted.reason_key);
    }
}

#[test]
fn selected_group_export_keeps_nested_descendants_but_not_siblings() {
    let mut doc = Document::new([64.0, 64.0], Units::Px, 72.0, 1);
    let group = doc.create_object(dtool_core::Object { kind: dtool_core::ObjectKind::Group, ..Default::default() });
    let child = doc.create_object(dtool_core::Object { parent_id: Some(group), kind: dtool_core::ObjectKind::Group, ..Default::default() });
    let leaf = doc.create_object(dtool_core::Object { parent_id: Some(child), ..Default::default() });
    let sibling = doc.create_object(dtool_core::Object::default());
    let result = dtool_export::only_objects(&doc, &[group]);
    assert!(result.get_object(leaf).is_some());
    assert!(result.get_object(sibling).is_none());
    assert_eq!(doc.objects.len(), 4);
}
