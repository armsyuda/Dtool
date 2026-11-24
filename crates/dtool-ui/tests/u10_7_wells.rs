use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint, PaintRole, StrokeStyle};
use dtool_ui::fill_stroke::{FillStrokeWells, PaintOwners, WellState};
use egui::{Context, RawInput};

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn session_with(fills: &[Paint]) -> Session {
    let mut document = Document::new([200.0, 200.0], Units::Pt, 72.0, 1);
    for (index, fill) in fills.iter().enumerate() {
        document.objects.push(Object { id: ObjectId(index as u64 + 1), fill: fill.clone(), stroke: StrokeStyle::default(), ..Object::default() });
    }
    Session::new(document)
}

fn owners(object: &mut Object) -> PaintOwners<'_> {
    PaintOwners { source: Some(&mut object.fill), frame: None, cell: None, edge: Some(&mut object.stroke), glyph: None }
}

fn fill_of(session: &Session, id: u64) -> Paint {
    session.document.get_object(ObjectId(id)).unwrap().fill.clone()
}

#[test]
fn wells_bar_popup_cancel_restores() {
    let mut fill = solid(10, 20, 30);
    let mut stroke = StrokeStyle::default();
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    let mut wells = FillStrokeWells::new(&owners);
    let ctx = Context::default();
    let mut clicked = false;
    for frame in 0..5 {
        let mut input = RawInput::default();
        if frame > 0 {
            let pos = egui::pos2(12.0, 12.0);
            input.events.push(egui::Event::PointerMoved(pos));
            if frame == 2 { input.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }); }
            if frame == 3 { input.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }); }
        }
        let mut out = ctx.run_ui(input, |ui| {
            egui::Area::new(egui::Id::new("wells")).fixed_pos(egui::pos2(0.0, 0.0)).show(ui.ctx(), |ui| {
                ui.horizontal(|ui| wells.show_context_bar(ui, &owners));
            });
        });
        out.textures_delta.clear();
        clicked |= wells.active_popup.is_some();
    }
    assert!(clicked, "fill well button did not open the popup");
    assert_eq!(wells.active_popup, Some(PaintRole::SourceObject));
    assert_eq!(wells.popup_original, Some(solid(10, 20, 30)));
    assert!(wells.preview_state(solid(200, 0, 0)));
    wells.cancel_state();
    assert!(wells.fill_state == WellState::Single(solid(10, 20, 30)));
    assert!(wells.active_popup.is_none());
    drop(owners);
    assert_eq!(fill, solid(10, 20, 30));
}

#[test]
fn wells_mixed_selection_is_mixed() {
    let mut session = session_with(&[solid(1, 2, 3), solid(4, 5, 6)]);
    let (a, b) = session.document.objects.split_at_mut(1);
    let wells = FillStrokeWells::new_from_selection(&[owners(&mut a[0]), owners(&mut b[0])]);
    assert!(wells.fill_state == WellState::Mixed);
    assert!(wells.stroke_state == WellState::Single(StrokeStyle::default().paint));
    assert!(wells.commit_command(PaintRole::SourceObject, &session.document, &[ObjectId(1), ObjectId(2)]).is_none());
    let mut same = session_with(&[solid(1, 2, 3), solid(1, 2, 3)]);
    let (a, b) = same.document.objects.split_at_mut(1);
    let wells = FillStrokeWells::new_from_selection(&[owners(&mut a[0]), owners(&mut b[0])]);
    assert!(wells.fill_state == WellState::Single(solid(1, 2, 3)));
    assert!(FillStrokeWells::new_from_selection(&[]).fill_state == WellState::None);
}

#[test]
fn wells_commit_is_undoable() {
    let mut session = session_with(&[solid(1, 2, 3), solid(4, 5, 6)]);
    let ids = [ObjectId(1), ObjectId(2)];
    let mut wells = {
        let (a, b) = session.document.objects.split_at_mut(1);
        let selection = [owners(&mut a[0]), owners(&mut b[0])];
        let mut wells = FillStrokeWells::new_from_selection(&selection);
        wells.open_popup_for(&selection[0], PaintRole::SourceObject);
        wells
    };
    assert!(wells.preview_state(solid(9, 9, 9)));
    assert_eq!(fill_of(&session, 1), solid(1, 2, 3));
    let command = wells.commit_state(&session.document, &ids).expect("commit command");
    assert_eq!(command.name, "edit.batch");
    let version = session.document.version;
    session.run(&command).expect("commit runs");
    assert_eq!(fill_of(&session, 1), solid(9, 9, 9));
    assert_eq!(fill_of(&session, 2), solid(9, 9, 9));
    assert!(session.document.version > version);
    session.run_named("edit.undo", serde_json::json!({})).expect("undo");
    assert_eq!(fill_of(&session, 1), solid(1, 2, 3));
    assert_eq!(fill_of(&session, 2), solid(4, 5, 6));
    assert!(wells.active_popup.is_none());
}

#[test]
fn wells_stroke_commit_keeps_width_and_is_undoable() {
    let mut session = session_with(&[solid(1, 2, 3)]);
    session.document.objects[0].stroke.width = 3.0;
    let mut wells = FillStrokeWells::new_from_selection(&[owners(&mut session.document.objects[0])]);
    wells.stroke_state = WellState::Single(solid(7, 7, 7));
    let command = wells.commit_command(PaintRole::Edge, &session.document, &[ObjectId(1)]).expect("stroke command");
    assert_eq!(command.name, "object.update");
    session.run(&command).expect("stroke runs");
    let stroke = session.document.get_object(ObjectId(1)).unwrap().stroke.clone();
    assert_eq!((stroke.paint.clone(), stroke.width), (solid(7, 7, 7), 3.0));
    session.run_named("edit.undo", serde_json::json!({})).expect("undo");
    assert_eq!(session.document.get_object(ObjectId(1)).unwrap().stroke.paint, StrokeStyle::default().paint);
    assert!(wells.commit_command(PaintRole::Edge, &session.document, &[ObjectId(1)]).is_some());
    wells.stroke_state = WellState::Single(StrokeStyle::default().paint);
    assert!(wells.commit_command(PaintRole::Edge, &session.document, &[ObjectId(1)]).is_none());
}
#[test]
fn wells_mixed_popup_opens_mixed_and_ok_without_pick_does_nothing() {
    let mut session = session_with(&[solid(1, 2, 3), solid(4, 5, 6)]);
    let ids = [ObjectId(1), ObjectId(2)];
    let mut wells = {
        let (a, b) = session.document.objects.split_at_mut(1);
        let selection = [owners(&mut a[0]), owners(&mut b[0])];
        let mut wells = FillStrokeWells::new_from_selection(&selection);
        wells.open_popup_for(&selection[0], PaintRole::SourceObject);
        wells
    };
    assert!(wells.active_popup == Some(PaintRole::SourceObject));
    assert!(wells.fill_state == WellState::Mixed, "no paint preselected for a mixed selection");
    let version = session.document.version;
    assert!(wells.commit_state(&session.document, &ids).is_none(), "OK with no pick commits nothing");
    assert_eq!(session.document.version, version);
    assert_eq!(fill_of(&session, 1), solid(1, 2, 3));
    assert_eq!(fill_of(&session, 2), solid(4, 5, 6));

    // Pick then cancel goes back to Mixed; pick then OK sets both.
    {
        let (a, b) = session.document.objects.split_at_mut(1);
        let selection = [owners(&mut a[0]), owners(&mut b[0])];
        wells.open_popup_for(&selection[0], PaintRole::SourceObject);
    }
    assert!(wells.preview_state(solid(9, 9, 9)));
    wells.cancel_state();
    assert!(wells.fill_state == WellState::Mixed);
    {
        let (a, b) = session.document.objects.split_at_mut(1);
        let selection = [owners(&mut a[0]), owners(&mut b[0])];
        wells.open_popup_for(&selection[0], PaintRole::SourceObject);
    }
    assert!(wells.preview_state(solid(9, 9, 9)));
    let command = wells.commit_state(&session.document, &ids).expect("picked paint commits");
    session.run(&command).expect("commit runs");
    assert_eq!(fill_of(&session, 1), solid(9, 9, 9));
    assert_eq!(fill_of(&session, 2), solid(9, 9, 9));
}
