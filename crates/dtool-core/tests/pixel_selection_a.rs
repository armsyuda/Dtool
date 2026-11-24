use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::selection::{PixelSelection, SelectionOp};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{Value, json};

#[path = "../../dtool-tools/src/selection_tools.rs"]
mod selection_tools;

fn fixture() -> (Workspace, ContextId) {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("pixel-selection-acceptance");
    let session = &mut workspace.get_mut(id).unwrap().session;
    session.document = Document::new([24.0, 24.0], Units::Px, 96.0, 1);
    let mut rgba = vec![0; 24 * 24 * 4];
    for pixel in rgba.chunks_exact_mut(4) { pixel.copy_from_slice(&[0, 0, 255, 255]); }
    for x0 in [1, 8] {
        for y in 1..3 {
            for x in x0..x0 + 2 {
                let index = (y * 24 + x) * 4;
                rgba[index..index + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
    }
    session.document.objects.push(Object { id: ObjectId(1), name: "pixel-fixture".into(), size: [24.0, 24.0], raster_data: Some(rgba), raster_size: Some((24, 24)), ..Object::default() });
    session.document.next_object_id = 2;
    session.selection.ids = vec![ObjectId(1)];
    (workspace, id)
}

fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (target, result) = workspace.run(Scope::of(id), Command::with_params(name, params));
    assert_eq!(target, Some(id));
    result.unwrap()
}

fn mask(workspace: &Workspace, id: ContextId) -> Option<PixelSelection> {
    workspace.get(id).unwrap().session.document.pixel_selection.clone()
}

fn checked(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) {
    let session = &workspace.get(id).unwrap().session;
    let before = mask(workspace, id);
    let objects = serde_json::to_value(&session.document.objects).unwrap();
    let selection = session.selection.ids.clone();
    let depth = session.undo.applied_steps();
    run(workspace, id, name, params);
    assert_eq!(workspace.get(id).unwrap().session.undo.applied_steps(), depth + 1, "{name}");
    let after = mask(workspace, id);
    run(workspace, id, "edit.undo", json!({}));
    assert_eq!(mask(workspace, id), before, "undo {name}");
    assert_eq!(workspace.get(id).unwrap().session.undo.applied_steps(), depth);
    run(workspace, id, "edit.redo", json!({}));
    assert_eq!(mask(workspace, id), after, "redo {name}");
    let session = &workspace.get(id).unwrap().session;
    assert_eq!(session.undo.applied_steps(), depth + 1);
    assert_eq!(session.selection.ids, selection);
    assert_eq!(serde_json::to_value(&session.document.objects).unwrap(), objects);
}

fn rect(workspace: &mut Workspace, id: ContextId, x: u32, y: u32, width: u32, height: u32, op: &str) {
    checked(workspace, id, "selection.rect", json!({"rect": {"x": x, "y": y, "w": width, "h": height}, "op": op, "antialias": false}));
}

fn area(workspace: &Workspace, id: ContextId) -> f64 { mask(workspace, id).unwrap().area() }

#[test]
fn workspace_rect_twelve_by_six_points_at_96_dpi_selects_128_pixels_and_keeps_objects() {
    let (mut workspace, id) = fixture();
    checked(&mut workspace, id, "selection.rect", json!({"rect": {"x": 0, "y": 0, "w": "12pt", "h": "6pt"}, "antialias": false}));
    assert_eq!(area(&workspace, id), 128.0);
    assert_eq!(mask(&workspace, id).unwrap().bounds(), Some([0, 0, 16, 8]));
}

#[test]
fn workspace_freehand_triangle_has_area_36_and_zero_outside_corner() {
    let (mut workspace, id) = fixture();
    checked(&mut workspace, id, "selection.lasso", json!({"points": [[2, 2], [11, 2], [2, 10]], "mode": "freehand", "antialias": false}));
    let selected = mask(&workspace, id).unwrap();
    assert_eq!(selected.area(), 36.0);
    assert_eq!(selected.get(11, 10), 0);
}

#[test]
fn workspace_magic_wand_contiguous_four_pixels_vs_global_eight_pixels() {
    for (contiguous, expected) in [(true, 4.0), (false, 8.0)] {
        let (mut workspace, id) = fixture();
        checked(&mut workspace, id, "selection.magicWand", json!({"at": {"x": 1, "y": 1}, "contiguous": contiguous, "tolerance": 0, "sampling_scope": "current_layer", "op": "new"}));
        assert_eq!(area(&workspace, id), expected);
        assert_eq!(mask(&workspace, id).unwrap().get(8, 1), if contiguous { 0 } else { 255 });
    }
}

#[test]
fn workspace_add_then_subtract_has_area_150_then_100() {
    let (mut workspace, id) = fixture();
    rect(&mut workspace, id, 0, 0, 10, 10, "new");
    assert_eq!(area(&workspace, id), 100.0);
    rect(&mut workspace, id, 5, 0, 10, 10, "add");
    assert_eq!(area(&workspace, id), 150.0);
    rect(&mut workspace, id, 10, 0, 5, 10, "subtract");
    assert_eq!(area(&workspace, id), 100.0);
}

#[test]
fn workspace_grow_radius_one_has_nine_square_or_five_circular_pixels() {
    for (circular, expected) in [(false, 9.0), (true, 5.0)] {
        let (mut workspace, id) = fixture();
        rect(&mut workspace, id, 8, 8, 1, 1, "new");
        checked(&mut workspace, id, "selection.modify", json!({"operation": "grow", "radius": "1px", "circular": circular}));
        assert_eq!(area(&workspace, id), expected);
    }
}

#[test]
fn real_toolbar_modifier_op_maps_add_subtract_intersect() {
    assert_eq!(selection_tools::modifier_op(true, false), Some(SelectionOp::Add));
    assert_eq!(selection_tools::modifier_op(false, true), Some(SelectionOp::Subtract));
    assert_eq!(selection_tools::modifier_op(true, true), Some(SelectionOp::Intersect));
    assert_eq!(selection_tools::modifier_op(false, false), None);
}

#[test]
fn workspace_invert_and_selection_commands_each_restore_exact_coverage_in_one_undo() {
    let (mut workspace, id) = fixture();
    rect(&mut workspace, id, 0, 0, 10, 10, "new");
    checked(&mut workspace, id, "selection.invert", json!({}));
    assert_eq!(area(&workspace, id), 476.0);
    rect(&mut workspace, id, 0, 0, 10, 10, "intersect");
    assert_eq!(area(&workspace, id), 0.0);
}
