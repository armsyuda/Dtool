use dtool_core::paint::{Arrow, Cap, Colour, Join, Paint, PaintOrder, StrokeAlign, StrokeStyle};
use dtool_core::{Object, ObjectId, SharedServices, Workspace};
use dtool_ui::stroke_panel::StrokePanel;
use serde_json::{Value, json};
use std::sync::Arc;

fn fixture() -> (Workspace, ObjectId) {
    let mut workspace = Workspace::new(Arc::new(SharedServices {}));
    workspace.new_document("b17-stroke-acceptance");
    let session = workspace.session_mut().unwrap();
    let id = session.document.create_object(Object { fill: Paint::Solid { colour: Colour::new(10, 20, 30, 255) }, ..Object::default() });
    session.run_named("selection.set", json!({"ids": [id.0]})).unwrap();
    (workspace, id)
}

fn set(workspace: &mut Workspace, id: ObjectId, patch: Value) -> StrokeStyle {
    let session = workspace.session_mut().unwrap();
    let stroke = &session.document.objects.iter().find(|object| object.id == id).unwrap().stroke;
    let mut params = serde_json::to_value(stroke).unwrap();
    params["object_id"] = json!(id.0);
    for (key, value) in patch.as_object().unwrap() { params[key] = value.clone(); }
    session.run_named("stroke.set", params).unwrap();
    let result = session.run_named("stroke.query", json!({"object_id": id.0})).unwrap();
    serde_json::from_value(result).unwrap()
}

fn panel_matches_query(workspace: &Workspace, stroke: &StrokeStyle) {
    let mut panel = StrokePanel::new();
    panel.sync_from_selection(workspace);
    assert_eq!(panel.width_value, stroke.width);
    assert_eq!(panel.dash_values, stroke.dash);
    assert_eq!(panel.cap_value, stroke.cap);
    assert_eq!(panel.join_value, stroke.join);
    assert_eq!(panel.align_value, stroke.align);
    assert_eq!(panel.start_arrow_name, stroke.start_arrow.as_ref().map(|arrow| arrow.name.clone()));
    assert_eq!(panel.end_arrow_name, stroke.end_arrow.as_ref().map(|arrow| arrow.name.clone()));
    assert_eq!(panel.pressure_profile, stroke.pressure);
    assert_eq!(panel.scale_with_object, stroke.scale_with_object);
}

#[test]
fn real_stroke_set_width_solid_dashed_dotted_and_panel_query_agree() {
    let (mut workspace, id) = fixture();
    for (width, dash) in [(0.5, vec![]), (3.0, vec![4.0, 2.0]), (5.0, vec![1.0, 1.0])] {
        let stroke = set(&mut workspace, id, json!({"width": width, "dash": dash}));
        assert_eq!(stroke.width, width);
        assert_eq!(stroke.dash, dash);
        panel_matches_query(&workspace, &stroke);
    }
}

#[test]
fn real_stroke_set_all_caps_joins_and_alignments_match_panel() {
    let (mut workspace, id) = fixture();
    for cap in [Cap::Butt, Cap::Round, Cap::Square] {
        for join in [Join::Miter { limit: 4.0 }, Join::Round, Join::Bevel] {
            for align in [StrokeAlign::Inside, StrokeAlign::Centre, StrokeAlign::Outside] {
                let stroke = set(&mut workspace, id, json!({"width": 3.0, "cap": cap, "join": join, "align": align}));
                assert_eq!((stroke.cap, stroke.join, stroke.align), (cap, join, align));
                panel_matches_query(&workspace, &stroke);
            }
        }
    }
}

#[test]
fn real_arrowheads_scales_and_swap_are_visible_in_query_and_panel() {
    let (mut workspace, id) = fixture();
    let start = Arrow { name: "Arrow".into(), scale: 1.5 };
    let end = Arrow { name: "Circle".into(), scale: 2.5 };
    let stroke = set(&mut workspace, id, json!({"width": 2.0, "start_arrow": start, "end_arrow": end}));
    assert_eq!(stroke.start_arrow, Some(start.clone()));
    assert_eq!(stroke.end_arrow, Some(end.clone()));
    panel_matches_query(&workspace, &stroke);
    let mut panel = StrokePanel::new();
    panel.sync_from_selection(&workspace);
    assert_eq!(panel.arrow_scale, end.scale);
    workspace.session_mut().unwrap().run_named("stroke.arrows.swap", json!({"object_id": id.0})).unwrap();
    let value = workspace.session_mut().unwrap().run_named("stroke.query", json!({"object_id": id.0})).unwrap();
    let swapped: StrokeStyle = serde_json::from_value(value).unwrap();
    assert_eq!(swapped.start_arrow, Some(end));
    assert_eq!(swapped.end_arrow, Some(start));
    panel_matches_query(&workspace, &swapped);
}

#[test]
fn real_pressure_and_scale_with_object_round_trip_through_panel() {
    let (mut workspace, id) = fixture();
    let pressure = vec![[0.0, 0.25], [0.5, 1.0], [1.0, 0.5]];
    for scale in [false, true] {
        let stroke = set(&mut workspace, id, json!({"width": 4.0, "pressure": pressure, "scale_with_object": scale}));
        assert_eq!(stroke.pressure, pressure);
        assert_eq!(stroke.scale_with_object, scale);
        panel_matches_query(&workspace, &stroke);
    }
}

#[test]
fn real_paint_order_panel_queue_changes_only_order_and_supports_undo() {
    let (mut workspace, id) = fixture();
    set(&mut workspace, id, json!({"width": 3.0, "dash": [4.0, 2.0]}));
    let before = workspace.session().unwrap().document.objects.iter().find(|object| object.id == id).unwrap().clone();
    let mut panel = StrokePanel::new();
    panel.pending_commands.push(dtool_core::Command::with_params("paint.order.set", json!({"order": "StrokeUnder", "object_id": id.0})));
    panel.apply_pending(&mut workspace).unwrap();
    panel.sync_from_selection(&workspace);
    let after = workspace.session().unwrap().document.objects.iter().find(|object| object.id == id).unwrap();
    assert_eq!(after.paint_order, PaintOrder::StrokeUnder);
    assert_eq!(panel.paint_order_value, after.paint_order);
    assert_eq!(after.fill, before.fill);
    assert_eq!(after.stroke, before.stroke);
    workspace.session_mut().unwrap().run_named("edit.undo", json!({})).unwrap();
    let restored = workspace.session().unwrap().document.objects.iter().find(|object| object.id == id).unwrap();
    assert_eq!(restored.paint_order, before.paint_order);
    assert_eq!(restored.stroke, before.stroke);
}
