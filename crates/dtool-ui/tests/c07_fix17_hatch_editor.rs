use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Hatch, HatchLine, HatchUnits, Paint};
use dtool_core::Session;
use dtool_ui::hatch_editor::{HatchEditorState, HatchRole, LineEdit};

fn hatch(colour: Colour) -> Hatch {
    Hatch {
        lines: vec![HatchLine { angle_rad: 0.5, origin: [0.0, 0.0], shift: 0.0, spacing: 6.0, dash: Vec::new() }],
        colour,
        background: None,
        weight: 1.0,
        units: HatchUnits::Px,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

fn session() -> Session {
    let mut document = Document::new([200.0, 200.0], Units::Px, 96.0, 1);
    let mut object = Object { id: ObjectId(1), size: [10.0, 10.0], ..Object::default() };
    object.fill = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    object.stroke.paint = Paint::Hatch(hatch(Colour::new(0, 0, 255, 255)));
    document.objects.push(object);
    document.next_object_id = 2;
    let mut session = Session::new(document);
    session.selection.ids = vec![ObjectId(1)];
    session
}

#[test]
fn fill_hatch_edit_commits_once_and_keeps_stroke() {
    let mut session = session();
    let stroke = session.document.objects[0].stroke.paint.clone();
    let depth = session.undo.applied_steps();
    let mut state = HatchEditorState::new();
    assert!(state.load_from_session(&session, &[ObjectId(1)], HatchRole::Fill));
    state.apply(LineEdit::Add);
    state.apply(LineEdit::ToggleDash(1));
    assert_eq!(session.document.objects[0].fill, Paint::Hatch(hatch(Colour::new(0, 0, 0, 255))));
    let command = state.set_command(Some(&[ObjectId(1)]), HatchRole::Fill).unwrap();
    assert_eq!(command.name, "fill.hatch.set");
    assert_eq!(command.params["role"], "fill");
    assert!(state.commit(&mut session, Some(&[ObjectId(1)]), HatchRole::Fill).unwrap());
    assert_eq!(session.undo.applied_steps(), depth + 1);
    match &session.document.objects[0].fill { Paint::Hatch(h) => assert_eq!(h.lines.len(), 2), other => panic!("{other:?}") }
    assert_eq!(session.document.objects[0].stroke.paint, stroke);
    assert!(!state.commit(&mut session, Some(&[ObjectId(1)]), HatchRole::Fill).unwrap());
    assert_eq!(session.undo.applied_steps(), depth + 1);
    session.run_named("edit.undo", serde_json::json!({})).unwrap();
    assert_eq!(session.document.objects[0].fill, Paint::Hatch(hatch(Colour::new(0, 0, 0, 255))));
    assert_eq!(session.document.objects[0].stroke.paint, stroke);
}

#[test]
fn swatch_hatch_add_is_one_undo() {
    let mut session = session();
    let depth = session.undo.applied_steps();
    let count = session.document.swatches.len();
    let mut state = HatchEditorState::new();
    state.load_from_session(&session, &[ObjectId(1)], HatchRole::Stroke);
    let id = state.add_swatch(&mut session, "Blue Hatch", Some("C07")).unwrap().unwrap();
    assert_eq!(session.undo.applied_steps(), depth + 1);
    let added = session.document.swatches.iter().find(|s| s.id == id).unwrap();
    assert_eq!(added.paint, Some(Paint::Hatch(hatch(Colour::new(0, 0, 255, 255)))));
    session.run_named("edit.undo", serde_json::json!({})).unwrap();
    assert_eq!(session.document.swatches.len(), count);
}

#[test]
fn cancel_leaves_document_unchanged() {
    let mut session = session();
    let version = session.document.version;
    let before = (session.document.objects[0].fill.clone(), session.document.objects[0].stroke.paint.clone(), session.undo.applied_steps());
    let mut state = HatchEditorState::new();
    state.load_from_session(&session, &[ObjectId(1)], HatchRole::Fill);
    state.apply(LineEdit::Add);
    state.cancel();
    assert!(!state.commit(&mut session, Some(&[ObjectId(1)]), HatchRole::Fill).unwrap());
    assert_eq!(session.document.version, version);
    assert_eq!((session.document.objects[0].fill.clone(), session.document.objects[0].stroke.paint.clone(), session.undo.applied_steps()), before);
}

#[test]
fn editor_has_no_in_place_owner_writer() {
    let source = include_str!("../src/hatch_editor.rs");
    assert!(!source.contains("write_to_owner"));
    assert!(!source.contains("paint_mut"));
    assert!(source.contains("FILL_HATCH_SET") && source.contains("SWATCH_HATCH_ADD"));
}
