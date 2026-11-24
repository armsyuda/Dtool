use dtool_core::workspace::ContextId;
use dtool_core::Session;
use dtool_ui::context_bar::{ContextBarModel, ContextControl, ContextEdit, ControlState, OPTION_SET};
use serde_json::json;

fn tool_model(tool: &str, context: u64) -> ContextBarModel {
    let mut s = Session::default();
    s.run_named("ui.tool.set", json!({ "id": tool })).expect("tool");
    ContextBarModel::stamped(&s, ContextId(context))
}

#[test]
fn context_bar_move_vs_zoom_ids() {
    let moving = tool_model("tool.move", 1);
    let zoom = tool_model("tool.zoom", 1);
    assert_ne!(moving.ids(), zoom.ids());
    assert_eq!(moving.item("move.align.left").expect("align").state, ControlState::Disabled);
    assert!(zoom.items.iter().filter(|item| !matches!(item.control, ContextControl::Action { .. })).all(|item| item.state == ControlState::Enabled));
}

#[test]
fn context_bar_unknown_kind_unsupported() {
    let query = json!({
        "tool": "tool.move",
        "options": [
            { "id": "a", "key": "a", "kind": "wheel", "value": 1 },
            { "id": "b", "key": "b", "kind": "toggle", "value": true, "state": "disabled" },
            { "id": "c", "key": "c", "kind": "toggle", "value": true, "state": "mixed" },
            { "id": "d", "key": "d", "kind": "toggle", "value": true, "state": "sparkly" },
            { "id": "e", "key": "e", "kind": "action" }
        ]
    });
    let model = ContextBarModel::from_stamped_query(&query, ContextId(3), 9);
    assert_eq!(model.ids(), vec!["a", "b", "c", "d", "e"]);
    let item = model.item("a").expect("a");
    assert_eq!(item.state, ControlState::Unsupported);
    assert_eq!(item.control, ContextControl::Unsupported { kind: "wheel".into() });
    assert_eq!(model.item("b").unwrap().state, ControlState::Disabled);
    assert_eq!(model.item("c").unwrap().state, ControlState::Mixed);
    assert_eq!(model.item("d").unwrap().state, ControlState::Unsupported);
    assert_eq!(model.item("e").unwrap().state, ControlState::Unsupported);
    let stamp = Some((ContextId(3), 9));
    assert!(model.bind(&ContextEdit { id: "a".into(), value: json!(2) }, stamp).is_none());
    assert!(model.bind(&ContextEdit { id: "b".into(), value: json!(false) }, stamp).is_none());
    assert!(model.bind(&ContextEdit { id: "c".into(), value: json!(false) }, stamp).is_some());
}

#[test]
fn context_bar_stale_stamp_no_command() {
    let model = tool_model("tool.shape.polygon", 7);
    let stamp = model.stamp;
    let edit = ContextEdit { id: "shape.sides".into(), value: json!(7) };
    let command = model.bind(&edit, stamp).expect("fresh command");
    assert_eq!(command.name, OPTION_SET);
    assert_eq!(command.params, json!({ "tool": "tool.shape.polygon", "id": "shape.sides", "value": 7 }));
    let (context, generation) = stamp.expect("stamp");
    assert!(model.bind(&edit, Some((context, generation + 1))).is_none());
    assert!(model.bind(&edit, Some((ContextId(context.0 + 1), generation))).is_none());
    assert!(model.bind(&edit, None).is_none());
    assert!(ContextBarModel { stamp: None, ..model.clone() }.bind(&edit, None).is_none());
}

#[test]
fn context_bar_disabled_action_no_command() {
    let query = json!({
        "tool": "tool.move",
        "options": [
            { "id": "go", "key": "go", "kind": "action", "command": "view.zoom.fit", "state": "disabled" },
            { "id": "run", "key": "run", "kind": "action", "command": "view.zoom.fit" }
        ]
    });
    let model = ContextBarModel::from_stamped_query(&query, ContextId(1), 1);
    let stamp = Some((ContextId(1), 1));
    assert!(model.bind(&ContextEdit { id: "go".into(), value: json!(null) }, stamp).is_none());
    let command = model.bind(&ContextEdit { id: "run".into(), value: json!(null) }, stamp).expect("action");
    assert_eq!(command.name, "view.zoom.fit");
}

#[test]
fn context_bar_section_slices_by_prefix() {
    let model = tool_model("tool.shape.polygon", 1);
    let shape: Vec<&str> = model.section("shape.").map(|item| item.id.as_str()).collect();
    assert!(shape.contains(&"shape.sides"));
    assert!(shape.iter().all(|id| id.starts_with("shape.")));
}