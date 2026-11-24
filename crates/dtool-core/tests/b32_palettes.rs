use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::palette::{decode_ase, encode_ase, entries_of, ordered_ids, PaletteSort};
use dtool_core::{Command, Session};
use serde_json::{json, Value};
use std::path::PathBuf;

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Pt, 72.0, 1);
    document.objects.push(Object { id: ObjectId(1), fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    Session::new(document)
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("b32_palettes_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn ase_round_trip_keeps_names_models_values_and_flags() {
    let mut session = session();
    run(&mut session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "global": true }));
    run(&mut session, "swatch.spot.add", json!({ "name": "PANTONE 185 C", "colour": "#E4002B" }));
    run(&mut session, "swatch.add", json!({ "name": "Plain", "colour": "#336699" }));
    let mut cmyk = session.document.swatches[2].clone();
    cmyk.name = "Cyan".to_string();
    cmyk.colour = Colour::from_managed(dtool_core::colour::Colour::cmyk(100.0, 0.0, 0.0, 0.0, 1.0));
    let mut entries = entries_of(&session.document.swatches);
    entries.extend(entries_of(&[cmyk]));
    let bytes = encode_ase("Fixture", &entries);
    assert_eq!(&bytes[..4], b"ASEF");
    let (group, decoded) = decode_ase(&bytes).unwrap();
    assert_eq!(group.as_deref(), Some("Fixture"));
    assert_eq!(decoded.len(), 4);
    assert_eq!(decoded[0].name, "Red");
    assert_eq!(decoded[0].colour, Colour::new(255, 0, 0, 255));
    assert!(decoded[0].global && !decoded[0].spot);
    assert!(decoded[1].spot);
    assert!(!decoded[2].global);
    assert_eq!(decoded[3].colour.model, dtool_core::colour::ColourModel::Cmyk);
    assert!((decoded[3].colour.values[0] - 100.0).abs() < 1e-3);
    assert!(decode_ase(b"NOPE").is_err());
}

#[test]
fn document_import_export_with_conflicts_and_one_undo() {
    let mut source = session();
    run(&mut source, "palette.create", json!({ "name": "Fixture" }));
    run(&mut source, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "palette": "Fixture", "global": true }));
    run(&mut source, "swatch.add", json!({ "name": "Blue", "colour": "#0000FF", "palette": "Fixture" }));
    let path = temp("input.ase");
    let exported = run(&mut source, "palette.export", json!({ "path": path, "format": "ase", "scope": "document", "palette": "Fixture" }));
    assert_eq!(exported["count"], 2);
    let original = std::fs::read(&path).unwrap();
    let mut session = session();
    let params = json!({ "path": path, "format": "ase", "scope": "document", "palette_name": "Fixture", "on_conflict": "rename" });
    let first = run(&mut session, "palette.import", params.clone());
    assert_eq!(first["palette"], "Fixture");
    assert_eq!(first["added"], 2);
    let second = run(&mut session, "palette.import", params.clone());
    assert_eq!(second["palette"], "Fixture 2");
    assert_eq!(second["renamed"], 1);
    let mut skip = params.clone();
    skip["on_conflict"] = json!("skip");
    assert_eq!(run(&mut session, "palette.import", skip)["skipped"], 2);
    let mut replace = params;
    replace["on_conflict"] = json!("replace");
    assert_eq!(run(&mut session, "palette.import", replace)["replaced"], 2);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(session.document.palettes, vec!["Fixture".to_string(), "Fixture 2".to_string()]);
    assert!(session.document.swatches.iter().any(|swatch| swatch.name == "Red" && swatch.global));
    let count = session.document.swatches.len();
    run(&mut session, "edit.undo", json!({}));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.palettes, vec!["Fixture".to_string()]);
    assert!(session.document.swatches.len() < count);
    run(&mut session, "edit.undo", json!({}));
    assert!(session.document.palettes.is_empty());
    assert!(session.document.swatches.is_empty());
}

#[test]
fn document_palettes_create_rename_delete_undo() {
    let mut session = session();
    run(&mut session, "palette.create", json!({ "name": "Brand", "scope": "document" }));
    assert_eq!(session.run(&Command::with_params("palette.create", json!({ "name": "Brand" }))).unwrap_err().code, "PALETTE_EXISTS");
    let id = run(&mut session, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "palette": "Brand", "global": true }))["id"].as_u64().unwrap();
    assert_eq!(session.run(&Command::with_params("swatch.add", json!({ "colour": "#FF0000", "palette": "Nope" }))).unwrap_err().code, "PALETTE_NOT_FOUND");
    run(&mut session, "swatch.apply", json!({ "id": id, "ids": [1] }));
    run(&mut session, "palette.rename", json!({ "name": "Brand", "to": "Corp" }));
    assert_eq!(session.document.swatches[0].palette, "Corp");
    let listed = run(&mut session, "palette.list", json!({ "scope": "document" }));
    assert_eq!(listed["document"][1]["name"], "Corp");
    assert_eq!(listed["document"][1]["swatches"][0]["id"], id);
    run(&mut session, "palette.delete", json!({ "name": "Corp" }));
    assert!(session.document.swatches.is_empty());
    assert!(session.document.swatch_bindings.is_empty());
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatches.len(), 1);
    assert_eq!(session.document.swatch_bindings.len(), 1);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.palettes, vec!["Brand".to_string()]);
}

#[test]
fn sort_view_search_is_view_state_only() {
    let mut session = session();
    for (name, colour) in [("zeta red", "#FF0000"), ("Alpha blue", "#0000FF"), ("mid red", "#800000")] {
        run(&mut session, "swatch.add", json!({ "name": name, "colour": colour }));
    }
    let ids: Vec<u64> = session.document.swatches.iter().map(|swatch| swatch.id).collect();
    let undo_before = session.undo.can_undo();
    let reply = run(&mut session, "palette.sort", json!({ "scope": "document", "palette": "", "by": "name", "search": "red" }));
    assert_eq!(reply["order"], json!([ids[2], ids[0]]));
    let reply = run(&mut session, "palette.view", json!({ "view": "grid", "search": "" }));
    assert_eq!(reply["state"]["view"], "grid");
    assert_eq!(reply["order"], json!([ids[1], ids[2], ids[0]]));
    assert_eq!(session.document.swatches.iter().map(|swatch| swatch.id).collect::<Vec<_>>(), ids);
    assert_eq!(session.undo.can_undo(), undo_before);
    assert_eq!(ordered_ids(&session.document.swatches, PaletteSort::Lightness, "")[0], ids[2]);
}

#[test]
fn application_palette_is_stored_outside_document_and_undo() {
    let path = temp("app_palettes.json");
    dtool_core::palette::set_app_path(Some(path.clone()));
    let mut session = session();
    let reply = run(&mut session, "swatch.add", json!({ "name": "App red", "colour": "#FF0000", "scope": "application" }));
    assert_eq!(reply["scope"], "application");
    assert!(session.document.swatches.is_empty());
    assert!(!session.undo.can_undo());
    run(&mut session, "palette.create", json!({ "name": "Studio", "scope": "application" }));
    run(&mut session, "palette.rename", json!({ "name": "Studio", "to": "Lab", "scope": "application" }));
    let listed = run(&mut session, "palette.list", json!({ "scope": "application" }));
    let names: Vec<&str> = listed["application"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.contains(&dtool_core::palette::APP_DEFAULT) && names.contains(&"Lab"));
    let saved = dtool_core::palette::load_app(&path);
    assert_eq!(saved.palettes.iter().find(|p| p.name == dtool_core::palette::APP_DEFAULT).unwrap().swatches[0].name, "App red");
    let file = temp("app_export.json");
    run(&mut session, "palette.export", json!({ "path": file, "scope": "application" }));
    run(&mut session, "palette.import", json!({ "path": file, "scope": "application", "on_conflict": "rename" }));
    let listed = run(&mut session, "palette.list", json!({ "scope": "application" }));
    assert!(listed["application"].as_array().unwrap().iter().any(|p| p["name"] == "Application 2"));
    run(&mut session, "palette.delete", json!({ "name": "Lab", "scope": "application" }));
    assert!(!session.undo.can_undo());
    let mut empty = Session::new(Document::new([10.0, 10.0], Units::Pt, 72.0, 1));
    empty.has_document = false;
    assert!(empty.run(&Command::with_params("palette.list", json!({ "scope": "application" }))).is_ok());
}