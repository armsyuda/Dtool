use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_protocol::Command;
use serde_json::json;

fn session(names: &[&str]) -> (Session, Vec<ObjectId>) {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    let ids = names
        .iter()
        .map(|name| session.document.create_object(Object { name: (*name).into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() }))
        .collect();
    (session, ids)
}

fn order(session: &Session) -> Vec<String> {
    session.document.objects.iter().map(|obj| obj.name.clone()).collect()
}

fn arrange(session: &mut Session, id: ObjectId, step: &str) {
    session.run(&Command::with_params("node.arrange", json!({ "nodes": [id.0], "step": step }))).unwrap();
}

#[test]
fn to_front_inside_a_group_stays_below_objects_above_the_group() {
    let (mut s, ids) = session(&["A", "B", "C", "D"]);
    s.run(&Command::with_params("node.group", json!({ "nodes": [ids[0].0, ids[1].0], "name": "G" }))).unwrap();
    s.run(&Command::with_params("node.reparent", json!({ "nodes": [ids[3].0], "parent": null }))).unwrap();
    let before = order(&s);
    let pos = |s: &Session, name: &str| order(s).iter().position(|n| n == name).unwrap();
    let c_before = pos(&s, "C");
    let d_before = pos(&s, "D");
    arrange(&mut s, ids[0], "to_front");
    assert!(pos(&s, "A") > pos(&s, "B"), "{:?}", order(&s));
    assert_eq!(pos(&s, "C"), c_before);
    assert_eq!(pos(&s, "D"), d_before);
    assert!(pos(&s, "A") < d_before.max(c_before), "{:?}", order(&s));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(order(&s), before);
}

#[test]
fn forward_and_back_step_over_siblings_only() {
    let (mut s, ids) = session(&["A", "B", "C", "D"]);
    let group = s.run(&Command::with_params("node.group", json!({ "nodes": [ids[1].0, ids[2].0], "name": "G" }))).unwrap()["group"].as_u64().unwrap();
    let top: Vec<String> = |s: &Session| -> Vec<String> { s.document.objects.iter().filter(|o| o.parent_id.is_none()).map(|o| o.name.clone()).collect() }(&s);
    let a_index = top.iter().position(|n| n == "A").unwrap();
    arrange(&mut s, ids[0], "forward");
    let top_after: Vec<String> = s.document.objects.iter().filter(|o| o.parent_id.is_none()).map(|o| o.name.clone()).collect();
    assert_eq!(top_after.iter().position(|n| n == "A").unwrap(), a_index + 1, "{top_after:?}");
    let children: Vec<String> = s.document.objects.iter().filter(|o| o.parent_id == Some(ObjectId(group))).map(|o| o.name.clone()).collect();
    assert_eq!(children, vec!["B".to_string(), "C".to_string()]);
    arrange(&mut s, ids[2], "to_back");
    let children: Vec<String> = s.document.objects.iter().filter(|o| o.parent_id == Some(ObjectId(group))).map(|o| o.name.clone()).collect();
    assert_eq!(children, vec!["C".to_string(), "B".to_string()]);
    let top_last: Vec<String> = s.document.objects.iter().filter(|o| o.parent_id.is_none()).map(|o| o.name.clone()).collect();
    assert_eq!(top_last, top_after);
}

#[test]
fn top_level_to_front_still_moves_above_everything() {
    let (mut s, ids) = session(&["A", "B", "C"]);
    arrange(&mut s, ids[0], "to_front");
    assert_eq!(order(&s), vec!["B", "C", "A"]);
    arrange(&mut s, ids[0], "to_back");
    assert_eq!(order(&s), vec!["A", "B", "C"]);
}