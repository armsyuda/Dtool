use dtool_core::commands::export_presets::{self as presets, current_options, load_store, merged_options, save_store, set_store_path, store_path, ExportPreset, ExportPresetStore, FORMATS, OPTIONS_KEY, PRESET_EXISTS};
use dtool_core::document::{Document, Units};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, Session};
use serde_json::{json, Value};
use std::path::PathBuf;

fn store(tag: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_export_presets_{tag}_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    set_store_path(Some(path.clone()));
    path
}

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1))
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    let command = Command::with_params(name, params);
    let result = match presets::run(session, &command) {
        Some(result) => result,
        None => session.run(&command),
    };
    result.unwrap_or_else(|e| panic!("{name}: {}", e.code))
}

fn code(session: &mut Session, name: &str, params: Value) -> String {
    presets::run(session, &Command::with_params(name, params)).expect(name).expect_err(name).code
}

fn stored() -> Value {
    let mut value = options();
    value["format"] = json!("Png");
    value
}

fn options() -> Value {
    json!({ "format": "png", "dpi": 300.0, "include_bleed": true, "icc": "sRGB IEC61966-2.1", "embed_icc": true, "embed_metadata": false, "future_key": { "nested": [1, 2] } })
}

#[test]
fn crud_round_trip_persists_to_the_store_file_and_keeps_unknown_keys() {
    let path = store("crud");
    let mut s = session();
    let created = run(&mut s, "export.options_preset.create", json!({ "name": " Web ", "options": options() }));
    assert_eq!((created["preset_id"].clone(), created["name"].clone()), (json!(1), json!("Web")));
    assert_eq!(created["options"], stored());
    let second = run(&mut s, "export.options_preset.create", json!({ "name": "Print", "options": { "format": "pdf" } }));
    assert_eq!(second["preset_id"], json!(2));
    assert_eq!(code(&mut s, "export.options_preset.create", json!({ "name": "Web", "options": {} })), PRESET_EXISTS);
    assert_eq!(code(&mut s, "export.options_preset.create", json!({ "name": "x", "options": [1] })), "INVALID_PARAMS");
    assert_eq!(code(&mut s, "export.options_preset.create", json!({ "name": "  ", "options": {} })), "INVALID_PARAMS");
    let on_disk: ExportPresetStore = load_store(&path).unwrap();
    assert_eq!(serde_json::to_value(&on_disk.presets[0].options).unwrap(), stored());
    let listed = run(&mut s, "export.options_preset.list", json!({}));
    assert_eq!(listed["presets"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect::<Vec<_>>(), vec!["Web", "Print"]);
    assert_eq!(listed["presets"][0]["options"]["future_key"], json!({ "nested": [1, 2] }));
    assert_eq!(code(&mut s, "export.options_preset.rename", json!({ "preset_id": 2, "name": "Web" })), PRESET_EXISTS);
    assert_eq!(run(&mut s, "export.options_preset.rename", json!({ "preset_id": 2, "name": "Press" }))["name"], json!("Press"));
    let mut next = options();
    next["dpi"] = json!(150.0);
    let mut expected = next.clone();
    expected["format"] = json!("Png");
    assert_eq!(run(&mut s, "export.options_preset.update", json!({ "preset": "Web", "options": next.clone() }))["options"], expected);
    assert_eq!(load_store(&path).unwrap().presets[0].options.get("future_key"), Some(&json!({ "nested": [1, 2] })));
    assert_eq!(run(&mut s, "export.options_preset.delete", json!({ "preset_id": 1 }))["preset_id"], json!(1));
    assert_eq!(code(&mut s, "export.options_preset.delete", json!({ "preset_id": 1 })), "NOT_FOUND");
    assert_eq!(code(&mut s, "export.options_preset.load", json!({})), "INVALID_PARAMS");
    let names: Vec<String> = load_store(&path).unwrap().presets.into_iter().map(|p| p.name).collect();
    assert_eq!(names, vec!["Press".to_string()]);
    set_store_path(None);
}

#[test]
fn load_sets_live_document_options_with_one_undo_step_and_unknown_keys_survive() {
    store("load");
    let mut s = session();
    let id = run(&mut s, "export.options_preset.create", json!({ "name": "Web", "options": options() }))["preset_id"].clone();
    assert!(current_options(&s.document).is_none());
    let generation = s.generation;
    let reply = run(&mut s, "export.options_preset.load", json!({ "preset_id": id }));
    assert_eq!(s.generation, generation + 1);
    assert_eq!(reply["options"], stored());
    assert_eq!(Value::Object(current_options(&s.document).unwrap()), stored());
    assert_eq!(s.document.export_defaults[OPTIONS_KEY]["future_key"], json!({ "nested": [1, 2] }));
    run(&mut s, "edit.undo", json!({}));
    assert!(current_options(&s.document).is_none());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(Value::Object(current_options(&s.document).unwrap()), stored());
    let saved = serde_json::to_string(&s.document).unwrap();
    let reopened: Document = serde_json::from_str(&saved).unwrap();
    assert_eq!(Value::Object(current_options(&reopened).unwrap()), stored());
    set_store_path(None);
}

#[test]
fn create_and_update_without_options_snapshot_the_document_options() {
    store("snapshot");
    let mut s = session();
    s.document.export_defaults = json!({ OPTIONS_KEY: { "format": "jpeg", "quality": 80, "later_field": "kept" } });
    let created = run(&mut s, "export.options_preset.create", json!({ "name": "From doc" }));
    assert_eq!(created["options"], json!({ "format": "Jpeg", "quality": 80, "later_field": "kept" }));
    s.document.export_defaults[OPTIONS_KEY]["quality"] = json!(60);
    let updated = run(&mut s, "export.options_preset.update", json!({ "preset_id": created["preset_id"] }));
    assert_eq!(updated["options"]["quality"], json!(60));
    set_store_path(None);
}

#[test]
fn default_store_sits_next_to_prefs_under_appdata() {
    set_store_path(None);
    let path = store_path();
    assert_eq!(path, dtool_core::prefs::Prefs::default_path().with_file_name("export_presets.json"));
    if let Some(appdata) = std::env::var_os("APPDATA") {
        assert!(path.starts_with(PathBuf::from(appdata)));
    }
}

#[test]
fn workspace_route_runs_presets_once_catalog_rows_are_live() {
    store("workspace");
    let mut ws = Workspace::default();
    ws.new_document("c13_presets");
    let created = ws.run(Scope::active(), Command::with_params("export.options_preset.create", json!({ "name": "Web", "options": options() }))).1.expect("create");
    ws.run(Scope::active(), Command::with_params("export.options_preset.load", json!({ "preset_id": created["preset_id"] }))).1.expect("load");
    let doc = &ws.session_mut().unwrap().document;
    assert_eq!(Value::Object(current_options(doc).unwrap()), stored());
    set_store_path(None);
}

#[test]
fn session_route_runs_presets_once_catalog_rows_are_live() {
    store("session");
    let mut s = session();
    let created = s.run(&Command::with_params("export.options_preset.create", json!({ "name": "Web", "options": options() }))).expect("create");
    s.run(&Command::with_params("export.options_preset.load", json!({ "preset_id": created["preset_id"] }))).expect("load");
    assert_eq!(Value::Object(current_options(&s.document).unwrap()), stored());
    set_store_path(None);
}

#[test]
fn every_preset_command_is_handled_by_the_module() {
    store("names");
    let mut s = session();
    for name in presets::NAMES {
        assert!(presets::can_execute(&s, &Command::new(name)).is_some(), "{name}");
        assert!(presets::run(&mut s, &Command::with_params(name, json!({ "preset_id": 99, "name": "n", "options": {} }))).is_some(), "{name}");
    }
    set_store_path(None);
}

#[test]
fn format_is_stored_in_canonical_case_and_unknown_formats_are_rejected() {
    let path = store("format");
    let mut s = session();
    let cases = [("png", "Png"), ("PNG", "Png"), ("Png", "Png"), (" pdf ", "Pdf"), ("jpg", "Jpeg"), ("JPEG", "Jpeg"), ("tif", "Tiff"), ("wmf", "Wmf")];
    for (index, (input, expected)) in cases.iter().enumerate() {
        let reply = run(&mut s, "export.options_preset.create", json!({ "name": format!("p{index}"), "options": { "format": input, "dpi": 72.0 } }));
        assert_eq!(reply["options"]["format"], json!(expected), "{input}");
        assert_eq!(load_store(&path).unwrap().presets[index].options["format"], json!(expected), "{input}");
    }
    for name in FORMATS {
        let reply = run(&mut s, "export.options_preset.update", json!({ "preset": "p0", "options": { "format": name.to_lowercase() } }));
        assert_eq!(reply["options"]["format"], json!(name));
    }
    assert_eq!(code(&mut s, "export.options_preset.create", json!({ "name": "bad", "options": { "format": "bmp" } })), "INVALID_PARAMS");
    assert_eq!(code(&mut s, "export.options_preset.create", json!({ "name": "bad", "options": { "format": 5 } })), "INVALID_PARAMS");
    let before = load_store(&path).unwrap();
    assert_eq!(code(&mut s, "export.options_preset.update", json!({ "preset": "p0", "options": { "format": "xyz" } })), "INVALID_PARAMS");
    assert_eq!(load_store(&path).unwrap(), before);
    let free = run(&mut s, "export.options_preset.create", json!({ "name": "no format", "options": { "dpi": 96.0 } }));
    assert_eq!(free["options"], json!({ "dpi": 96.0 }));
    set_store_path(None);
}

#[test]
fn load_canonicalises_presets_saved_before_the_fix_and_drops_unknown_formats() {
    let path = store("legacy");
    let legacy = |id: u64, name: &str, format: &str| ExportPreset { id, name: name.to_string(), options: json!({ "format": format, "quality": 70 }).as_object().unwrap().clone() };
    save_store(&path, &ExportPresetStore { presets: vec![legacy(1, "old", "png"), legacy(2, "odd", "bmp")] }).unwrap();
    let mut s = session();
    let reply = run(&mut s, "export.options_preset.load", json!({ "preset_id": 1 }));
    assert_eq!(reply["options"]["format"], json!("Png"));
    assert_eq!(s.document.export_defaults[OPTIONS_KEY], json!({ "format": "Png", "quality": 70 }));
    run(&mut s, "export.options_preset.load", json!({ "preset": "odd" }));
    assert_eq!(s.document.export_defaults[OPTIONS_KEY], json!({ "quality": 70 }));
    set_store_path(None);
}

#[test]
fn merged_options_canonicalises_base_and_saved_formats() {
    let saved = json!({ "format": "pdf", "dpi": 300.0 });
    let merged = merged_options(json!({ "format": "Png", "quality": 90 }), saved.as_object().unwrap());
    assert_eq!(merged, json!({ "format": "Pdf", "quality": 90, "dpi": 300.0 }));
    let unknown = json!({ "format": "bmp" });
    let kept = merged_options(json!({ "format": "jpeg" }), unknown.as_object().unwrap());
    assert_eq!(kept, json!({ "format": "Jpeg" }));
}
