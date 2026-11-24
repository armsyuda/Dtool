use dtool_core::commands::layer_style::staged_effects;
use dtool_core::layer_style::{default_effect, BlendRanges, EffectKind, EffectParams};
use dtool_core::{Command, Object, ObjectId, Session};
use dtool_ui::dialogs::{layer_effects::LayerEffectsDialog, blend_options::BlendOptionsDialog};
use serde_json::json;
fn setup() -> (Session, ObjectId) { let mut s = Session::new(dtool_core::Document::default()); let id = s.document.create_object(Object { size: [80.0, 60.0], ..Default::default() }); (s, id) }
fn effects(s: &Session, id: ObjectId) -> Vec<dtool_core::layer_style::LayerEffect> { s.document.get_object(id).unwrap().effects.clone() }
#[test]
fn reopening_effects_edits_existing_values_and_commits_one_undo_without_duplicates() {
    let (mut s, id) = setup(); s.run(&Command::with_params("effect.add", json!({"id": id.0, "kind": "OuterShadow"}))).unwrap();
    let before = effects(&s, id); let depth = s.undo.applied_steps(); let generation = s.generation;
    let mut dialog = LayerEffectsDialog::default(); dialog.open_for(id.0);
    let mut edit = before[0].clone(); edit.opacity = 0.25;
    if let EffectParams::Shadow { radius, angle, .. } = &mut edit.params { *radius = 14.0; *angle = 135.0; }
    assert!(dialog.set_effect(&mut s, edit.clone())); dialog.set_enabled(&mut s, "Outline", true);
    assert_eq!(effects(&s, id), before); assert_eq!(s.generation, generation); assert_eq!(staged_effects(&s, id).len(), 2);
    assert!(dialog.ok(&mut s)); assert_eq!(effects(&s, id).len(), 2); assert_eq!(effects(&s, id)[0], edit); assert_eq!(s.undo.applied_steps(), depth + 1);
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap(); assert_eq!(effects(&s, id), before);
    s.run(&Command::with_params("edit.redo", json!({}))).unwrap(); assert_eq!(effects(&s, id)[0], edit);
}
#[test]
fn cancel_and_invalid_parameters_never_change_document_or_history() {
    let (mut s, id) = setup(); let before = (s.generation, s.document.version, s.undo.applied_steps());
    let mut dialog = LayerEffectsDialog::default(); dialog.open_for(id.0);
    let mut effect = default_effect(EffectKind::ColourOverlay); effect.opacity = 0.4; assert!(dialog.set_effect(&mut s, effect));
    let mut invalid = default_effect(EffectKind::OuterShadow); invalid.opacity = 2.0; assert!(!dialog.set_effect(&mut s, invalid));
    dialog.cancel(&mut s); assert!(effects(&s, id).is_empty()); assert!(staged_effects(&s, id).is_empty());
    assert_eq!((s.generation, s.document.version, s.undo.applied_steps()), before);
}
#[test]
fn stale_effect_confirmation_preserves_the_external_change() {
    let (mut s, id) = setup(); let mut dialog = LayerEffectsDialog::default(); dialog.open_for(id.0); dialog.enable_outer_shadow(&mut s);
    s.run(&Command::with_params("object.update", json!({"id": id.0, "name": "external"}))).unwrap();
    assert!(!dialog.ok(&mut s)); assert!(effects(&s, id).is_empty()); assert_eq!(s.document.get_object(id).unwrap().name, "external"); assert!(staged_effects(&s, id).is_empty());
}
#[test]
fn all_effect_property_editors_render_headlessly_without_committing() {
    let (mut s, id) = setup(); let mut dialog = LayerEffectsDialog::default(); dialog.open_for(id.0);
    for (kind, _) in dtool_ui::dialogs::layer_effects::EFFECT_KINDS { dialog.set_enabled(&mut s, kind, true); }
    let before = (s.generation, s.document.version, s.undo.applied_steps());
    let ctx = egui::Context::default(); let mut out = ctx.run_ui(egui::RawInput::default(), |ui| dialog.show(ui.ctx(), &mut s)); out.textures_delta.clear();
    assert_eq!((s.generation, s.document.version, s.undo.applied_steps()), before); assert!(effects(&s, id).is_empty()); assert_eq!(staged_effects(&s, id).len(), 10);
    dialog.cancel(&mut s);
}
#[test]
fn blend_mode_and_ranges_are_one_undo_group_and_cancel_is_unchanged() {
    let (mut s, id) = setup(); let mut dialog = BlendOptionsDialog::default(); dialog.open_for(id.0, "Normal"); dialog.set_mode("Screen");
    let ranges = BlendRanges { source: vec![[0.0, 0.0], [0.3, 0.1], [1.0, 1.0]], underlying: vec![[0.0, 0.0], [1.0, 1.0]], gamma: 1.2, antialias: true };
    assert!(dialog.set_ranges(&s, Some(ranges.clone()))); assert!(dialog.apply(&mut s));
    assert_eq!(s.document.get_object(id).unwrap().blend_mode, dtool_core::blend::BlendMode::Screen); assert_eq!(s.document.get_object(id).unwrap().blend_ranges, Some(ranges));
    s.run(&Command::with_params("edit.undo", json!({}))).unwrap(); assert_eq!(s.document.get_object(id).unwrap().blend_mode, dtool_core::blend::BlendMode::Normal); assert!(s.document.get_object(id).unwrap().blend_ranges.is_none());
    let before = s.generation; dialog.open_for(id.0, "Normal"); dialog.set_mode("Multiply"); dialog.cancel(); assert_eq!(s.generation, before);
}
#[test]
fn every_supported_blend_mode_is_exposed_and_applies_through_the_real_command() {
    let (mut s, id) = setup();
    let names: Vec<String> = dtool_core::blend::BlendMode::ALL.iter().map(|mode| format!("{mode:?}")).collect(); assert_eq!(names, dtool_ui::dialogs::blend_options::MODES);
    for mode in dtool_core::blend::BlendMode::ALL { if matches!(mode, dtool_core::blend::BlendMode::Normal | dtool_core::blend::BlendMode::PassThrough) { continue; }
        let mut dialog = BlendOptionsDialog::default(); dialog.open_for(id.0, "Normal"); dialog.set_mode(&format!("{mode:?}")); assert!(dialog.apply(&mut s)); assert_eq!(s.document.get_object(id).unwrap().blend_mode, mode);
        s.run(&Command::with_params("edit.undo", json!({}))).unwrap(); assert_eq!(s.document.get_object(id).unwrap().blend_mode, dtool_core::blend::BlendMode::Normal);
    }
}

#[test]
fn reopening_duplicate_effect_kinds_without_edits_preserves_enabled_flags_and_history() {
    let (mut s, id) = setup();
    let mut disabled = default_effect(EffectKind::OuterShadow); disabled.enabled = false;
    let mut enabled = default_effect(EffectKind::OuterShadow); enabled.opacity = 0.35;
    for effect in [disabled, enabled] { s.run(&Command::with_params("effect.add", json!({"id": id.0, "effect": effect}))).unwrap(); }
    let before = effects(&s, id); let state = (s.generation, s.undo.applied_steps());
    let mut dialog = LayerEffectsDialog::default(); dialog.open_for(id.0);
    let ctx = egui::Context::default(); let mut out = ctx.run_ui(egui::RawInput::default(), |ui| dialog.show(ui.ctx(), &mut s)); out.textures_delta.clear();
    assert!(dialog.ok(&mut s)); assert_eq!(effects(&s, id), before); assert_eq!((s.generation, s.undo.applied_steps()), state);
}
