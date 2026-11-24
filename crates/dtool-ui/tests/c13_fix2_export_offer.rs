use dtool_export::batch::E06_OFFERED_FORMATS;
use dtool_export::raster::{self, ExportError, E06_HELD_FORMATS};
use dtool_export::ExportFormat;
use dtool_ui::dialogs::export::{self, ExportDialog};
use serde_json::json;

#[test]
fn tiff_and_gif_are_rejected_and_offer_set_matches_e06() {
    assert_eq!(export::format_choices(), E06_OFFERED_FORMATS);
    assert_eq!(export::format_choices(), raster::E06_OFFERED_FORMATS);
    let doc = dtool_core::document::Document::new([120.0, 80.0], dtool_core::document::Units::Px, 96.0, 1);
    let mut dialog = ExportDialog::new();
    for name in ["tiff", "gif"] {
        let err = dialog
            .run(&doc, export::UI_EXPORT_OPEN, &json!({ "format": name }))
            .unwrap()
            .unwrap_err();
        assert!(matches!(err, ExportError::Unavailable(_)));
        assert!(!dialog.is_open());
    }
    let opened = dialog
        .run(&doc, export::UI_EXPORT_OPEN, &json!({ "format": "png" }))
        .unwrap()
        .unwrap();
    assert!(opened.get("open").and_then(|v| v.as_bool()).unwrap_or(false) || dialog.is_open());
    let visible = dialog.visible_formats();
    let offered: Vec<_> = visible.iter().filter(|(_, offered)| *offered).map(|(format, _)| *format).collect();
    let planned: Vec<_> = visible.iter().filter(|(_, offered)| !*offered).map(|(format, _)| *format).collect();
    assert_eq!(offered, E06_OFFERED_FORMATS);
    assert_eq!(planned, E06_HELD_FORMATS.to_vec());
    assert!(offered.contains(&ExportFormat::Pdf));
    assert!(offered.contains(&ExportFormat::Svg));
    assert!(offered.contains(&ExportFormat::Eps));
}

#[test]
fn webp_lossless_choice_is_settable() {
    let doc = dtool_core::document::Document::new([120.0, 80.0], dtool_core::document::Units::Px, 96.0, 1);
    let mut dialog = ExportDialog::new();
    dialog
        .run(&doc, export::UI_EXPORT_OPEN, &json!({ "format": "webp" }))
        .unwrap()
        .unwrap();
    assert!(dialog.options.lossless);
    dialog
        .run(&doc, export::UI_EXPORT_SET, &json!({ "lossless": false }))
        .unwrap()
        .unwrap();
    assert!(!dialog.options.lossless);
}
