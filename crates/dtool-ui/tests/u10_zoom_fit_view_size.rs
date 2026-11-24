use dtool_core::workspace::ContextId;
use dtool_core::Session;
use dtool_ui::context_bar::{action_params, apply_edit, apply_edit_with_view, pick_bound_with_view, pick_with_view, ContextBarModel, ContextEdit, ControlState, VIEW_ZOOM_FIT};
use serde_json::json;

const VIEW: [f32; 2] = [800.0, 600.0];

fn session_with(tool: &str) -> Session {
    let mut s = Session::new(dtool_core::document::Document::new([400.0, 300.0], dtool_core::document::Units::Px, 72.0, 1));
    s.has_document = true;
    s.run_named("ui.tool.set", json!({ "id": tool })).expect("tool");
    s
}

fn fit_zoom(s: &Session) -> f32 {
    let page = s.document.pages.first().expect("page").size;
    dtool_core::view::fit_zoom(page, VIEW, s.view.rotation_deg)
}

#[test]
fn u10_zoom_fit_disabled_without_view_size_enabled_with_it() {
    let s = session_with("tool.zoom");
    let plain = ContextBarModel::current(&s);
    assert_eq!(plain.item("zoom.fit").expect("fit").state, ControlState::Disabled);
    let sized = ContextBarModel::current(&s).with_view_size(&s, VIEW);
    assert_eq!(sized.item("zoom.fit").expect("fit").state, ControlState::Enabled);
    assert_eq!(sized.item("zoom.in").unwrap().state, plain.item("zoom.in").unwrap().state);
    let zero = ContextBarModel::current(&s).with_view_size(&s, [0.0, 600.0]);
    assert_eq!(zero.item("zoom.fit").unwrap().state, ControlState::Disabled);
    assert_eq!(action_params(VIEW_ZOOM_FIT, VIEW), json!({ "view_size": [800.0, 600.0] }));
    assert_eq!(action_params("view.zoom.in", VIEW), json!({}));
}

#[test]
fn u10_zoom_fit_apply_with_view_fits() {
    for (tool, id) in [("tool.zoom", "zoom.fit"), ("tool.view", "view.zoom_fit")] {
        let mut s = session_with(tool);
        s.run_named("view.zoom.set", json!({ "zoom": 7.0 })).expect("zoom set");
        let edit = ContextEdit { id: id.into(), value: json!(null) };
        assert!(apply_edit(&mut s, tool, &edit).is_err(), "{tool}");
        apply_edit_with_view(&mut s, tool, &edit, VIEW).expect(id);
        assert!((s.view.zoom - fit_zoom(&s)).abs() < 1e-4, "{tool}: {} vs {}", s.view.zoom, fit_zoom(&s));
    }
}

#[test]
fn u10_zoom_fit_non_action_matches_apply_edit() {
    let mut a = session_with("tool.zoom");
    let mut b = session_with("tool.zoom");
    let edit = ContextEdit { id: "zoom.level".into(), value: json!(200.0) };
    let ra = apply_edit(&mut a, "tool.zoom", &edit).expect("plain");
    let rb = apply_edit_with_view(&mut b, "tool.zoom", &edit, VIEW).expect("view");
    assert_eq!(ra, rb);
    assert_eq!(a.tool_options.query("tool.zoom"), b.tool_options.query("tool.zoom"));
}

#[test]
fn u10_zoom_fit_bound_command_carries_view_size() {
    let s = session_with("tool.zoom");
    let model = ContextBarModel::stamped(&s, ContextId(2)).with_view_size(&s, VIEW);
    let edit = ContextEdit { id: "zoom.fit".into(), value: json!(null) };
    let command = model.bind_with_view(&edit, Some((ContextId(2), s.generation)), VIEW).expect("command");
    assert_eq!(command.name, VIEW_ZOOM_FIT);
    assert_eq!(command.params, json!({ "view_size": [800.0, 600.0] }));
    assert!(s.can_execute(&command));
    let zoom_in = model.bind_with_view(&ContextEdit { id: "zoom.in".into(), value: json!(null) }, Some((ContextId(2), s.generation)), VIEW).expect("in");
    assert_eq!(zoom_in.name, "view.zoom.in");
    assert!(model.bind_with_view(&edit, Some((ContextId(3), s.generation)), VIEW).is_none());
}

#[test]
fn u10_zoom_fit_pick_without_input_returns_nothing() {
    let s = session_with("tool.zoom");
    let ctx = egui::Context::default();
    let mut plain = Some(None);
    let mut bound = Some(None);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        plain = Some(pick_with_view(ui, &s, VIEW));
        bound = Some(pick_bound_with_view(ui, &s, ContextId(1), VIEW));
    });
    output.textures_delta.clear();
    assert!(matches!(plain, Some(None)));
    assert!(matches!(bound, Some(None)));
}
