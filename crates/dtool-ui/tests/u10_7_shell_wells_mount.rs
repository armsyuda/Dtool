#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::paint::{Colour, Paint};
use dtool_core::selection::SelectOp;
use dtool_protocol::Command;
use dtool_ui::fill_stroke::{WellState, PAINT_NONE};
use dtool_ui::shell::AppShell;
use serde_json::json;

fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
        _ => {}
    }
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) -> Vec<String> {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
    let mut found = Vec::new();
    for clipped in &out.shapes {
        texts(&clipped.shape, &mut found);
    }
    found
}

#[test]
fn shell_mounts_wells_and_bar_command_edits_live_doc_with_one_undo_step() {
    let _home = test_home::enter("shell_mounts_wells_and_bar_command_edits_live_doc_with_one_undo_step");
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("wells shell");
    shell.workspace.activate(doc).unwrap();
    shell.run_and_reconcile(Command::with_params("object.create", json!({ "name": "a", "pos": [0.0, 0.0], "size": [10.0, 10.0] }))).expect("create");
    let ctx = egui::Context::default();
    let well_label = dtool_strings::text("fill.well");
    assert!(!frame(&mut shell, &ctx).iter().any(|text| text == &well_label), "no wells without a selection");

    let id = shell.workspace.session().unwrap().document.objects.last().unwrap().id;
    let red = Paint::Solid { colour: Colour::new(200, 10, 10, 255) };
    shell.run_and_reconcile(Command::with_params("object.update", json!({ "id": id.0, "fill": red }))).expect("fill fixture");
    shell.workspace.session_mut().unwrap().selection.set(vec![id], SelectOp::Add);
    let drawn = frame(&mut shell, &ctx);
    assert!(!drawn.iter().any(|text| text == &well_label), "the swatch is icon-only: {drawn:?}");
    let fill = shell.workspace.session().unwrap().document.get_object(id).unwrap().fill.clone();
    assert!(fill != Paint::None, "fixture object needs a fill");
    assert!(shell.wells.fill_state == WellState::Single(fill.clone()));

    let steps = dtool_core::history_store::HistoryStore::from_undo(&shell.workspace.session().unwrap().undo).list().len();
    shell.wells_bar_action(PAINT_NONE).expect("bar command").expect("paint.none runs");
    let session = shell.workspace.session().unwrap();
    assert!(session.document.get_object(id).unwrap().fill == Paint::None);
    assert!(session.document.get_object(id).unwrap().stroke.paint == Paint::None);
    assert_eq!(dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len(), steps + 1, "one undo step");
    frame(&mut shell, &ctx);
    assert!(shell.wells.fill_state == WellState::Single(Paint::None) || shell.wells.fill_state == WellState::None);

    shell.run_and_reconcile(Command::with_params("edit.undo", json!({}))).expect("undo");
    assert!(shell.workspace.session().unwrap().document.get_object(id).unwrap().fill == fill);
}
