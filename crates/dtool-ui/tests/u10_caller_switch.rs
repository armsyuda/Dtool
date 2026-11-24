use dtool_core::workspace::ContextId;
use dtool_core::Session;
use dtool_ui::context_bar::{bound_command, is_pen_close, pick_bound, ContextEdit, OPTION_SET, PEN_CLOSE_CURVE};
use dtool_ui::persona_bar::{selection_command, LEAVE_APPLY, LEAVE_CANCEL};
use serde_json::json;

fn session_with(tool: &str) -> Session {
    let mut s = Session::default();
    s.run_named("ui.tool.set", json!({ "id": tool })).expect("tool");
    s
}

#[test]
fn u10_bound_command_routes_option_set_through_binder() {
    let s = session_with("tool.zoom");
    let edit = ContextEdit { id: "zoom.level".into(), value: json!(200.0) };
    let command = bound_command(&s, ContextId(1), &edit).expect("command");
    assert_eq!(command.name, OPTION_SET);
    assert_eq!(command.params["id"], json!("zoom.level"));
    assert_eq!(command.params["value"], json!(200.0));
    assert_eq!(command.params["tool"], json!("tool.zoom"));
}

#[test]
fn u10_bound_command_action_and_disabled() {
    let s = session_with("tool.zoom");
    let action = bound_command(&s, ContextId(1), &ContextEdit { id: "zoom.in".into(), value: json!(null) }).expect("action");
    assert_eq!(action.name, "view.zoom.in");
    let moving = session_with("tool.move");
    assert!(bound_command(&moving, ContextId(1), &ContextEdit { id: "move.align.left".into(), value: json!(null) }).is_none());
    assert!(bound_command(&moving, ContextId(1), &ContextEdit { id: "no.such".into(), value: json!(1) }).is_none());
}

#[test]
fn u10_pen_close_edit_detected() {
    assert!(is_pen_close(&ContextEdit { id: PEN_CLOSE_CURVE.into(), value: json!(null) }));
    assert!(!is_pen_close(&ContextEdit { id: "zoom.in".into(), value: json!(null) }));
}

#[test]
fn u10_pick_bound_without_input_returns_nothing() {
    let s = session_with("tool.zoom");
    let ctx = egui::Context::default();
    let mut picked = Some(None);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        picked = Some(pick_bound(ui, &s, ContextId(1)));
    });
    output.textures_delta.clear();
    assert!(matches!(picked, Some(None)));
}

#[test]
fn u10_persona_selection_commands() {
    assert_eq!(selection_command(LEAVE_APPLY).name, "ui.persona.leave_sub");
    assert_eq!(selection_command(LEAVE_APPLY).params["apply"], json!(true));
    assert_eq!(selection_command(LEAVE_CANCEL).params["apply"], json!(false));
    let sub = selection_command("enter_sub:ai_image");
    assert_eq!((sub.name.as_str(), sub.params["id"].clone()), ("ui.persona.enter_sub", json!("ai_image")));
    let set = selection_command("pixel");
    assert_eq!((set.name.as_str(), set.params["id"].clone()), ("ui.persona.set", json!("pixel")));
}
