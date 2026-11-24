use dtool_core::document::{Document, Units};
use dtool_core::grid::GridKind;
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new(Document::new([720.0, 360.0], Units::Pt, 72.0, 1))
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

#[test]
fn grid_set_lives_on_document_and_undoes() {
    let mut s = session();
    let result = run(&mut s, "grid.set", json!({"kind": "Isometric", "show": true, "spacing": [10.0, 20.0], "subdivisions": 2}));
    assert_eq!(result["grid"]["kind"], "Isometric");
    assert_eq!(s.document.grid.kind, GridKind::Isometric);
    assert_eq!(s.document.grid.spacing, [10.0, 20.0]);
    assert!(s.undo.can_undo());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document.grid.kind, GridKind::Standard);
    assert!(!s.document.grid.show);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.document.grid.spacing, [10.0, 20.0]);
    let query = run(&mut s, "grid.query", json!({}));
    assert_eq!(query["grid"]["subdivisions"], 2);
}

#[test]
fn grid_set_rejects_bad_values_and_unknown_fields() {
    let mut s = session();
    for params in [json!({"spacing": [0.0, 1.0]}), json!({"subdivisions": 0}), json!({"baseline_spacing": -1.0}), json!({"bogus": 1})] {
        assert_eq!(s.run(&Command::with_params("grid.set", params)).unwrap_err().code, "INVALID_PARAMS");
    }
    assert!(!s.undo.can_undo());
}

#[test]
fn unchanged_grid_set_adds_no_undo_step() {
    let mut s = session();
    run(&mut s, "grid.set", json!({"show": false}));
    assert!(!s.undo.can_undo());
}

#[test]
fn baseline_spacing_is_separate_from_grid_spacing() {
    let mut s = session();
    run(&mut s, "grid.set", json!({"baseline_spacing": 18.0, "show_baseline": true}));
    assert_eq!(s.document.grid.baseline_spacing, 18.0);
    assert_eq!(s.document.grid.spacing, [64.0, 64.0]);
}

#[test]
fn grid_and_rulers_round_trip_with_document_file() {
    let mut s = session();
    run(&mut s, "grid.set", json!({"kind": "Axonometric", "spacing": [5.0, 7.0]}));
    run(&mut s, "ruler.origin.set", json!({"origin": [12.0, 34.0]}));
    run(&mut s, "ruler.unit.set", json!({"units": "Mm"}));
    let text = serde_json::to_string(&s.document).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.grid, s.document.grid);
    assert_eq!(loaded.rulers.origin, [12.0, 34.0]);
    assert_eq!(loaded.rulers.units, Some(Units::Mm));
}

#[test]
fn old_document_without_grid_or_rulers_loads_defaults() {
    let mut value = serde_json::to_value(Document::default()).unwrap();
    let map = value.as_object_mut().unwrap();
    map.remove("grid");
    map.remove("rulers");
    let loaded: Document = serde_json::from_value(value).unwrap();
    assert_eq!(loaded.grid, dtool_core::grid::Grid::default());
    assert_eq!(loaded.rulers, dtool_core::RulerState::default());
    let partial: dtool_core::grid::Grid = serde_json::from_value(json!({"show": true})).unwrap();
    assert!(partial.show);
    assert_eq!(partial.subdivisions, 4);
}

#[test]
fn ruler_origin_and_units_undo_show_does_not() {
    let mut s = session();
    run(&mut s, "ruler.show", json!({"show": true}));
    assert!(s.document.rulers.show);
    assert!(!s.undo.can_undo());
    run(&mut s, "ruler.origin.set", json!({"origin": [5.0, 6.0]}));
    run(&mut s, "ruler.unit.set", json!({"units": "Cm"}));
    run(&mut s, "ruler.origin.reset", json!({}));
    assert_eq!(s.document.rulers.origin, [0.0, 0.0]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document.rulers.origin, [5.0, 6.0]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document.rulers.units, None);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document.rulers.origin, [0.0, 0.0]);
    assert!(s.document.rulers.show);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.document.rulers.origin, [5.0, 6.0]);
}

#[test]
fn grid_and_ruler_commands_are_catalogued_and_preflighted() {
    let s = session();
    for name in ["grid.set", "grid.query", "ruler.show", "ruler.origin.set", "ruler.origin.reset", "ruler.unit.set"] {
        assert!(s.catalog.contains(name), "{name}");
    }
    assert!(s.can_execute(&Command::with_params("grid.query", json!({}))));
    assert!(!s.can_execute(&Command::with_params("ruler.origin.set", json!({"origin": "x"}))));
}

#[test]
fn ruler_units_display_scale_uses_document_rulers() {
    let mut s = session();
    run(&mut s, "ruler.unit.set", json!({"units": "In"}));
    let scale = dtool_core::commands::ruler_units::display_scale(&s);
    assert!((scale - 1.0 / 72.0).abs() < 1e-6);
}

#[test]
fn snap_candidates_reports_grid_points_when_grid_snapping_on() {
    let mut s = session();
    let mut prefs = s.snap_prefs.query().clone();
    prefs.enabled = true;
    prefs.snap_to_grid = true;
    s.snap_prefs.replace(prefs);
    run(&mut s, "grid.set", json!({"show": true, "spacing": [10.0, 10.0], "subdivisions": 1}));
    let result = run(&mut s, "snap.candidates", json!({"position": [12.0, 19.0]}));
    let candidates = result["candidates"].as_array().unwrap();
    assert!(candidates.iter().any(|c| c["kind"] == "grid"), "{result}");
    assert_eq!(s.run(&Command::with_params("snap.candidates", json!({"position": [1.0]}))).unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn smart_guide_feedback_never_persists_a_guide() {
    let mut s = session();
    let before = serde_json::to_value(&s.guides).ok();
    let result = run(&mut s, "snap.smart_guide.feedback", json!({"kind": "Vertical", "position": 50.0}));
    assert_eq!(result["persisted"], false);
    assert_eq!(serde_json::to_value(&s.guides).ok(), before);
    assert!(!s.undo.can_undo());
}
