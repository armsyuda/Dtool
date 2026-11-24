use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint, StrokeStyle};
use dtool_core::selection::SelectOp;
use dtool_core::{Object, ObjectDefaults, Session};
use serde_json::json;
use std::path::PathBuf;

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn near_all(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y))
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c07_defaults_units");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn stored(width: f32, dash: Vec<f32>, unit: Option<&str>) -> serde_json::Value {
    let mut defaults = ObjectDefaults::default();
    defaults.stroke = StrokeStyle { paint: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, width, dash, ..StrokeStyle::default() };
    let mut value = serde_json::to_value(&defaults).unwrap();
    let map = value.as_object_mut().unwrap();
    match unit {
        Some(unit) => {
            map.insert("unit".into(), json!(unit));
        }
        None => {
            map.remove("unit");
        }
    }
    value
}

#[test]
fn c07_units_factory_and_saved_files_carry_pt_marker() {
    assert_eq!(ObjectDefaults::default().unit.as_deref(), Some("pt"));
    let path = scratch("factory.json");
    ObjectDefaults::default().save_to(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["unit"], json!("pt"));
    assert_eq!(ObjectDefaults::load_from(&path).unwrap(), ObjectDefaults::default());
}

#[test]
fn c07_units_legacy_px_file_keeps_px_and_migrates_on_save() {
    let path = scratch("legacy.json");
    std::fs::write(&path, serde_json::to_string_pretty(&stored(2.0, vec![4.0, 2.0], None)).unwrap()).unwrap();
    let loaded = ObjectDefaults::load_from(&path).unwrap();
    assert_eq!(loaded.unit.as_deref(), Some("pt"));
    assert!(near(loaded.stroke.width, 1.5));
    assert!(near_all(&loaded.stroke.dash, &[3.0, 1.5]));
    let mut object = Object::default();
    loaded.apply_to(&mut object, 96.0);
    assert!(near(object.stroke.width, 2.0));
    assert!(near_all(&object.stroke.dash, &[4.0, 2.0]));
    loaded.save_to(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["unit"], json!("pt"));
    let reloaded = ObjectDefaults::load_from(&path).unwrap();
    assert_eq!(reloaded, loaded);
    let mut again = Object::default();
    reloaded.apply_to(&mut again, 96.0);
    assert_eq!(again.stroke, object.stroke);
}

#[test]
fn c07_units_pt_file_loads_unchanged() {
    let path = scratch("points.json");
    std::fs::write(&path, serde_json::to_string(&stored(1.0, vec![3.0, 1.5], Some("pt"))).unwrap()).unwrap();
    let loaded = ObjectDefaults::load_from(&path).unwrap();
    assert!(near(loaded.stroke.width, 1.0));
    assert!(near_all(&loaded.stroke.dash, &[3.0, 1.5]));
    let px = loaded.stroke_for(96.0);
    assert!(near(px.width, 96.0 / 72.0));
    assert!(near_all(&px.dash, &[4.0, 2.0]));
}

#[test]
fn c07_units_unknown_marker_falls_back_to_factory() {
    let path = scratch("unknown.json");
    std::fs::write(&path, serde_json::to_string(&stored(9.0, vec![], Some("mm"))).unwrap()).unwrap();
    assert_eq!(ObjectDefaults::load_from(&path).unwrap(), ObjectDefaults::default());
}

#[test]
fn c07_units_sync_and_apply_convert_width_and_dash_with_document_dpi() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 144.0, 1));
    let stroke = StrokeStyle { paint: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, width: 4.0, dash: vec![8.0, 4.0], ..StrokeStyle::default() };
    let id = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], stroke: stroke.clone(), ..Object::default() });
    s.selection.set(vec![id], SelectOp::Replace);
    let mut defaults = ObjectDefaults::default();
    defaults.sync_from_selection(&s.document, &s.selection);
    assert!(near(defaults.stroke.width, 2.0));
    assert!(near_all(&defaults.stroke.dash, &[4.0, 2.0]));
    let mut direct = ObjectDefaults::default();
    direct.sync_from_object(s.document.get_object(id).unwrap(), 144.0);
    assert_eq!(direct, defaults);
    let mut back = Object::default();
    defaults.apply_to(&mut back, 144.0);
    assert_eq!(back.stroke, stroke);
    let mut low = Object::default();
    defaults.apply_to(&mut low, 72.0);
    assert!(near(low.stroke.width, 2.0));
    assert!(near_all(&low.stroke.dash, &[4.0, 2.0]));
}

#[test]
fn c07_units_from_json_marks_legacy_values_as_converted() {
    let legacy = ObjectDefaults::from_json(stored(96.0, vec![48.0], None)).unwrap();
    assert!(near(legacy.stroke.width, 72.0));
    assert!(near_all(&legacy.stroke.dash, &[36.0]));
    let points = ObjectDefaults::from_json(stored(96.0, vec![48.0], Some("pt"))).unwrap();
    assert!(near(points.stroke.width, 96.0));
}