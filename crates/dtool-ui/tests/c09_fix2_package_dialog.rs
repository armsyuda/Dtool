use dtool_core::document::Units;
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_core::{Document, Session};
use dtool_ui::dialogs::package::{PackageDialogState, PACKAGE_SAVE};
use std::path::PathBuf;

#[test]
fn dialog_builds_doc_package_save_and_reports_missing() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09_fix2_pkg_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut document = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    document.add_image_resource(ImageResource::new_linked(root.join("gone.png"), ImageFormat::Png, [1, 1], 72.0));
    let mut session = Session::new(document);
    let mut state = PackageDialogState::default();
    assert!(state.command().is_none());
    state.folder = root.join("out").display().to_string();
    let command = state.command().unwrap();
    assert_eq!(command.name, PACKAGE_SAVE);
    assert_eq!(command.params["fonts"], serde_json::json!(false));
    state.apply_result(session.run(&command));
    assert!(state.error_key.is_none());
    assert_eq!(state.missing().len(), 1);
    assert_eq!(state.packaged_count(), 0);
    state.apply_result(session.run(&dtool_core::Command::with_params(PACKAGE_SAVE, serde_json::json!({"folder": state.folder, "fonts": true}))));
    assert!(state.report.is_none());
    assert_eq!(state.error_key.as_deref(), Some("error.package_font_sources"));
    state.zip = true;
    let zipped = state.command().unwrap();
    assert_eq!(zipped.params["zip"], serde_json::json!(true));
    state.apply_result(session.run(&zipped));
    assert!(state.error_key.is_none());
    assert!(std::path::Path::new(&state.zip_path().unwrap()).is_file());
    assert!(state.fonts_missing().is_empty());
    state.include_fonts = true;
    assert_eq!(state.command().unwrap().params["fonts"], serde_json::json!(true));
}
