use dtool_core::command::execute_object_update;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectUpdateArgs};
use dtool_core::paint::{Colour, Paint, PaintParseError, normalize_legacy_paint_alias, paint_from_json_value};

fn fill_set(colour: &str) -> Option<Paint> {
    normalize_legacy_paint_alias(Some(colour)).ok()
}

#[test]
fn test_fill_set_hex_rgb_alias() {
    let paint = fill_set("#FF0000").unwrap();
    assert_eq!(paint, Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
}

#[test]
fn test_fill_set_hex_rgba_alias() {
    let paint = fill_set("#FF000080").unwrap();
    assert_eq!(paint, Paint::Solid { colour: Colour::new(255, 0, 0, 128) });
}

#[test]
fn test_fill_set_null_returns_none() {
    let paint = normalize_legacy_paint_alias(None).ok().unwrap();
    assert_eq!(paint, Paint::None);
    assert_eq!(paint_from_json_value(&serde_json::Value::Null), Paint::None);
}

#[test]
fn test_fill_set_empty_returns_none() {
    let paint = fill_set("").unwrap();
    assert_eq!(paint, Paint::None);
}

#[test]
fn test_fill_set_invalid_alias_error() {
    let result = normalize_legacy_paint_alias(Some("INVALID_PARAMS"));
    assert!(matches!(result, Err(PaintParseError::InvalidParams)));
}

#[test]
fn test_fill_set_failed_envelope_preserves_previous_paint() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 96.0, 1);
    let id = doc.create_object(Object { name: "target".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let green = fill_set("#00FF00").unwrap();
    let initial = ObjectUpdateArgs { id: id.0, name: None, pos: None, size: None, fill: Some(green.clone()), stroke: None, opacity: None, transparency: None };
    execute_object_update(&mut doc, initial).unwrap();
    assert!(fill_set("INVALID_PARAMS").is_none());
    let failed = ObjectUpdateArgs { id: id.0, name: None, pos: None, size: Some([0.0, 0.0]), fill: Some(Paint::None), stroke: None, opacity: None, transparency: None };
    let result = execute_object_update(&mut doc, failed);
    assert!(result.is_err());
    let paint = doc.get_object(id).unwrap().fill.clone();
    assert_eq!(paint, Paint::Solid { colour: Colour::new(0, 255, 0, 255) });
}
