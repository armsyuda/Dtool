use dtool_core::commands::layer_style::{snapshot_staged, staged_effects};
use dtool_core::layer_style::{default_effect, EffectKind};
use dtool_core::{Command, Object, ObjectId, Session};
use serde_json::json;
fn setup() -> (Session, ObjectId) {
    let mut s = Session::new(dtool_core::Document::default());
    let id = s.document.create_object(Object { size: [40.0, 40.0], ..Default::default() });
    (s, id)
}
fn preview(s: &mut Session, id: ObjectId) {
    s.run(&Command::with_params("effect.preview", json!({"id": id.0, "effects": [default_effect(EffectKind::OuterShadow), default_effect(EffectKind::Outline)]}))).unwrap();
}
#[test]
fn several_effects_preview_together_without_document_or_undo_changes() {
    let (mut s, id) = setup(); let before = (s.generation, s.document.version, s.undo.applied_steps());
    preview(&mut s, id);
    assert_eq!(staged_effects(&s, id).len(), 2);
    assert_eq!(dtool_ui::app_canvas::AppCanvas::staged_effects(&s, id).len(), 2);
    assert!(s.document.get_object(id).unwrap().effects.is_empty());
    assert_eq!((s.generation, s.document.version, s.undo.applied_steps()), before);
    s.run(&Command::with_params("effect.cancel", json!({"id": id.0}))).unwrap();
    assert!(staged_effects(&s, id).is_empty());
}
#[test]
fn previews_follow_a_moved_session_and_never_leak_to_another_document() {
    let (mut s, id) = setup(); preview(&mut s, id);
    let moved = Box::new(s);
    assert_eq!(staged_effects(&moved, id).len(), 2);
    let (other, same_id) = setup(); assert_eq!(same_id, id);
    assert!(staged_effects(&other, same_id).is_empty());
    drop(moved);
    assert!(snapshot_staged(&other).is_empty());
}
#[test]
fn invalid_preview_and_failed_effect_batch_preserve_the_previous_preview() {
    let (mut s, id) = setup(); preview(&mut s, id); let before = snapshot_staged(&s);
    let mut bad = default_effect(EffectKind::OuterGlow); bad.opacity = 2.0;
    assert!(s.run(&Command::with_params("effect.preview", json!({"id": id.0, "effects": [bad]}))).is_err());
    assert_eq!(snapshot_staged(&s), before);
    assert!(s.run(&Command::with_params("edit.batch", json!({"commands": [
        {"name": "effect.add", "params": {"id": id.0, "kind": "OuterGlow"}},
        {"name": "effect.update", "params": {"id": id.0, "index": 99, "opacity": 0.5}}
    ]}))).is_err());
    assert!(s.document.get_object(id).unwrap().effects.is_empty());
    assert_eq!(snapshot_staged(&s), before);
}
#[test]
fn failed_macro_restores_effect_preview_as_well_as_document_state() {
    let (mut s, id) = setup(); preview(&mut s, id); let before = snapshot_staged(&s);
    let step = |command: &str, params| dtool_export::macros::MacroStep { command: command.into(), params, enabled: true, show_name: false };
    let steps = vec![step("effect.add", json!({"id": id.0, "kind": "OuterGlow"})), step("object.update", json!({"id": 999999, "name": "bad"}))];
    assert!(dtool_export::macros::play_steps(&mut s, &steps).is_err());
    assert!(s.document.get_object(id).unwrap().effects.is_empty());
    assert_eq!(snapshot_staged(&s), before);
}
