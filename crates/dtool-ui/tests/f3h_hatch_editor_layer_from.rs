use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Hatch, HatchLine, HatchUnits, Paint, PaintRole};
use dtool_core::Session;
use dtool_ui::fill_stroke::PaintOwners;
use dtool_ui::hatch_editor::{HatchEditorState, LineEdit};
use dtool_ui::snapshots_panel;
use serde_json::json;

fn hatch() -> Hatch {
    Hatch {
        lines: vec![HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 6.0, dash: Vec::new() }],
        colour: Colour::new(0, 0, 0, 255),
        background: None,
        weight: 1.0,
        units: HatchUnits::Px,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

fn owners(source: &mut Paint) -> PaintOwners<'_> {
    PaintOwners { source: Some(source), frame: None, cell: None, edge: None, glyph: None }
}

#[test]
fn hatch_editor_line_edits() {
    let mut state = HatchEditorState::new();
    assert!(!state.apply(LineEdit::Add));
    state.set_hatch(Some(hatch()));
    assert!(state.apply(LineEdit::Add));
    assert_eq!(state.get_hatch().unwrap().lines.len(), 2);
    assert_eq!(state.get_hatch().unwrap().lines[1].spacing, 6.0);
    state.set_hatch(Some({ let mut h = state.get_hatch().unwrap().clone(); h.lines[1].spacing = 9.0; h }));
    assert!(state.apply(LineEdit::MoveUp(1)));
    assert_eq!(state.get_hatch().unwrap().lines[0].spacing, 9.0);
    assert!(!state.apply(LineEdit::MoveUp(0)));
    assert!(state.apply(LineEdit::MoveDown(0)));
    assert!(!state.apply(LineEdit::MoveDown(1)));
    assert!(state.apply(LineEdit::ToggleDash(0)));
    assert_eq!(state.get_hatch().unwrap().lines[0].dash, vec![1.0, 1.0]);
    assert!(state.apply(LineEdit::ToggleDash(0)));
    assert!(state.get_hatch().unwrap().lines[0].dash.is_empty());
    assert!(state.apply(LineEdit::Remove(0)));
    assert!(!state.apply(LineEdit::Remove(0)));
    assert_eq!(state.get_hatch().unwrap().lines.len(), 1);
}

#[test]
fn hatch_editor_reads_owner_and_tracks_edits() {
    let mut paint = Paint::Hatch(hatch());
    let mut state = HatchEditorState::new();
    {
        let o = owners(&mut paint);
        assert!(state.load_from_owner(&o, PaintRole::SourceObject));
    }
    assert!(!state.is_dirty());
    state.apply(LineEdit::Add);
    assert!(state.is_dirty());
    state.cancel();
    assert!(!state.is_dirty());
    match &paint { Paint::Hatch(h) => assert_eq!(h.lines.len(), 1), _ => panic!("expected hatch") }
    let mut solid = Paint::None;
    let o = owners(&mut solid);
    assert!(!HatchEditorState::new().load_from_owner(&o, PaintRole::SourceObject));
}

#[test]
fn snapshots_panel_layer_from_adds_group() {
    let mut s = Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1));
    s.run_named("object.create", json!({"name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    let id = snapshots_panel::create(&mut s).unwrap();
    let group = snapshots_panel::layer_from(&mut s, id).unwrap();
    assert_eq!(s.document.objects.len(), 3);
    assert!(s.document.objects.iter().any(|o| o.parent_id.map(|p| p.0) == Some(group)));
    assert!(snapshots_panel::layer_from(&mut s, id + 50).is_none());
    assert!(dtool_strings::has("snapshot.layer_from"));
}
