use dtool_core::workspace::Workspace;
use dtool_ui::dock::{DockSide, Layout, PanelId};
use dtool_ui::panels::{show_side_with_slices, workspace_panel, PanelRegistry, SessionPanels};

fn workspace() -> Workspace {
    let mut workspace = Workspace::default();
    workspace.new_document("u06");
    workspace
}

#[test]
fn u06_live_hide_then_show_right_restores_same_tabs() {
    let mut layout = Layout::default();
    layout.dock_panel(&PanelId::Stroke, DockSide::Right);
    let before = layout.panels_on(DockSide::Right);
    assert!(!before.is_empty());
    assert!(before.iter().all(|id| layout.is_panel_shown(id)));
    layout.hide_panels();
    assert!(layout.all_hidden());
    assert!(before.iter().all(|id| !layout.is_panel_shown(id)));
    assert!(before.iter().all(|id| layout.panel_state(id).is_some_and(|state| state.visible)));
    layout.show_right();
    let after = layout.panels_on(DockSide::Right);
    assert_eq!(after.len(), before.len());
    assert!(before.iter().all(|id| after.contains(id)));
    assert!(before.iter().all(|id| layout.is_panel_shown(id)));
}

#[test]
fn u06_live_hide_panels_keeps_floating_state() {
    let mut layout = Layout::default();
    layout.float_panel(&PanelId::Swatches, false);
    layout.hide_panels();
    let state = layout.panel_state(&PanelId::Swatches).unwrap();
    assert!(state.visible && state.floated);
}

#[test]
fn u06_live_stroke_body_is_live_in_dock_path() {
    let mut registry = PanelRegistry::with_existing_bodies();
    assert!(registry.has_body(&PanelId::Stroke));
    let mut workspace = workspace();
    let mut layout = Layout::default();
    layout.dock_panel(&PanelId::Stroke, DockSide::Right);
    let mut slices = dtool_ui::slices_panel::SlicesPanel::default();
    let mut history = dtool_ui::history_panel::HistoryPanel::default();
    let mut snapshots = dtool_ui::snapshots_panel::SnapshotsPanel::default();
    let ctx = egui::Context::default();
    let mut shown = false;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        shown = workspace_panel(ui, &PanelId::Stroke, &mut registry, &mut workspace);
        let panels = SessionPanels { slices: &mut slices, history: &mut history, snapshots: &mut snapshots };
        show_side_with_slices(ui, &mut layout, DockSide::Right, &mut registry, panels, &mut workspace);
    });
    output.textures_delta.clear();
    assert!(shown);
    assert_eq!(layout.side_of(&PanelId::Stroke), Some(DockSide::Right));
}

#[test]
fn u06_live_os_panel_close_redocks_and_focuses_root() {
    let mut layout = Layout::default();
    layout.float_panel(&PanelId::Swatches, true);
    assert!(layout.panel_state(&PanelId::Swatches).is_some_and(|state| state.floated && state.os_window));
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    let mut bodies = 0;
    let mut output = ctx.run_ui(input, |ui| {
        layout.show_floating(ui.ctx(), |_ui, _id| bodies += 1);
    });
    output.textures_delta.clear();
    assert_eq!(bodies, 1);
    let state = layout.panel_state(&PanelId::Swatches).unwrap();
    assert!(!state.floated && !state.os_window);
    let root = output.viewport_output.get(&egui::ViewportId::ROOT).expect("root viewport output");
    assert!(root.commands.iter().any(|command| matches!(command, egui::ViewportCommand::Focus)));
}

#[test]
fn u06_live_open_os_panel_does_not_steal_root_focus() {
    let mut layout = Layout::default();
    layout.float_panel(&PanelId::Swatches, true);
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        layout.show_floating(ui.ctx(), |_ui, _id| {});
    });
    output.textures_delta.clear();
    assert!(layout.panel_state(&PanelId::Swatches).is_some_and(|state| state.floated && state.os_window));
    let focus = output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|root| root.commands.iter().any(|command| matches!(command, egui::ViewportCommand::Focus)));
    assert!(!focus);
}