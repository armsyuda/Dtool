use dtool_core::palette::{PaletteScope, PaletteUi};
use dtool_ui::panels::swatches::PaletteFileDialog;

#[test]
fn import_dialog_builds_palette_import_command() {
    let state = PaletteUi { scope: PaletteScope::Document, palette: "Fixture".to_string(), ..PaletteUi::default() };
    let mut dialog = PaletteFileDialog::import();
    assert!(dialog.command(&state).is_none());
    dialog.path = "C:/tmp/input.ase".to_string();
    dialog.conflict = 1;
    dialog.palette_name = "Fixture".to_string();
    let command = dialog.command(&state).unwrap();
    assert_eq!(command.name, "palette.import");
    assert_eq!(command.params["format"], "ase");
    assert_eq!(command.params["on_conflict"], "skip");
    assert_eq!(command.params["scope"], "document");
    assert_eq!(command.params["palette_name"], "Fixture");
    assert_eq!(dialog.title_key(), "palette.import");
}

#[test]
fn export_dialog_uses_current_palette_and_format() {
    let state = PaletteUi { scope: PaletteScope::Application, palette: "Application".to_string(), ..PaletteUi::default() };
    let mut dialog = PaletteFileDialog::export();
    dialog.path = "out.json".to_string();
    dialog.format = 1;
    let command = dialog.command(&state).unwrap();
    assert_eq!(command.name, "palette.export");
    assert_eq!(command.params["format"], "json");
    assert_eq!(command.params["scope"], "application");
    assert_eq!(command.params["palette"], "Application");
}