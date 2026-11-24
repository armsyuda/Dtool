use dtool_core::object::{Object, ObjectId};
use dtool_core::text_wrap::{TextWrap, WrapMode, WrapSides};
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use dtool_ui::dock::PanelId;
use dtool_ui::panels::text_wrap::{grouped, option_label, parse_state, TextWrapPanel, WrapState, WrapView, KEY_MODE, TEXT_WRAP_PANEL_KEY};
use dtool_ui::panels::{window_panel, workspace_panel, PanelRegistry, WINDOW_PANELS, WINDOW_TEXT_WRAP};
use serde_json::json;

fn workspace() -> (Workspace, ObjectId, ObjectId) {
    let mut workspace = Workspace::default();
    workspace.new_document("c11_text_wrap_panel");
    let session = workspace.session_mut().unwrap();
    let a = session.document.create_object(Object { name: "a".into(), pos: [40.0, 40.0], size: [30.0, 30.0], ..Object::default() });
    let b = session.document.create_object(Object { name: "b".into(), pos: [90.0, 40.0], size: [30.0, 30.0], ..Object::default() });
    session.selection.ids = vec![a];
    (workspace, a, b)
}

fn select(workspace: &mut Workspace, ids: &[ObjectId]) {
    workspace.session_mut().unwrap().selection.ids = ids.to_vec();
}

fn undo(workspace: &mut Workspace) {
    workspace.run(Scope::active(), Command::new("edit.undo")).1.expect("undo");
}

fn depth(workspace: &mut Workspace) -> usize {
    let mut stack = workspace.session_mut().unwrap().undo.clone();
    let mut count = 0;
    while stack.pop_last().is_some() {
        count += 1;
    }
    count
}

fn table_wrap(workspace: &mut Workspace, id: ObjectId) -> Option<TextWrap> {
    workspace.session_mut().unwrap().document.text_wraps.get(id).and_then(|entry| entry.wrap)
}

fn render(workspace: &mut Workspace, registry: &mut PanelRegistry, id: &PanelId) -> bool {
    let ctx = egui::Context::default();
    let mut shown = false;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        shown = workspace_panel(ui, id, registry, workspace);
    });
    output.textures_delta.clear();
    shown
}

#[test]
fn show_text_wrap_settings_menu_opens_the_panel_body() {
    let id = PanelId::Other(TEXT_WRAP_PANEL_KEY.to_string());
    assert_eq!(window_panel(WINDOW_TEXT_WRAP), Some(id.clone()));
    assert_eq!(WINDOW_TEXT_WRAP, "text.text_wrap.show_text_wrap_settings");
    assert!(WINDOW_PANELS.contains(&WINDOW_TEXT_WRAP));
    assert!(PanelRegistry::new().has_body(&id));
    let (mut workspace, a, b) = workspace();
    let mut registry = PanelRegistry::new();
    assert!(render(&mut workspace, &mut registry, &id));
    select(&mut workspace, &[a, b]);
    assert!(render(&mut workspace, &mut registry, &id));
    select(&mut workspace, &[]);
    assert!(render(&mut workspace, &mut registry, &id));
}

#[test]
fn panel_reads_selection_and_each_edit_is_one_undo_step_on_the_live_document() {
    let (mut workspace, id, _) = workspace();
    let mut panel = TextWrapPanel::default();
    let state = panel.state(&mut workspace).expect("state");
    assert_eq!((state.id, state.wrap, state.ignore), (id.0, None, false));
    let start = depth(&mut workspace);
    assert!(panel.set_mode(&mut workspace, WrapMode::Tight));
    assert!(panel.set_sides(&mut workspace, WrapSides::Left));
    assert!(panel.set_distance(&mut workspace, 1, 6.0));
    let wrap = table_wrap(&mut workspace, id).unwrap();
    assert_eq!((wrap.mode, wrap.sides, wrap.distance), (WrapMode::Tight, WrapSides::Left, [0.0, 6.0, 0.0, 0.0]));
    assert_eq!(depth(&mut workspace), start + 3);
    undo(&mut workspace);
    assert_eq!(table_wrap(&mut workspace, id).unwrap().distance, [0.0; 4]);
    undo(&mut workspace);
    assert_eq!(table_wrap(&mut workspace, id).unwrap().sides, WrapSides::Both);
    undo(&mut workspace);
    assert_eq!(table_wrap(&mut workspace, id), None);
}

#[test]
fn multi_select_change_updates_all_objects_and_one_undo_reverts_all() {
    let (mut workspace, a, b) = workspace();
    let mut panel = TextWrapPanel::default();
    select(&mut workspace, &[a, b]);
    let start = depth(&mut workspace);
    assert!(panel.set_mode(&mut workspace, WrapMode::Square));
    assert_eq!(depth(&mut workspace), start + 1);
    assert!(panel.set_distance(&mut workspace, 0, 5.0));
    assert!(panel.set_ignore(&mut workspace, true));
    assert_eq!(depth(&mut workspace), start + 3);
    for id in [a, b] {
        let wrap = table_wrap(&mut workspace, id).unwrap();
        assert_eq!((wrap.mode, wrap.distance[0]), (WrapMode::Square, 5.0));
    }
    let view = panel.view(&mut workspace).unwrap();
    assert_eq!((view.mode, view.distance[0], view.ignore, view.ids.len()), (Some(WrapMode::Square), Some(5.0), Some(true), 2));
    undo(&mut workspace);
    undo(&mut workspace);
    assert_eq!(table_wrap(&mut workspace, a).unwrap().distance[0], 0.0);
    assert_eq!(table_wrap(&mut workspace, b).unwrap().distance[0], 0.0);
    undo(&mut workspace);
    assert_eq!((table_wrap(&mut workspace, a), table_wrap(&mut workspace, b)), (None, None));
}

#[test]
fn mixed_values_show_as_mixed_and_a_change_unifies_them() {
    let (mut workspace, a, b) = workspace();
    let mut panel = TextWrapPanel::default();
    select(&mut workspace, &[a]);
    assert!(panel.set_mode(&mut workspace, WrapMode::Tight));
    assert!(panel.set_distance(&mut workspace, 2, 3.0));
    select(&mut workspace, &[a, b]);
    let view = panel.view(&mut workspace).unwrap();
    assert_eq!(view.mode, None);
    assert_eq!(view.sides, Some(WrapSides::Both));
    assert_eq!(view.distance, [Some(0.0), Some(0.0), None, Some(0.0)]);
    assert_eq!(view.ignore, Some(false));
    assert!(view.any_wrap);
    let mut registry = PanelRegistry::new();
    assert!(render(&mut workspace, &mut registry, &PanelId::Other(TEXT_WRAP_PANEL_KEY.to_string())));
    assert!(panel.set_mode(&mut workspace, WrapMode::Through));
    assert_eq!(panel.view(&mut workspace).unwrap().mode, Some(WrapMode::Through));
    assert_eq!(table_wrap(&mut workspace, a).unwrap().distance[2], 3.0);
}

#[test]
fn errors_are_reported_and_leave_every_object_unchanged() {
    let (mut workspace, a, b) = workspace();
    let mut panel = TextWrapPanel::default();
    select(&mut workspace, &[a, b]);
    let start = depth(&mut workspace);
    assert!(!panel.set_distance(&mut workspace, 0, -5.0));
    assert!(panel.last_error.is_some());
    assert_eq!((table_wrap(&mut workspace, a), table_wrap(&mut workspace, b)), (None, None));
    assert_eq!(depth(&mut workspace), start);
    assert!(!panel.set_distance(&mut workspace, 9, 1.0));
    select(&mut workspace, &[]);
    assert!(panel.view(&mut workspace).is_none());
    assert!(!panel.set_mode(&mut workspace, WrapMode::Square));
}

#[test]
fn grouping_view_and_labels() {
    assert!(grouped("object.text_wrap.set", &[], &json!({})).is_none());
    assert_eq!(grouped("object.text_wrap.set", &[4], &json!({ "mode": "tight" })).unwrap().name, "object.text_wrap.set");
    let batch = grouped("object.text_wrap.set", &[4, 5], &json!({ "mode": "tight" })).unwrap();
    assert_eq!(batch.name, "edit.batch");
    assert_eq!(batch.params["commands"][1]["params"], json!({ "mode": "tight", "id": 5 }));
    let state = parse_state(&json!({ "id": 3, "wrap": { "mode": "jump_object", "sides": "largest", "distance": [1.0, 2.0, 3.0, 4.0] }, "ignore": true })).unwrap();
    assert_eq!(state.effective().mode, WrapMode::JumpObject);
    assert_eq!(parse_state(&json!({ "id": 3, "wrap": null })).unwrap().wrap, None);
    let plain = WrapState { id: 4, wrap: None, ignore: false };
    let view = WrapView::from_states(&[state, plain]).unwrap();
    assert_eq!((view.mode, view.ignore, view.distance[0]), (None, None, None));
    assert!(WrapView::from_states(&[]).is_none());
    assert!(!option_label(KEY_MODE, &WrapMode::Through).is_empty());
}