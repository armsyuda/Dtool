use dtool_core::commands::stroke::STROKE_SET;
use dtool_core::curve::{Contour, PathPoint, PointId};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::outline::{apply_corners, corner_eligible};
use dtool_core::paint::{Cap, Join, StrokeAlign};
use dtool_core::{Command, Session};
use serde_json::json;

fn session() -> (Session, ObjectId) {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    s.has_document = true;
    let id = s.document.create_object(Object { pos: [10.0, 10.0], size: [50.0, 50.0], ..Object::default() });
    (s, id)
}

fn set(s: &mut Session, params: serde_json::Value) -> Result<serde_json::Value, String> {
    s.run(&Command::with_params(STROKE_SET, params)).map_err(|e| e.code)
}

fn full(id: ObjectId) -> serde_json::Value {
    json!({
        "object_id": id.0, "width": 3.0, "dash": [4.0, 2.0], "cap": "Round", "join": "Bevel", "align": "Inside",
        "start_arrow": { "name": "arrow", "scale": 1.5 }, "end_arrow": null, "scale_with_object": true,
        "pressure": [[0.0, 1.0], [0.5, 0.25], [1.0, 1.0]]
    })
}

#[test]
fn stroke_set_cap_join_align_arrows_pressure_persist() {
    let (mut s, id) = session();
    set(&mut s, full(id)).unwrap();
    let st = &s.document.get_object(id).unwrap().stroke;
    assert_eq!(st.width, 3.0);
    assert_eq!(st.dash, vec![4.0, 2.0]);
    assert_eq!(st.cap, Cap::Round);
    assert_eq!(st.join, Join::Bevel);
    assert_eq!(st.align, StrokeAlign::Inside);
    assert_eq!(st.start_arrow.as_ref().map(|a| a.name.as_str()), Some("arrow"));
    assert!(st.scale_with_object);
    assert_eq!(st.pressure, vec![[0.0, 1.0], [0.5, 0.25], [1.0, 1.0]]);
    let q = s.run(&Command::with_params("stroke.query", json!({ "object_id": id.0 }))).unwrap();
    assert_eq!(q["stroke"]["pressure"], json!([[0.0, 1.0], [0.5, 0.25], [1.0, 1.0]]));
}

#[test]
fn absent_keys_keep_current_values() {
    let (mut s, id) = session();
    set(&mut s, full(id)).unwrap();
    set(&mut s, json!({ "object_id": id.0, "width": 5.0 })).unwrap();
    let st = &s.document.get_object(id).unwrap().stroke;
    assert_eq!(st.width, 5.0);
    assert_eq!(st.cap, Cap::Round);
    assert_eq!(st.pressure.len(), 3);
}

#[test]
fn stroke_set_rejects_bad_pressure_and_bad_enums() {
    let (mut s, id) = session();
    let before = s.document.get_object(id).unwrap().stroke.clone();
    for bad in [json!([[1.5, 1.0]]), json!([[0.5, -1.0]]), json!([[0.6, 1.0], [0.2, 1.0]])] {
        assert_eq!(set(&mut s, json!({ "object_id": id.0, "pressure": bad })), Err("INVALID_PARAMS".to_string()));
    }
    assert_eq!(set(&mut s, json!({ "object_id": id.0, "cap": "Pointy" })), Err("INVALID_PARAMS".to_string()));
    assert_eq!(s.document.get_object(id).unwrap().stroke, before);
    assert!(!s.undo.can_undo());
}

#[test]
fn stroke_set_noop_pushes_no_undo() {
    let (mut s, id) = session();
    set(&mut s, full(id)).unwrap();
    let steps = s.undo.applied_steps();
    let reply = set(&mut s, full(id)).unwrap();
    assert_eq!(reply["changes"], json!([]));
    assert_eq!(s.undo.applied_steps(), steps);
}

#[test]
fn stroke_set_undo_restores_all_fields() {
    let (mut s, id) = session();
    let before = s.document.get_object(id).unwrap().stroke.clone();
    set(&mut s, full(id)).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.get_object(id).unwrap().stroke, before);
}

fn square(closed: bool) -> Contour {
    let pts = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
    Contour { points: pts.iter().enumerate().map(|(i, p)| PathPoint::sharp(PointId(i as u64 + 1), *p)).collect(), closed }
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn at(kind: &'static str, r: f64) -> Vec<Option<(&'static str, f64)>> {
    vec![None, Some((kind, r)), None, None]
}

#[test]
fn rounded_corner_is_tangent_arc() {
    let out = apply_corners(&square(true), &at("rounded", 10.0));
    assert_eq!(out.points.len(), 5);
    let (a, b) = (&out.points[1], &out.points[2]);
    assert!(near(a.pos, [90.0, 0.0]) && near(b.pos, [100.0, 10.0]));
    let k = 4.0 / 3.0 * (std::f64::consts::PI / 8.0).tan();
    assert!(near(a.handle_out.unwrap(), [90.0 + 10.0 * k, 0.0]));
    assert!(near(b.handle_in.unwrap(), [100.0, 10.0 - 10.0 * k]));
    assert_eq!(a.id, PointId(2));
    let mut ids: Vec<u64> = out.points.iter().map(|p| p.id.0).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 5);
}

#[test]
fn straight_concave_cutout_geometry() {
    let s = apply_corners(&square(true), &at("straight", 10.0));
    assert!(near(s.points[1].pos, [90.0, 0.0]) && near(s.points[2].pos, [100.0, 10.0]) && s.points[1].handle_out.is_none());
    let c = apply_corners(&square(true), &at("concave", 10.0));
    let (a, b) = (&c.points[1], &c.points[2]);
    let (h1, h2) = (a.handle_out.unwrap(), b.handle_in.unwrap());
    let t = 0.5;
    let u = 1.0 - t;
    let mid = [0, 1].map(|k| u * u * u * a.pos[k] + 3.0 * u * u * t * h1[k] + 3.0 * u * t * t * h2[k] + t * t * t * b.pos[k]);
    assert!(((mid[0] - 100.0).hypot(mid[1]) - 10.0).abs() < 0.05, "{mid:?}");
    assert!(mid[0] < 100.0 && mid[1] > 0.0);
    let x = apply_corners(&square(true), &at("cutout", 10.0));
    assert_eq!(x.points.len(), 6);
    assert!(near(x.points[1].pos, [90.0, 0.0]) && near(x.points[2].pos, [90.0, 10.0]) && near(x.points[3].pos, [100.0, 10.0]));
}

#[test]
fn radius_clamps_to_half_side_and_none_is_identity() {
    let out = apply_corners(&square(true), &at("straight", 500.0));
    assert!(near(out.points[1].pos, [50.0, 0.0]) && near(out.points[2].pos, [100.0, 50.0]));
    assert_eq!(apply_corners(&square(true), &at("none", 10.0)), square(true));
    assert_eq!(apply_corners(&square(true), &at("bogus", 10.0)), square(true));
    assert_eq!(apply_corners(&square(true), &at("rounded", 0.0)), square(true));
}

#[test]
fn open_endpoints_and_curved_sides_are_not_eligible() {
    let open = square(false);
    assert!(!corner_eligible(&open, 0) && !corner_eligible(&open, 3) && corner_eligible(&open, 1));
    let out = apply_corners(&open, &[Some(("rounded", 10.0)), None, None, Some(("rounded", 10.0))]);
    assert_eq!(out, open);
    let mut curved = square(true);
    curved.points[1].handle_in = Some([80.0, 10.0]);
    assert!(!corner_eligible(&curved, 1));
    assert_eq!(apply_corners(&curved, &at("rounded", 10.0)), curved);
}