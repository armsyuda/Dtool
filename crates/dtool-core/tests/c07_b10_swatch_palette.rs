use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c07_b10_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn fresh() -> (Workspace, ObjectId) {
    dtool_core::palette::set_app_path(Some(tmp("app_palettes.json")));
    let mut workspace = Workspace::default();
    workspace.new_document("c07_b10");
    let session = workspace.session_mut().unwrap();
    let id = session.document.create_object(Object { name: "box".into(), size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(0x33, 0x66, 0x99, 0x80) }, ..Object::default() });
    session.selection.ids = vec![id];
    (workspace, id)
}

fn run(workspace: &mut Workspace, name: &str, params: Value) -> Value {
    workspace.run(Scope::active(), Command::with_params(name, params)).1.unwrap_or_else(|error| panic!("{name}: {error:?}"))
}

fn fill(workspace: &Workspace, id: ObjectId) -> Paint {
    workspace.session().unwrap().document.get_object(id).unwrap().fill.clone()
}

#[test]
fn c07_b10_swatch_from_selection_keeps_hex_alpha_on_live_path() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut workspace, id) = fresh();
    let added = run(&mut workspace, "swatch.add_from_selection", json!({}));
    let swatches = workspace.session().unwrap().document.swatches.clone();
    assert_eq!(swatches.len(), 1, "{added}");
    assert_eq!(swatches[0].colour, Colour::new(0x33, 0x66, 0x99, 0x80));
    assert_eq!(swatches[0].name, "#33669980");
    let plain = run(&mut workspace, "swatch.add", json!({ "colour": "#33669980" }))["id"].as_u64().unwrap();
    let plain = workspace.session().unwrap().document.swatches.iter().find(|swatch| swatch.id == plain).cloned().unwrap();
    assert_eq!(plain.name, "#33669980");
    assert_eq!(plain.colour.a, 0x80);
    run(&mut workspace, "object.update", json!({ "id": id.0, "fill": Paint::Solid { colour: Colour::new(0, 0, 0, 255) } }));
    run(&mut workspace, "swatch.apply_to_selection", json!({ "swatch": plain.id.to_string(), "to": "fill" }));
    match fill(&workspace, id) {
        Paint::Solid { colour } => assert_eq!(colour, Colour::new(0x33, 0x66, 0x99, 0x80)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn c07_b10_pick_and_recent_report_hex_alpha() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (mut workspace, _) = fresh();
    let session = workspace.session_mut().unwrap();
    let mut pixels = Vec::new();
    for _ in 0..16 {
        pixels.extend_from_slice(&[0x33, 0x66, 0x99, 0x80]);
    }
    session.document.create_object(Object { name: "img".into(), pos: [50.0, 50.0], size: [4.0, 4.0], raster_data: Some(pixels), raster_size: Some((4, 4)), ..Object::default() });
    let picked = run(&mut workspace, "color.pick", json!({ "x": 51.5, "y": 51.5, "mode": "point", "apply": false }));
    let hex = picked["colour"]["hex"].as_str().unwrap().to_string();
    let rgba: Vec<u64> = picked["colour"]["rgba"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(hex.len(), if rgba[3] < 255 { 9 } else { 7 }, "{picked}");
    let decoded = dtool_core::paint::paint_from_legacy_str(&hex).unwrap();
    match decoded {
        Paint::Solid { colour } => assert_eq!(vec![colour.r as u64, colour.g as u64, colour.b as u64, colour.a as u64], rgba, "{picked}"),
        other => panic!("{other:?}"),
    }
    dtool_core::palette::push_recent(Colour::new(0x33, 0x66, 0x99, 0x80)).unwrap();
    let recent = run(&mut workspace, "color.recent.list", json!({}));
    assert_eq!(recent["recent"][0]["hex"], "#33669980", "{recent}");
}

#[test]
fn c07_b10_palette_round_trip_json_and_ase_on_live_path() {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    for format in ["json", "ase"] {
        let (mut workspace, _) = fresh();
        run(&mut workspace, "palette.create", json!({ "name": "Brand" }));
        run(&mut workspace, "swatch.add", json!({ "name": "Red", "colour": "#FF0000", "palette": "Brand", "global": true }));
        run(&mut workspace, "swatch.add", json!({ "name": "Blue", "colour": "#0000FF", "palette": "Brand" }));
        run(&mut workspace, "swatch.add", json!({ "name": "Green", "colour": "#00FF00", "palette": "Brand" }));
        let path = tmp(&format!("brand.{format}"));
        let exported = run(&mut workspace, "palette.export", json!({ "path": path, "format": format, "scope": "document", "palette": "Brand" }));
        assert_eq!(exported["count"], 3, "{format} {exported}");
        let (mut other, _) = fresh();
        let imported = run(&mut other, "palette.import", json!({ "path": path, "format": format, "scope": "document", "palette_name": "Brand", "on_conflict": "rename" }));
        assert_eq!(imported["added"], 3, "{format} {imported}");
        let names: Vec<String> = other.session().unwrap().document.swatches.iter().map(|swatch| swatch.name.clone()).collect();
        assert_eq!(names, ["Red", "Blue", "Green"], "{format}");
        let colours: Vec<Colour> = other.session().unwrap().document.swatches.iter().map(|swatch| swatch.colour).collect();
        assert_eq!(colours, [Colour::new(255, 0, 0, 255), Colour::new(0, 0, 255, 255), Colour::new(0, 255, 0, 255)], "{format}");
        run(&mut other, "edit.undo", json!({}));
        assert!(other.session().unwrap().document.swatches.is_empty(), "{format} import is one undo step");
        let listed = run(&mut workspace, "palette.list", json!({ "scope": "document" }));
        assert!(listed.to_string().contains("Brand"), "{listed}");
    }
    let (mut workspace, _) = fresh();
    let bad = workspace.run(Scope::active(), Command::with_params("palette.import", json!({ "path": tmp("missing.ase"), "format": "ase", "scope": "document" }))).1;
    assert!(bad.is_err());
}