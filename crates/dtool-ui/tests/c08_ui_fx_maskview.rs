//! B50-T002/T006 (Layer Effects dialog enable/preview/cancel for every kind) + B29-T004 (mask-only view toggle).
use dtool_core::workspace::Workspace;
use dtool_core::{ObjectId, Session};
use dtool_ui::dialogs::layer_effects::{LayerEffectsDialog, EFFECT_KINDS, OUTER_SHADOW};
use dtool_ui::mask_actions;
use serde_json::json;

fn primed() -> (Workspace, u64) {
    let mut workspace = Workspace::default();
    workspace.new_document("c08ui");
    workspace.session_mut().unwrap().run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [40.0, 30.0]})).unwrap();
    let id = workspace.session().unwrap().document.objects.last().unwrap().id.0;
    (workspace, id)
}

fn s(w: &mut Workspace) -> &mut Session {
    w.session_mut().unwrap()
}

fn undo_depth(w: &Workspace) -> usize {
    format!("{:?}", w.session().unwrap().undo).matches("Change {").count()
}

fn staged_kind(w: &Workspace, id: u64) -> Option<String> {
    dtool_core::commands::layer_style::staged(w.session().unwrap(), ObjectId(id)).map(|st| serde_json::to_value(&st.effect).unwrap()["kind"].as_str().unwrap_or_default().to_string())
}

fn effect_kinds(w: &Workspace, id: u64) -> Vec<String> {
    let o = w.session().unwrap().document.get_object(ObjectId(id)).unwrap();
    o.effects.iter().map(|e| serde_json::to_value(e).unwrap()["kind"].as_str().unwrap_or_default().to_string()).collect()
}

#[test]
fn every_effect_kind_has_a_toggle_and_previews_without_touching_the_document() {
    assert_eq!(EFFECT_KINDS.len(), 10);
    let (mut w, id) = primed();
    let (v, d) = (w.session().unwrap().document.version, undo_depth(&w));
    let mut dialog = LayerEffectsDialog::default();
    dialog.open_for(id);
    for (kind, _) in EFFECT_KINDS {
        dialog.set_enabled(s(&mut w), kind, true);
        assert_eq!(staged_kind(&w, id).as_deref(), Some(kind), "latest ticked kind is previewed");
    }
    dialog.set_enabled(s(&mut w), "GaussianBlur", false);
    assert_eq!(staged_kind(&w, id).as_deref(), Some("GradientOverlay"), "untick falls back to the previous ticked kind");
    for (kind, _) in EFFECT_KINDS {
        dialog.set_enabled(s(&mut w), kind, false);
    }
    assert_eq!(staged_kind(&w, id), None, "unticking everything cancels the preview");
    assert!(!dialog.outer_shadow && dialog.enabled.is_empty());
    assert_eq!(w.session().unwrap().document.version, v);
    assert_eq!(undo_depth(&w), d);
    dialog.set_enabled(s(&mut w), "NoSuchKind", true);
    assert!(dialog.enabled.is_empty(), "unknown kind ignored");
}

#[test]
fn ok_commits_all_ticked_kinds_with_undo_and_cancel_commits_nothing() {
    let (mut w, id) = primed();
    let d = undo_depth(&w);
    let mut dialog = LayerEffectsDialog::default();
    dialog.open_for(id);
    dialog.enable_outer_shadow(s(&mut w));
    dialog.set_enabled(s(&mut w), "InnerGlow", true);
    dialog.set_enabled(s(&mut w), "Outline", true);
    assert!(dialog.outer_shadow);
    assert!(dialog.ok(s(&mut w)));
    assert!(!dialog.open);
    assert_eq!(effect_kinds(&w, id), vec![OUTER_SHADOW.to_string(), "InnerGlow".into(), "Outline".into()]);
    assert_eq!(staged_kind(&w, id), None, "preview dropped on OK");
    let steps = undo_depth(&w) - d;
    assert_eq!(steps, 1, "confirming every enabled effect is one undo step");
    for _ in 0..steps {
        s(&mut w).run_named("edit.undo", json!({})).unwrap();
    }
    assert!(effect_kinds(&w, id).is_empty());

    let v = w.session().unwrap().document.version;
    dialog.open_for(id);
    dialog.set_enabled(s(&mut w), "BevelEmboss", true);
    dialog.cancel(s(&mut w));
    assert!(!dialog.open && effect_kinds(&w, id).is_empty() && staged_kind(&w, id).is_none());
    assert_eq!(w.session().unwrap().document.version, v);
}

#[test]
fn mask_only_view_toggle_is_per_view_and_never_changes_the_document() {
    let (mut w, parent) = primed();
    s(&mut w).run_named("object.create", json!({"name": "m", "pos": [0.0, 0.0], "size": [20.0, 30.0]})).unwrap();
    let mask = w.session().unwrap().document.objects.last().unwrap().id.0;
    s(&mut w).selection.ids = vec![ObjectId(parent)];
    assert!(!mask_actions::mask_view_available(w.session().unwrap()), "no vector mask yet");
    assert!(mask_actions::toggle_mask_view(s(&mut w)).is_err(), "refused without a mask");
    s(&mut w).run_named("layer.vector_mask.set", json!({"id": mask, "target": parent})).unwrap();
    s(&mut w).selection.ids = vec![ObjectId(parent)];
    let (v, d) = (w.session().unwrap().document.version, undo_depth(&w));
    assert!(mask_actions::mask_view_available(w.session().unwrap()));
    mask_actions::toggle_mask_view(s(&mut w)).unwrap();
    assert_eq!(w.session().unwrap().view.canvas_flags.mask_view, Some(ObjectId(parent)));
    assert!(mask_actions::mask_view_on(w.session().unwrap()));
    mask_actions::toggle_mask_view(s(&mut w)).unwrap();
    assert_eq!(w.session().unwrap().view.canvas_flags.mask_view, None);
    let panel = dtool_ui::panels::layers::mask_view_command(w.session().unwrap(), ObjectId(parent));
    assert_eq!(panel.name, "view.mask_view.set");
    assert_eq!(panel.params["id"], mask_actions::mask_view_target(w.session().unwrap(), ObjectId(parent)), "button and Layers Alt-click (8ec1aed) send the same command");
    mask_actions::alt_click_mask(s(&mut w), ObjectId(parent)).unwrap();
    assert_eq!(w.session().unwrap().view.canvas_flags.mask_view, Some(ObjectId(parent)));
    mask_actions::alt_click_mask(s(&mut w), ObjectId(parent)).unwrap();
    assert_eq!(w.session().unwrap().view.canvas_flags.mask_view, None, "Alt-click again leaves mask view");
    assert_eq!(w.session().unwrap().document.version, v, "view state only");
    assert_eq!(undo_depth(&w), d);
    let json = serde_json::to_string(&w.session().unwrap().view.canvas_flags).unwrap();
    assert!(!json.contains("mask_view"), "never serialized");
}

#[test]
fn mask_actions_row_renders_the_mask_only_button_headless() {
    let (mut w, parent) = primed();
    s(&mut w).selection.ids = vec![ObjectId(parent)];
    let ctx = egui::Context::default();
    let mut out = None;
    let mut full = ctx.run_ui(egui::RawInput::default(), |ui| {
        out = mask_actions::show_mask_actions(ui, s(&mut w));
    });
    full.textures_delta.clear();
    assert!(out.is_none(), "nothing clicked");
}