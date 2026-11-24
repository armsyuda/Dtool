use dtool_core::document::{Document, Units};
use dtool_core::{ObjectId, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1))
}

fn create(s: &mut Session, pos: [f32; 2], size: [f32; 2]) -> ObjectId {
    let id = s.document.next_object_id;
    s.run_named("object.create", json!({"name": "o", "pos": pos, "size": size})).unwrap();
    ObjectId(id)
}

fn pos(s: &Session, id: ObjectId) -> [f32; 2] {
    s.document.get_object(id).unwrap().pos
}

#[test]
fn distribute_fixed_spacing_three_objects_and_undo() {
    let mut s = session();
    let a = create(&mut s, [10.0, 10.0], [10.0, 10.0]);
    let b = create(&mut s, [50.0, 30.0], [20.0, 20.0]);
    let c = create(&mut s, [100.0, 60.0], [10.0, 30.0]);
    s.selection.ids = vec![c, a, b];
    s.run_named("layer.arrange.distribute.h", json!({"spacing": 5.0})).unwrap();
    assert_eq!([pos(&s, a)[0], pos(&s, b)[0], pos(&s, c)[0]], [10.0, 25.0, 50.0]);
    assert_eq!([pos(&s, a)[1], pos(&s, b)[1], pos(&s, c)[1]], [10.0, 30.0, 60.0]);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!([pos(&s, a)[0], pos(&s, b)[0], pos(&s, c)[0]], [10.0, 50.0, 100.0]);
}

#[test]
fn distribute_fixed_spacing_allows_two_objects_and_spread_start() {
    let mut s = session();
    let a = create(&mut s, [30.0, 10.0], [10.0, 10.0]);
    let b = create(&mut s, [90.0, 40.0], [10.0, 20.0]);
    s.selection.ids = vec![a, b];
    assert!(s.run_named("layer.arrange.distribute.v", json!({})).is_err());
    s.run_named("layer.arrange.distribute.v", json!({"spacing": 0.0})).unwrap();
    assert_eq!([pos(&s, a)[1], pos(&s, b)[1]], [10.0, 20.0]);
    s.run_named("layer.arrange.distribute.h", json!({"spacing": 2.0, "relative_to": "spread"})).unwrap();
    assert_eq!([pos(&s, a)[0], pos(&s, b)[0]], [0.0, 12.0]);
}

#[test]
fn distribute_fixed_spacing_rejects_bad_input() {
    let mut s = session();
    let a = create(&mut s, [30.0, 10.0], [10.0, 10.0]);
    let b = create(&mut s, [90.0, 40.0], [10.0, 20.0]);
    s.selection.ids = vec![a];
    assert!(s.run_named("layer.arrange.distribute.h", json!({"spacing": 3.0})).is_err());
    s.selection.ids = vec![a, b];
    assert!(s.run_named("layer.arrange.distribute.h", json!({"spacing": "wide"})).is_err());
    assert_eq!([pos(&s, a)[0], pos(&s, b)[0]], [30.0, 90.0]);
}

#[test]
fn transform_query_reports_mixed_width_for_two_objects() {
    let mut s = session();
    let a = create(&mut s, [0.0, 0.0], [100.0, 50.0]);
    let b = create(&mut s, [0.0, 10.0], [200.0, 50.0]);
    s.selection.ids = vec![a, b];
    let reply = s.run_named("transform.query", json!({})).unwrap();
    assert_eq!(reply["mixed"]["w"], json!(true));
    assert_eq!(reply["mixed"]["h"], json!(false));
    assert_eq!(reply["mixed"]["x"], json!(false));
    assert_eq!(reply["mixed"]["y"], json!(true));
    assert_eq!(reply["count"], json!(2));
}

#[test]
fn multi_object_set_about_anchor_keeps_mixed_fields_and_undoes_once() {
    let mut s = session();
    let a = create(&mut s, [0.0, 0.0], [100.0, 50.0]);
    let b = create(&mut s, [10.0, 10.0], [200.0, 80.0]);
    s.selection.ids = vec![a, b];
    s.run_named("transform.set", json!({"x": 40.0, "anchor": "TopLeft"})).unwrap();
    let query = s.run_named("transform.query", json!({"anchor": "TopLeft"})).unwrap();
    assert_eq!(query["mixed"]["x"], json!(false));
    assert_eq!(query["mixed"]["w"], json!(true));
    assert_eq!(s.document.get_object(a).unwrap().size, [100.0, 50.0]);
    assert_eq!(s.document.get_object(b).unwrap().size, [200.0, 80.0]);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pos(&s, a), [0.0, 0.0]);
    assert_eq!(pos(&s, b), [10.0, 10.0]);
}