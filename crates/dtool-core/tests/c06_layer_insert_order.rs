use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::{Command, Session};
use serde_json::json;

fn order(s: &Session) -> Vec<u64> {
    s.document.objects.iter().map(|o| o.id.0).collect()
}

fn session() -> (Session, Vec<ObjectId>) {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let a = s.document.create_object(Object { name: "a".into(), ..Object::default() });
    let group = s.document.create_object(Object { name: "g".into(), ..Object::default() });
    let child = s.document.create_object(Object { name: "c".into(), parent_id: Some(group), ..Object::default() });
    let b = s.document.create_object(Object { name: "b".into(), ..Object::default() });
    (s, vec![a, group, child, b])
}

#[test]
fn fill_layer_goes_directly_above_selection() {
    let (mut s, ids) = session();
    s.selection.ids = vec![ids[0]];
    let reply = s.run(&Command::with_params("layer.fill.add", json!({}))).unwrap();
    let id = reply["id"].as_u64().unwrap();
    assert_eq!(order(&s), vec![ids[0].0, id, ids[1].0, ids[2].0, ids[3].0]);
    assert_eq!(s.selection.ids, vec![ObjectId(id)]);
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap();
    assert_eq!(order(&s), vec![ids[0].0, ids[1].0, ids[2].0, ids[3].0]);
}

#[test]
fn layer_goes_on_top_without_selection_and_above_a_whole_group() {
    let (mut s, ids) = session();
    s.selection.ids.clear();
    let top = s.run(&Command::with_params("layer.fill.add", json!({}))).unwrap()["id"].as_u64().unwrap();
    assert_eq!(*order(&s).last().unwrap(), top);
    s.selection.ids = vec![ids[2]];
    let above = s.run(&Command::with_params("layer.fill.add", json!({}))).unwrap()["id"].as_u64().unwrap();
    assert_eq!(order(&s), vec![ids[0].0, ids[1].0, ids[2].0, above, ids[3].0, top]);
    assert!(s.document.get_object(ObjectId(above)).unwrap().parent_id.is_none());
}

#[test]
fn topmost_of_several_selected_is_the_anchor() {
    let (mut s, ids) = session();
    s.selection.ids = vec![ids[3], ids[0]];
    let id = s.run(&Command::with_params("layer.fill.add", json!({}))).unwrap()["id"].as_u64().unwrap();
    assert_eq!(*order(&s).last().unwrap(), id);
}
