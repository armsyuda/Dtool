#[path = "../src/text_wrap.rs"]
mod text_wrap;

pub use dtool_core::{CommandError, ObjectId};
use serde_json::json;
use text_wrap::{line_spans, TextWrap, WrapMode, WrapSides, WrapTable, CLEAR, IGNORE, QUERY, SET};

#[test]
fn set_side_and_distance_then_query_and_undo() {
    let table = WrapTable::default();
    let undo = table.clone();
    let next = table.edited(SET, &json!({"id": 7, "mode": "square", "sides": "left", "distance": {"left": 4.0, "right": 6.0}})).unwrap().unwrap();
    let value = next.query(ObjectId(7));
    assert_eq!(value["wrap"]["mode"], "square");
    assert_eq!(value["wrap"]["sides"], "left");
    assert_eq!(value["wrap"]["distance"], json!([0.0, 6.0, 0.0, 4.0]));
    assert_eq!(value["ignore"], false);
    let next = next.edited(SET, &json!({"id": 7, "distance": 2.0})).unwrap().unwrap();
    assert_eq!(next.query(ObjectId(7))["wrap"]["distance"], json!([2.0, 2.0, 2.0, 2.0]));
    assert_eq!(next.query(ObjectId(7))["wrap"]["sides"], "left");
    let restored = undo;
    assert_eq!(restored.query(ObjectId(7))["wrap"], serde_json::Value::Null);
    let reopened: WrapTable = serde_json::from_value(serde_json::to_value(&next).unwrap()).unwrap();
    assert_eq!(reopened, next);
}

#[test]
fn bad_params_leave_table_unchanged() {
    let table = WrapTable::default().edited(SET, &json!({"id": 1, "mode": "tight"})).unwrap().unwrap();
    for params in [json!({"id": 1, "mode": "spiral"}), json!({"id": 1, "distance": -1.0}), json!({"id": 1, "distance": [1.0, 2.0]}), json!({"id": 1, "sides": 3}), json!({"mode": "square"})] {
        assert!(table.edited(SET, &params).is_err(), "{params}");
    }
    assert_eq!(table.edited(SET, &json!({"id": 1, "distance": 1e9})).unwrap_err().code, "OUT_OF_RANGE");
    assert_eq!(table.edited(QUERY, &json!({"id": 1})).unwrap_err().code, "NOT_IMPLEMENTED");
    assert_eq!(table.edited(SET, &json!({"id": 1, "mode": "tight"})).unwrap(), None);
}

#[test]
fn clear_and_ignore() {
    let table = WrapTable::default().edited(SET, &json!({"id": 3})).unwrap().unwrap();
    assert_eq!(table.query(ObjectId(3))["wrap"]["mode"], "square");
    let cleared = table.edited(CLEAR, &json!({"id": 3})).unwrap().unwrap();
    assert!(cleared.entries.is_empty());
    let ignoring = cleared.edited(IGNORE, &json!({"id": 9, "on": true})).unwrap().unwrap();
    assert_eq!(ignoring.query(ObjectId(9))["ignore"], true);
    assert!(ignoring.edited(IGNORE, &json!({"id": 9})).is_err());
}

#[test]
fn flow_spans_follow_mode_and_sides() {
    let bounds = [50.0, 10.0, 20.0, 20.0];
    let wrap = |mode, sides| TextWrap { mode, sides, distance: [0.0, 5.0, 0.0, 5.0] };
    let frame = [0.0, 100.0];
    let inside = [15.0, 25.0];
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::Square, WrapSides::Both), bounds)]), vec![[0.0, 45.0], [75.0, 100.0]]);
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::Square, WrapSides::Left), bounds)]), vec![[0.0, 45.0]]);
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::Square, WrapSides::Right), bounds)]), vec![[75.0, 100.0]]);
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::Square, WrapSides::Largest), bounds)]), vec![[0.0, 45.0]]);
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::Square, WrapSides::Smallest), bounds)]), vec![[75.0, 100.0]]);
    assert!(line_spans(frame, inside, &[(wrap(WrapMode::JumpObject, WrapSides::Both), bounds)]).is_empty());
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::Through, WrapSides::Both), bounds)]), vec![frame]);
    assert_eq!(line_spans(frame, inside, &[(wrap(WrapMode::None, WrapSides::Both), bounds)]), vec![frame]);
    assert_eq!(line_spans(frame, [40.0, 50.0], &[(wrap(WrapMode::Square, WrapSides::Both), bounds)]), vec![frame]);
}

#[test]
fn frames_that_ignore_wraps_get_no_wrappers() {
    let table = WrapTable::default()
        .edited(SET, &json!({"id": 1, "distance": 2.0})).unwrap().unwrap()
        .edited(IGNORE, &json!({"id": 2, "on": true})).unwrap().unwrap();
    let candidates = [(ObjectId(1), [0.0, 0.0, 10.0, 10.0]), (ObjectId(3), [0.0, 0.0, 5.0, 5.0])];
    assert_eq!(table.wrappers_for(ObjectId(5), &candidates).len(), 1);
    assert!(table.wrappers_for(ObjectId(2), &candidates).is_empty());
    assert!(table.wrappers_for(ObjectId(1), &candidates).is_empty());
}