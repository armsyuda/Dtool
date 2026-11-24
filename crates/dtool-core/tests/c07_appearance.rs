use dtool_core::commands::appearance::{edit_list, query_json, AppearanceList, AppearanceSlots, Kind};
use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint, PaintOrder, StrokeStyle};
use dtool_core::{Object, ObjectId, Session};
use dtool_protocol::{Command, CommandError};
use serde_json::json;

fn solid(v: u8) -> Paint {
    Paint::Solid { colour: Colour::new(v, v, v, 255) }
}

fn stroke(width: f32) -> StrokeStyle {
    StrokeStyle { paint: solid(10), width, ..StrokeStyle::default() }
}

fn cmd(name: &str, params: serde_json::Value) -> Command {
    Command::with_params(name, params)
}

#[derive(Default)]
struct Multi {
    list: AppearanceList,
}

impl AppearanceSlots for Multi {
    fn appearance(&self) -> AppearanceList {
        self.list.clone()
    }
    fn set_appearance(&mut self, list: &AppearanceList) -> Result<(), CommandError> {
        self.list = list.clone();
        Ok(())
    }
    fn paint_order(&self) -> PaintOrder {
        PaintOrder::StrokeOver
    }
}

fn step(target: &mut Multi, command: Command) -> bool {
    let mut list = target.appearance();
    let changed = edit_list(&command, &mut list).unwrap();
    target.set_appearance(&list).unwrap();
    changed
}

#[test]
fn two_fills_two_strokes_reorder_and_round_trip_on_list_api() {
    let mut m = Multi::default();
    assert!(step(&mut m, cmd("appearance.add_fill", json!({ "paint": solid(1) }))));
    assert!(step(&mut m, cmd("appearance.add_fill", json!({ "paint": solid(2) }))));
    assert!(step(&mut m, cmd("appearance.add_stroke", json!({ "stroke": stroke(1.0) }))));
    assert!(step(&mut m, cmd("appearance.add_stroke", json!({ "stroke": stroke(2.0) }))));
    assert_eq!(m.list.fills, vec![solid(1), solid(2)]);
    assert!(step(&mut m, cmd("appearance.reorder", json!({ "kind": "fill", "from": 1, "to": 0 }))));
    assert_eq!(m.list.fills, vec![solid(2), solid(1)]);
    assert!(!step(&mut m, cmd("appearance.reorder", json!({ "kind": "stroke", "from": 1, "to": 1 }))));
    assert!(step(&mut m, cmd("appearance.remove", json!({ "kind": "stroke", "index": 1 }))));
    assert_eq!(m.list.strokes, vec![stroke(1.0)]);
    let mut list = m.appearance();
    assert!(edit_list(&cmd("appearance.remove", json!({ "kind": "fill", "index": 5 })), &mut list).is_err());
    assert!(edit_list(&cmd("appearance.reorder", json!({ "kind": "mask", "from": 0, "to": 0 })), &mut list).is_err());
    let q = query_json(1, &m.list, PaintOrder::StrokeOver);
    assert_eq!(q["render_order"], json!([{ "kind": "fill", "index": 0 }, { "kind": "fill", "index": 1 }, { "kind": "stroke", "index": 0 }]));
    assert_eq!(m.list.render_order(PaintOrder::StrokeUnder)[0], (Kind::Stroke, 0));
    let saved = serde_json::to_string(&m.list).unwrap();
    let loaded: AppearanceList = serde_json::from_str(&saved).unwrap();
    assert_eq!(loaded, m.list);
    assert_eq!(serde_json::to_string(&loaded).unwrap(), saved);
}

fn session() -> Session {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(1), ..Object::default() });
    Session::new(doc)
}

fn object(s: &Session) -> Object {
    s.document.get_object(ObjectId(1)).unwrap().clone()
}

#[test]
fn live_commands_are_one_undo_each_and_never_overwrite() {
    let mut s = session();
    let g0 = s.generation;
    s.run(&cmd("appearance.add_fill", json!({ "node": 1, "paint": solid(50) }))).unwrap();
    assert_eq!(object(&s).fill, solid(50));
    assert_eq!(s.generation, g0 + 1);
    s.run(&cmd("appearance.add_fill", json!({ "node": 1, "paint": solid(60) }))).unwrap();
    assert_eq!(object(&s).appearance().fills, vec![solid(50), solid(60)]);
    s.run(&cmd("edit.undo", json!({}))).unwrap();
    assert_eq!(object(&s).appearance().fills, vec![solid(50)]);
    let after_undo = s.generation;
    s.run(&cmd("appearance.add_stroke", json!({ "node": 1, "stroke": stroke(3.0) }))).unwrap();
    let noop = s.run(&cmd("appearance.reorder", json!({ "node": 1, "kind": "fill", "from": 0, "to": 0 }))).unwrap();
    assert_eq!(noop["changes"], json!([]));
    assert_eq!(s.generation, after_undo + 1);
    let q = s.run(&cmd("appearance.query", json!({ "node": 1 }))).unwrap();
    assert_eq!(q["fills"].as_array().unwrap().len(), 1);
    assert_eq!(q["render_order"], json!([{ "kind": "fill", "index": 0 }, { "kind": "stroke", "index": 0 }]));
    s.run(&cmd("appearance.remove", json!({ "node": 1, "kind": "fill", "index": 0 }))).unwrap();
    assert_eq!(object(&s).fill, Paint::None);
    s.run(&cmd("edit.undo", json!({}))).unwrap();
    assert_eq!(object(&s).fill, solid(50));
    s.run(&cmd("edit.undo", json!({}))).unwrap();
    assert_eq!(object(&s).stroke, StrokeStyle::default());
    assert_eq!(object(&s).fill, solid(50));
    s.run(&cmd("edit.undo", json!({}))).unwrap();
    assert_eq!(object(&s).fill, Paint::None);
    assert!(!s.undo.can_undo());
    s.run(&cmd("edit.redo", json!({}))).unwrap();
    assert_eq!(object(&s).fill, solid(50));
}
