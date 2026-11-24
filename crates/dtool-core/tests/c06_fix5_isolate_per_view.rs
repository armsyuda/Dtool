use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::scene::{estimate_scene, is_drawn, is_drawn_in_view, is_drawn_with_flags};
use dtool_core::CanvasFlags;
use dtool_protocol::Command;
use serde_json::json;

fn session_with_group() -> (Session, ObjectId, ObjectId, ObjectId) {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    session.has_document = true;
    let a = session.document.create_object(Object { name: "A".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let b = session.document.create_object(Object { name: "B".into(), pos: [20.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let c = session.document.create_object(Object { name: "C".into(), pos: [40.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    let group = ObjectId(session.run(&Command::with_params("node.group", json!({ "nodes": [a.0, b.0], "name": "G" }))).unwrap()["group"].as_u64().unwrap());
    (session, group, a, c)
}

fn listed(session: &mut Session) -> Vec<u64> {
    let mut items: Vec<u64> = session.run(&Command::new("canvas.display_list")).unwrap()["items"].as_array().unwrap().iter().map(|item| item["object_id"].as_u64().unwrap()).collect();
    items.sort_unstable();
    items
}

#[test]
fn c06_fix5_isolate_in_one_session_leaves_the_other_unchanged() {
    let (mut first, group, a, c) = session_with_group();
    let (mut second, second_group, _, second_c) = session_with_group();
    assert_eq!(group, second_group);
    let before = listed(&mut second);
    first.run(&Command::with_params("node.isolate", json!({ "nodes": [group.0] }))).unwrap();
    assert_eq!(first.view.isolate_target(), Some(group));
    assert_eq!(second.view.isolate_target(), None);
    assert!(!listed(&mut first).contains(&c.0));
    assert!(listed(&mut first).contains(&a.0));
    assert_eq!(listed(&mut second), before);
    assert!(is_drawn_in_view(&second.document, &second.view, second_c));
    assert!(!is_drawn_in_view(&first.document, &first.view, c));
    let second_scene = estimate_scene(&second.document, &second.view.canvas_flags);
    assert_eq!(second_scene.counts(), estimate_scene(&second.document, &CanvasFlags::default()).counts());
}

#[test]
fn c06_fix5_target_missing_from_doc_draws_everything() {
    let (session, _, a, c) = session_with_group();
    let flags = CanvasFlags { isolate_target: Some(ObjectId(9_999)), ..CanvasFlags::default() };
    assert!(is_drawn_with_flags(&session.document, &flags, a));
    assert!(is_drawn_with_flags(&session.document, &flags, c));
}

#[test]
fn c06_fix5_is_drawn_ignores_isolate_for_export() {
    let (mut session, group, _, c) = session_with_group();
    session.run(&Command::with_params("node.isolate", json!({ "nodes": [group.0] }))).unwrap();
    assert!(is_drawn(&session.document, c));
    assert!(!is_drawn_with_flags(&session.document, &session.view.canvas_flags, c));
}

#[test]
fn c06_fix5_isolate_round_trips_in_view_json_and_old_json_defaults() {
    let (mut session, group, _, _) = session_with_group();
    session.run(&Command::with_params("node.isolate", json!({ "nodes": [group.0] }))).unwrap();
    let text = serde_json::to_string(&session.view).unwrap();
    let back: dtool_core::ViewState = serde_json::from_str(&text).unwrap();
    assert_eq!(back.isolate_target(), Some(group));
    let old: CanvasFlags = serde_json::from_value(json!({ "force_pixel_alignment": false, "clip_to_canvas": false, "preview_mode": false, "view_mode": "Normal" })).unwrap();
    assert_eq!(old.isolate_target, None);
}