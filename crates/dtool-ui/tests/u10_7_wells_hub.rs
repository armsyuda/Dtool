use dtool_core::paint::{Colour, Paint, PaintRole, StrokeStyle};
use dtool_core::{Command, Object, ObjectId, Scope, Workspace};
use dtool_ui::fill_stroke::{FillStrokeWells, PaintOwners, WellState, PAINT_DEFAULT, PAINT_NONE, PAINT_SWAP};
use serde_json::{json, Value};

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn workspace_with_objects() -> (Workspace, ObjectId, ObjectId) {
    let mut workspace = Workspace::default();
    workspace.new_document("wells");
    let session = workspace.session_mut().expect("session");
    let a = session.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], fill: solid(255, 0, 0), stroke: StrokeStyle { paint: solid(0, 0, 255), width: 2.0, ..StrokeStyle::default() }, ..Object::default() });
    let b = session.document.create_object(Object { name: "b".into(), pos: [20.0, 0.0], size: [10.0, 10.0], fill: solid(0, 255, 0), ..Object::default() });
    (workspace, a, b)
}

fn run(workspace: &mut Workspace, name: &str, params: Value) -> Value {
    let (_, result) = workspace.run(Scope::active(), Command::with_params(name, params));
    result.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn wells_for(workspace: &mut Workspace, id: ObjectId) -> FillStrokeWells {
    let object = workspace.session_mut().expect("session").document.find_object_mut(id.0).expect("object");
    let owners = PaintOwners { source: Some(&mut object.fill), frame: None, cell: None, edge: Some(&mut object.stroke), glyph: None };
    FillStrokeWells::new(&owners)
}

fn single(state: &WellState) -> Paint {
    match state {
        WellState::Single(paint) => paint.clone(),
        WellState::Mixed => panic!("expected Single, got Mixed"),
        WellState::None => panic!("expected Single, got None"),
    }
}

fn live(workspace: &mut Workspace, id: ObjectId) -> (Paint, Paint) {
    let object = workspace.session_mut().expect("session").document.get_object(id).expect("object").clone();
    (object.fill, object.stroke.paint)
}

fn version(workspace: &mut Workspace) -> u64 {
    workspace.session_mut().expect("session").document.version
}

#[test]
fn u10_7_wells_read_matches_live_document_and_hub_query() {
    let (mut workspace, a, _) = workspace_with_objects();
    let wells = wells_for(&mut workspace, a);
    assert_eq!(single(wells.role_state(PaintRole::SourceObject)), solid(255, 0, 0));
    assert_eq!(single(wells.role_state(PaintRole::Edge)), solid(0, 0, 255));
    let stroke = run(&mut workspace, "stroke.query", json!({ "object_id": a.0 }));
    let paint = stroke.get("stroke").map(|s| s["paint"].clone()).unwrap_or_else(|| stroke["paint"].clone());
    assert_eq!(paint, serde_json::to_value(solid(0, 0, 255)).unwrap(), "{stroke}");
}

#[test]
fn u10_7_fill_well_commit_through_object_update_is_one_undo_step() {
    let (mut workspace, a, _) = workspace_with_objects();
    let mut wells = wells_for(&mut workspace, a);
    let mut scratch = live(&mut workspace, a).0;
    let mut stroke = StrokeStyle::default();
    let mut owners = PaintOwners { source: Some(&mut scratch), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    wells.preview_popup(&mut owners, solid(10, 20, 30));
    let paint = single(wells.role_state(PaintRole::SourceObject));
    let before = version(&mut workspace);
    run(&mut workspace, "object.update", json!({ "id": a.0, "fill": paint }));
    assert_eq!(live(&mut workspace, a).0, solid(10, 20, 30));
    assert!(version(&mut workspace) > before);
    assert_eq!(single(wells_for(&mut workspace, a).role_state(PaintRole::SourceObject)), solid(10, 20, 30));
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a).0, solid(255, 0, 0));
    assert_eq!(single(wells_for(&mut workspace, a).role_state(PaintRole::SourceObject)), solid(255, 0, 0));
    run(&mut workspace, "edit.redo", json!({}));
    assert_eq!(live(&mut workspace, a).0, solid(10, 20, 30));
}

#[test]
fn u10_7_stroke_well_commit_through_stroke_set_is_undoable() {
    let (mut workspace, a, _) = workspace_with_objects();
    run(&mut workspace, "stroke.set", json!({ "object_id": a.0, "paint": solid(1, 2, 3) }));
    assert_eq!(live(&mut workspace, a).1, solid(1, 2, 3));
    assert_eq!(single(wells_for(&mut workspace, a).role_state(PaintRole::Edge)), solid(1, 2, 3));
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a).1, solid(0, 0, 255));
    assert_eq!(single(wells_for(&mut workspace, a).role_state(PaintRole::Edge)), solid(0, 0, 255));
}

#[test]
fn u10_7_bar_actions_swap_none_default_through_hub() {
    let (mut workspace, a, b) = workspace_with_objects();
    run(&mut workspace, "paint.swap", json!({ "ids": [a.0, b.0] }));
    assert_eq!(live(&mut workspace, a), (solid(0, 0, 255), solid(255, 0, 0)));
    assert_eq!(live(&mut workspace, b).1, solid(0, 255, 0));
    let wells = wells_for(&mut workspace, a);
    assert_eq!(single(wells.role_state(PaintRole::SourceObject)), solid(0, 0, 255));
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a), (solid(255, 0, 0), solid(0, 0, 255)));
    run(&mut workspace, "paint.none", json!({ "object_id": a.0, "role": "fill" }));
    assert_eq!(live(&mut workspace, a).0, Paint::None);
    assert_eq!(single(wells_for(&mut workspace, a).role_state(PaintRole::SourceObject)), Paint::None);
    run(&mut workspace, "paint.default", json!({ "ids": [b.0] }));
    let defaults = dtool_core::commands::defaults::current();
    assert_eq!(live(&mut workspace, b), (defaults.fill.clone(), defaults.stroke.paint.clone()));
    run(&mut workspace, "edit.undo", json!({}));
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a).0, solid(255, 0, 0));
    assert_eq!(live(&mut workspace, b).0, solid(0, 255, 0));
}

#[test]
fn u10_7_cancel_sends_nothing_and_leaves_live_document() {
    let (mut workspace, a, _) = workspace_with_objects();
    let before = version(&mut workspace);
    let mut wells = wells_for(&mut workspace, a);
    let mut scratch = live(&mut workspace, a).0;
    let mut stroke = StrokeStyle::default();
    let mut owners = PaintOwners { source: Some(&mut scratch), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    wells.preview_popup(&mut owners, solid(9, 9, 9));
    wells.cancel_popup(&mut owners);
    assert_eq!(scratch, solid(255, 0, 0));
    assert_eq!(version(&mut workspace), before);
    assert_eq!(live(&mut workspace, a).0, solid(255, 0, 0));
}
fn selection_wells(workspace: &mut Workspace, ids: &[ObjectId]) -> FillStrokeWells {
    let session = workspace.session_mut().expect("session");
    let mut objects: Vec<&mut Object> = session.document.objects.iter_mut().filter(|o| ids.contains(&o.id)).collect();
    let owners: Vec<PaintOwners<'_>> = objects.iter_mut().map(|o| PaintOwners { source: Some(&mut o.fill), frame: None, cell: None, edge: Some(&mut o.stroke), glyph: None }).collect();
    FillStrokeWells::new_from_selection(&owners)
}

fn dispatch(workspace: &mut Workspace, command: Command) -> Value {
    let name = command.name.clone();
    let (_, result) = workspace.run(Scope::active(), command);
    result.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn steps(workspace: &mut Workspace) -> usize {
    workspace.session_mut().expect("session").undo.applied_steps()
}

#[test]
fn u10_7_fix_popup_cancel_restores_well_without_touching_document() {
    let (mut workspace, a, _) = workspace_with_objects();
    let before = version(&mut workspace);
    let mut wells = selection_wells(&mut workspace, &[a]);
    let mut fill = live(&mut workspace, a).0;
    let mut stroke = StrokeStyle::default();
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    assert!(wells.preview_state(solid(7, 7, 7)));
    assert_eq!(single(wells.role_state(PaintRole::SourceObject)), solid(7, 7, 7));
    wells.cancel_state();
    assert_eq!(single(wells.role_state(PaintRole::SourceObject)), solid(255, 0, 0));
    assert!(wells.active_popup.is_none());
    assert_eq!(version(&mut workspace), before);
    assert_eq!(live(&mut workspace, a).0, solid(255, 0, 0));
}

#[test]
fn u10_7_fix_mixed_selection_is_mixed_and_sends_nothing() {
    let (mut workspace, a, b) = workspace_with_objects();
    let wells = selection_wells(&mut workspace, &[a, b]);
    assert!(matches!(wells.role_state(PaintRole::SourceObject), WellState::Mixed));
    let document = workspace.session_mut().unwrap().document.clone();
    assert!(wells.commit_command(PaintRole::SourceObject, &document, &[a, b]).is_none());
    run(&mut workspace, "object.update", json!({ "id": b.0, "fill": solid(255, 0, 0) }));
    let same = selection_wells(&mut workspace, &[a, b]);
    assert_eq!(single(same.role_state(PaintRole::SourceObject)), solid(255, 0, 0));
}

#[test]
fn u10_7_fix_commit_command_is_one_undo_step_on_live_document() {
    let (mut workspace, a, b) = workspace_with_objects();
    let mut wells = selection_wells(&mut workspace, &[a, b]);
    let mut fill = Paint::None;
    let mut stroke = StrokeStyle::default();
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    wells.preview_state(solid(5, 6, 7));
    let document = workspace.session_mut().unwrap().document.clone();
    let command = wells.commit_state(&document, &[a, b]).expect("command");
    assert_eq!(command.name, "edit.batch");
    let (v0, s0) = (version(&mut workspace), steps(&mut workspace));
    dispatch(&mut workspace, command);
    assert!(version(&mut workspace) > v0);
    assert_eq!(steps(&mut workspace), s0 + 1);
    assert_eq!(live(&mut workspace, a).0, solid(5, 6, 7));
    assert_eq!(live(&mut workspace, b).0, solid(5, 6, 7));
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a).0, solid(255, 0, 0));
    assert_eq!(live(&mut workspace, b).0, solid(0, 255, 0));
    let mut edge = selection_wells(&mut workspace, &[a]);
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    edge.open_popup_for(&owners, PaintRole::Edge);
    edge.preview_state(solid(1, 1, 1));
    let document = workspace.session_mut().unwrap().document.clone();
    let command = edge.commit_state(&document, &[a]).expect("stroke command");
    assert_eq!(command.name, "object.update");
    dispatch(&mut workspace, command);
    assert_eq!(live(&mut workspace, a).1, solid(1, 1, 1));
    assert_eq!(workspace.session_mut().unwrap().document.get_object(a).unwrap().stroke.width, 2.0);
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a).1, solid(0, 0, 255));
}

#[test]
fn u10_7_fix_bar_buttons_dispatch_paint_commands() {
    let (mut workspace, a, b) = workspace_with_objects();
    let wells = selection_wells(&mut workspace, &[a]);
    assert!(wells.bar_command(PAINT_SWAP, &[]).is_none());
    assert!(wells.bar_command("paint.other", &[a]).is_none());
    let s0 = steps(&mut workspace);
    dispatch(&mut workspace, wells.bar_command(PAINT_SWAP, &[a, b]).expect("swap"));
    assert_eq!(live(&mut workspace, a), (solid(0, 0, 255), solid(255, 0, 0)));
    assert_eq!(steps(&mut workspace), s0 + 1);
    run(&mut workspace, "edit.undo", json!({}));
    let both = wells.bar_command(PAINT_NONE, &[a]).expect("none");
    assert_eq!(both.name, "edit.batch");
    dispatch(&mut workspace, both);
    assert_eq!(live(&mut workspace, a), (Paint::None, Paint::None));
    assert_eq!(steps(&mut workspace), s0 + 1);
    run(&mut workspace, "edit.undo", json!({}));
    assert_eq!(live(&mut workspace, a), (solid(255, 0, 0), solid(0, 0, 255)));
    let mut stroke_popup = selection_wells(&mut workspace, &[a]);
    let mut fill = Paint::None;
    let mut stroke = StrokeStyle::default();
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    stroke_popup.open_popup_for(&owners, PaintRole::Edge);
    let only_stroke = stroke_popup.bar_command(PAINT_NONE, &[a]).expect("none stroke");
    assert_eq!((only_stroke.name.as_str(), only_stroke.params["role"].as_str()), (PAINT_NONE, Some("stroke")));
    dispatch(&mut workspace, only_stroke);
    assert_eq!(live(&mut workspace, a), (solid(255, 0, 0), Paint::None));
    run(&mut workspace, "edit.undo", json!({}));
    let defaults = dtool_core::commands::defaults::current();
    dispatch(&mut workspace, wells.bar_command(PAINT_DEFAULT, &[b]).expect("default"));
    assert_eq!(live(&mut workspace, b), (defaults.fill.clone(), defaults.stroke.paint.clone()));
    let mut local = selection_wells(&mut workspace, &[a]);
    local.set_default();
    assert_eq!(single(local.role_state(PaintRole::SourceObject)), defaults.fill);
    assert_eq!(single(local.role_state(PaintRole::Edge)), defaults.stroke.paint);
}

#[test]
fn u10_7_fix_bar_without_input_returns_no_action() {
    let mut fill = solid(1, 2, 3);
    let mut stroke = StrokeStyle::default();
    let owners = PaintOwners { source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None };
    let mut wells = FillStrokeWells::new(&owners);
    let ctx = egui::Context::default();
    let mut action = Some("x");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        action = wells.show_context_bar(ui, &owners);
    });
    output.textures_delta.clear();
    assert!(action.is_none());
}
