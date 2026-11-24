use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use dtool_ui::panels::quick_fx::QuickFxPanel;
use serde_json::json;
fn setup() -> (Workspace, Vec<dtool_core::ObjectId>) {
    let mut ws = Workspace::default(); ws.new_document("fx");
    let ids = (0..2).map(|_| ws.session_mut().unwrap().document.create_object(dtool_core::Object { size: [20.0, 20.0], ..Default::default() })).collect::<Vec<_>>();
    ws.session_mut().unwrap().selection.ids = ids.clone(); (ws, ids)
}
#[test]
fn quick_effects_change_every_selected_object_with_one_undo_and_preserve_other_settings() {
    let (mut ws, ids) = setup(); let mut panel = QuickFxPanel::default();
    panel.set_enabled(&mut ws, "OuterShadow", true).unwrap();
    assert!(ids.iter().all(|id| ws.session().unwrap().document.get_object(*id).unwrap().effects.len() == 1));
    ws.run(Scope::active(), Command::with_params("edit.undo", json!({}))).1.unwrap();
    assert!(ids.iter().all(|id| ws.session().unwrap().document.get_object(*id).unwrap().effects.is_empty()));
    ws.run(Scope::active(), Command::with_params("edit.redo", json!({}))).1.unwrap();
    panel.set_numeric(&mut ws, "OuterShadow", "opacity", 0.4).unwrap();
    panel.set_numeric(&mut ws, "OuterShadow", "radius", 12.0).unwrap();
    for id in &ids { let fx = &ws.session().unwrap().document.get_object(*id).unwrap().effects[0]; assert_eq!(fx.opacity, 0.4); assert_eq!(serde_json::to_value(&fx.params).unwrap()["Shadow"]["radius"], 12.0); }
    ws.run(Scope::active(), Command::with_params("edit.undo", json!({}))).1.unwrap();
    assert!(ids.iter().all(|id| ws.session().unwrap().document.get_object(*id).unwrap().effects[0].opacity == 0.4));
}
#[test]
fn locked_target_rolls_back_all_selected_effect_changes() {
    let (mut ws, ids) = setup(); ws.session_mut().unwrap().document.objects.iter_mut().find(|o| o.id == ids[1]).unwrap().locked = true;
    let version = ws.session().unwrap().document.version;
    assert!(QuickFxPanel::default().set_enabled(&mut ws, "OuterGlow", true).is_err());
    assert_eq!(ws.session().unwrap().document.version, version);
    assert!(ids.iter().all(|id| ws.session().unwrap().document.get_object(*id).unwrap().effects.is_empty()));
}
#[test]
fn quick_fx_window_has_a_real_body_and_layer_fx_request_is_document_owned() {
    let (mut ws, ids) = setup();
    let mut registry = dtool_ui::panels::PanelRegistry::new();
    assert_eq!(dtool_ui::panels::window_panel("window.vector.quick_fx"), Some(dtool_ui::dock::PanelId::Effects));
    assert!(registry.has_body(&dtool_ui::dock::PanelId::Effects));
    assert!(registry.layers.request_effects(&mut ws, ids[0].0));
    assert_eq!(registry.layers.effects_target, Some((ws.active().unwrap(), ids[0].0)));
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| { assert!(dtool_ui::panels::workspace_panel(ui, &dtool_ui::dock::PanelId::Effects, &mut registry, &mut ws)); });
    output.textures_delta.clear();
}
