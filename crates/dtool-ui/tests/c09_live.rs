use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_protocol::Command;
use dtool_ui::dialogs::resource_manager::{format_label, image_headers, placed_size_label, ResourceManagerDialog, TYPE_KEY};
use serde_json::json;

#[test]
fn image_tab_headers_include_placed_size_and_type_only_when_keyed() {
    let headers = image_headers();
    assert!(headers.contains(&"rm.placed_size"));
    assert!(headers.iter().all(|key| dtool_strings::has(key)), "{headers:?}");
    assert_eq!(headers.contains(&TYPE_KEY), dtool_strings::has(TYPE_KEY));
    assert_eq!(format_label(ImageFormat::Jpeg), "JPEG");
    assert_eq!(placed_size_label(Some([12.0, 3.5])), "12.0x3.5");
    assert_eq!(placed_size_label(None), "");
}

#[test]
fn resource_manager_draws_live_rows_without_panicking() {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let rid = session.document.add_image_resource(ImageResource::new_embedded(ImageFormat::Png, [4, 4], 72.0, vec![0; 64]));
    session.run(&Command::with_params("frame.image.add", json!({ "pos": [0.0, 0.0], "size": [40.0, 40.0], "resource": rid.0 }))).unwrap();
    let mut dialog = ResourceManagerDialog::new();
    dialog.open();
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        dialog.show(ui.ctx(), &mut session, None);
    });
    out.textures_delta.clear();
    assert!(dialog.open);
    let rows = dtool_core::resources::resource_rows(&session.document, None);
    assert_eq!(rows[0].placed_size.map(|s| s[0].round()), Some(40.0));
}
