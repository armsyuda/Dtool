use dtool_core::commands::ui::{export_action_registered, is_export_command, UI_EXPORT_COMMANDS};
use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use dtool_ui::dialogs::export::ExportDialog;
use dtool_ui::shell::{export_dialog, install_export_host};
use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap, AFFINITY_EXPORT_COMMAND, EXPORT_OPEN_COMMAND, EXPORT_OPEN_KEY, EXPORT_OPEN_MODIFIERS};
use serde_json::json;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn session() -> Session {
    let _ = install_export_host();
    let mut s = Session::new(Document::new([120.0, 80.0], Units::Px, 96.0, 1));
    s.run_named("object.create", json!({ "name": "a", "pos": [10.0, 10.0], "size": [40.0, 20.0] })).unwrap();
    s
}

#[test]
fn ui_export_commands_reach_the_shell_dialog() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    assert!(export_action_registered());
    for name in UI_EXPORT_COMMANDS {
        assert!(is_export_command(name));
    }
    let opened = s.run(&Command::with_params("ui.export.open", json!({ "format": "png" })));
    assert!(opened.is_ok(), "{:?}", opened.err().map(|e| e.code));
    assert!(export_dialog().lock().unwrap().is_open());
    let query = s.run(&Command::new("ui.export.query")).expect("query");
    assert_eq!(query["open"], json!(true));
    let preview = s.run(&Command::with_params("ui.export.preview", json!({ "zoom": 2.0 }))).expect("preview");
    assert!(preview.is_object());
    s.run(&Command::new("ui.export.close")).expect("close");
    assert!(!export_dialog().lock().unwrap().is_open());
}

#[test]
fn export_commands_that_need_a_document_are_refused_without_one() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    s.has_document = false;
    assert_eq!(s.run(&Command::new("ui.export.open")).unwrap_err().code, "NO_DOCUMENT");
    assert!(s.run(&Command::new("ui.export.query")).is_ok());
    assert!(s.run(&Command::new("ui.export.close")).is_ok());
}

#[test]
fn feature_finder_is_unchanged() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    let out = s.run(&Command::with_params("help.feature_finder", json!({ "q": "path.join" }))).expect("feature finder");
    assert!(out["hits"].as_array().unwrap().iter().any(|v| v == "path.join"));
}

#[test]
fn ctrl_alt_shift_w_opens_export_and_affinity_command_maps_to_it() {
    let map = ShortcutMap::affinity_default();
    let bound = map.bindings_for(EXPORT_OPEN_COMMAND);
    assert!(!bound.is_empty());
    assert!(bound.iter().any(|b| b.key == EXPORT_OPEN_KEY && b.modifiers == EXPORT_OPEN_MODIFIERS));
    let mods = Modifiers { ctrl: true, shift: true, alt: true };
    assert_eq!(map.dispatch(Focus::Canvas, EXPORT_OPEN_KEY, mods).as_deref(), Some(EXPORT_OPEN_COMMAND));
    assert_eq!(ShortcutMap::dispatch_command(AFFINITY_EXPORT_COMMAND), EXPORT_OPEN_COMMAND);
    assert_eq!(ShortcutMap::dispatch_command("edit.undo"), "edit.undo");
    let again = map.clone().with_export_default();
    assert_eq!(again.bindings_for(EXPORT_OPEN_COMMAND).len(), bound.len());
}
#[test]
fn export_preview_pan_uses_core_view_sign() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut dialog = ExportDialog::new();
    let doc = Document::new([120.0, 80.0], Units::Px, 96.0, 1);
    dialog.refresh(&doc);
    dialog.set_viewport([40.0, 40.0]);
    let size = dialog.preview_size().expect("preview size");
    assert!(size.0 > 0 && size.1 > 0);
    let out = dialog.preview_command(&json!({ "zoom": 2.0, "anchor": [0.0, 0.0], "pan": [-8.0, 0.0] }));
    assert_eq!(out["zoom"], json!(2.0));
    let pan = out["pan"].as_array().expect("pan");
    let px = pan[0].as_f64().unwrap();
    assert!(px <= 0.0, "core pan sign must be <= 0, got {px}");
    assert_eq!(dialog.preview_pixel([9.0, 0.0]), Some((8, 0)));
    let kept = dialog.preview_pixel([0.0, 0.0]);
    dialog.zoom_at(4.0, [0.0, 0.0]);
    assert_eq!(dialog.preview_pixel([0.0, 0.0]), kept);
}
