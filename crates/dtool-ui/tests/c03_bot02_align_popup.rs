use dtool_core::document::{Document, Units};
use dtool_core::{ObjectId, Session};
use dtool_ui::context_bar::align::{AlignTarget, AlignmentPopup};
use serde_json::json;

fn create(s: &mut Session, pos: [f32; 2], size: [f32; 2]) -> ObjectId {
    let id = s.document.next_object_id;
    s.run_named("object.create", json!({"name": "o", "pos": pos, "size": size})).unwrap();
    ObjectId(id)
}

#[test]
fn popup_defaults_to_selection_and_enable_rules() {
    let mut popup = AlignmentPopup::default();
    assert_eq!(popup.target, AlignTarget::Selection);
    assert!(!popup.align_enabled(1));
    assert!(popup.align_enabled(2));
    assert!(!popup.distribute_enabled(2));
    assert!(popup.distribute_enabled(3));
    popup.target = AlignTarget::Spread;
    assert!(!popup.align_enabled(0));
    assert!(popup.align_enabled(1));
    assert!(popup.distribute_enabled(2));
    popup.target = AlignTarget::Key;
    assert!(!popup.align_enabled(1));
    assert!(popup.align_enabled(2));
    popup.target = AlignTarget::Selection;
    popup.spacing = Some(4.0);
    assert!(popup.distribute_enabled(2));
    assert!(!popup.distribute_enabled(1));
    popup.spacing = Some(f32::NAN);
    assert!(!popup.distribute_enabled(3));
}

#[test]
fn popup_commands_match_live_core_commands() {
    let mut popup = AlignmentPopup { target: AlignTarget::Spread, spacing: None };
    let cmd = popup.align_command("left").unwrap();
    assert_eq!(cmd.name, "layer.arrange.align.left");
    assert_eq!(cmd.params, json!({"relative_to": "spread"}));
    assert!(popup.align_command("diagonal").is_none());
    popup.spacing = Some(6.0);
    let cmd = popup.distribute_command(true);
    assert_eq!(cmd.name, "layer.arrange.distribute.h");
    assert_eq!(cmd.params, json!({"relative_to": "spread", "spacing": 6.0}));
    assert_eq!(popup.distribute_command(false).name, "layer.arrange.distribute.v");
}

#[test]
fn popup_fixed_spacing_command_runs_and_undoes() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
    let a = create(&mut s, [10.0, 0.0], [10.0, 10.0]);
    let b = create(&mut s, [80.0, 0.0], [10.0, 10.0]);
    s.selection.ids = vec![a, b];
    let popup = AlignmentPopup { target: AlignTarget::Selection, spacing: Some(5.0) };
    assert!(popup.distribute_enabled(s.selection.ids.len()));
    s.run(&popup.distribute_command(true)).unwrap();
    assert_eq!(s.document.get_object(b).unwrap().pos[0], 25.0);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.get_object(b).unwrap().pos[0], 80.0);
}

#[test]
fn popup_show_renders_without_command_when_idle() {
    let ctx = egui::Context::default();
    let mut popup = AlignmentPopup::default();
    let mut out = None;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        out = popup.show(ui, 2);
    })
    .textures_delta
    .clear();
    assert!(out.is_none());
}