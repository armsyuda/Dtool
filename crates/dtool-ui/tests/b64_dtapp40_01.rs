use dtool_core::document::{Document, Units};
use dtool_core::pixel::PixelLayer;
use dtool_core::{ObjectId, Session};
use serde_json::{json, Value};

const LAYER: u64 = 7;

fn session() -> Session {
    let mut s = Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1));
    let rgba: Vec<u8> = (0..100 * 100).flat_map(|i| [(i % 251) as u8, (i % 97) as u8, 40, 255]).collect();
    s.document.pixel_layers.insert(ObjectId(LAYER), PixelLayer::from_rgba(100, 100, &rgba));
    s
}

fn pixels(s: &Session) -> Value { serde_json::to_value(s.document.pixel_layers.get(ObjectId(LAYER)).unwrap()).unwrap() }

fn params(mode: &str, dst: Value) -> Value {
    json!({"layer": LAYER, "mode": mode, "src": [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], "dst": dst})
}

#[test]
fn perspective_adapter_reuses_engine() {
    let mut s = session();
    let dst = json!([[10.0, 0.0], [90.0, 10.0], [100.0, 100.0], [0.0, 90.0]]);
    let before = pixels(&s);
    let preview = s.run_named("pixel.perspective.preview", params("single", dst.clone())).unwrap();
    assert_eq!(preview["preview"], json!(true));
    assert_eq!(pixels(&s), before);
    let out = s.run_named("pixel.perspective.apply", params("single", dst)).unwrap();
    assert_eq!(out["changed"], json!(true));
    assert_ne!(pixels(&s), before);
}

#[test]
fn pixel_handles_modes() {
    let mut s = session();
    let dst = json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]]);
    assert!(s.run_named("pixel.perspective.preview", params("single", dst.clone())).is_ok());
    assert!(s.run_named("pixel.perspective.preview", params("two", dst.clone())).is_ok());
    assert!(s.run_named("pixel.perspective.preview", params("bogus", dst)).is_err());
}

#[test]
fn apply_one_undo_cancel_no_change() {
    let mut s = session();
    let before = pixels(&s);
    s.run_named("pixel.perspective.apply", params("single", json!([[10.0, 10.0], [90.0, 10.0], [90.0, 90.0], [10.0, 90.0]]))).unwrap();
    assert_ne!(pixels(&s), before);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pixels(&s), before);
    s.run_named("pixel.perspective.preview", params("single", json!([[20.0, 20.0], [80.0, 20.0], [80.0, 80.0], [20.0, 80.0]]))).unwrap();
    s.run_named("pixel.perspective.cancel", json!({})).unwrap();
    assert_eq!(pixels(&s), before);
}
