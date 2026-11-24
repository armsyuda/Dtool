use dtool_core::command::{Session, TransformSelectionBoxArgs};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_core::selection::{MarqueeMode, SelectOp};
use dtool_core::{Command, Document, ObjectId};
use dtool_tools::move_tool::{GestureKind, Handle, MoveTool, MoveToolAction, NudgeDirection, PointerEvent};

fn make_session() -> Session {
    Session::new(Document::default())
}

fn rect(session: &mut Session, x: f32, y: f32, w: f32, h: f32) -> u64 {
    let id = session.document.next_object_id.max(1);
    session.document.objects.push(Object { id: ObjectId(id), name: format!("rect-{id}"), pos: [x, y], size: [w, h], ..Object::default() });
    session.document.next_object_id = id + 1;
    id
}

fn group(session: &mut Session, children: &[u64]) -> u64 {
    let id = session.document.next_object_id.max(1);
    session.document.objects.push(Object { id: ObjectId(id), name: format!("group-{id}"), pos: [0.0, 0.0], size: [100.0, 100.0], kind: ObjectKind::Group, ..Object::default() });
    session.document.next_object_id = id + 1;
    for child in children {
        session.document.find_object_mut(*child).unwrap().parent_id = Some(ObjectId(id));
    }
    id
}

fn selected(session: &Session) -> Vec<u64> {
    session.selection.ids.iter().map(|id| id.0).collect()
}

fn queried(session: &mut Session) -> Vec<u64> {
    let out = session.run(&Command::new("selection.query")).expect("selection.query failed");
    out["ids"].as_array().expect("ids array").iter().filter_map(|value| value.as_u64()).collect()
}

fn marquee(session: &mut Session, rect: [f32; 4], mode: MarqueeMode, op: SelectOp) {
    let mut command = Command::new("selection.marquee");
    command.params["rect"] = rect.to_vec().into();
    command.params["mode"] = match mode { MarqueeMode::Touch => "Touch", MarqueeMode::Enclosed => "Enclosed" }.into();
    command.params["op"] = match op { SelectOp::Replace => "Replace", SelectOp::Add => "Add", SelectOp::Subtract => "Subtract", SelectOp::Intersect => "Intersect" }.into();
    session.run(&command).expect("selection.marquee failed");
}

fn hit(session: &mut Session, point: [f32; 2], cycle: bool) {
    let mut command = Command::new("selection.hit");
    command.params["point"] = point.to_vec().into();
    command.params["cycle"] = cycle.into();
    session.run(&command).expect("selection.hit failed");
}

fn set(session: &mut Session, ids: &[u64], op: &str) {
    let mut command = Command::new("selection.set");
    command.params["ids"] = ids.to_vec().into();
    command.params["op"] = op.into();
    session.run(&command).expect("selection.set failed");
}

fn nudge(session: &mut Session, dx: f32, dy: f32) {
    let mut command = Command::new("selection.nudge");
    command.params["dx"] = f64::from(dx).into();
    command.params["dy"] = f64::from(dy).into();
    session.run(&command).expect("selection.nudge failed");
}

fn transform(session: &mut Session, args: &TransformSelectionBoxArgs) {
    let mut command = Command::new("transform.selection_box");
    command.params["origin"] = args.origin.to_vec().into();
    command.params["translate"] = args.translate.to_vec().into();
    command.params["scale"] = args.scale.to_vec().into();
    command.params["rotation_deg"] = f64::from(args.rotation_deg).into();
    command.params["shear_deg"] = f64::from(args.shear_deg).into();
    session.run(&command).expect("transform.selection_box failed");
}

fn pos(session: &Session, id: u64) -> [f32; 2] {
    session.document.get_object(ObjectId(id)).unwrap().pos
}

fn pointer(x: f32, y: f32, pressed: bool, released: bool, shift: bool, ctrl: bool, alt: bool) -> PointerEvent {
    PointerEvent { x, y, pressed, released, shift, ctrl, alt }
}

fn drag_marquee(tool: &mut MoveTool, from: [f32; 2], to: [f32; 2], shift: bool, ctrl: bool, alt: bool) -> MoveToolAction {
    assert!(matches!(tool.on_pointer(pointer(from[0], from[1], true, false, shift, ctrl, alt)), MoveToolAction::MarqueeStart { .. }));
    assert!(matches!(tool.on_pointer(pointer(to[0], to[1], false, false, shift, ctrl, alt)), MoveToolAction::MarqueeUpdate { .. }));
    tool.on_pointer(pointer(to[0], to[1], false, true, shift, ctrl, alt))
}

fn commit_marquee(session: &mut Session, action: MoveToolAction) {
    match action {
        MoveToolAction::MarqueeCommit { rect, mode, op } => marquee(session, rect, mode, op),
        other => panic!("expected marquee commit, got {other:?}"),
    }
}

#[test]
fn test_marquee_touch_selects_intersecting_objects() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    marquee(&mut session, [5.0, 5.0, 10.0, 10.0], MarqueeMode::Touch, SelectOp::Replace);
    assert_eq!(queried(&mut session), vec![a]);
}

#[test]
fn test_marquee_enclosed_selects_only_fully_inside() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    rect(&mut session, 25.0, 25.0, 30.0, 30.0);
    marquee(&mut session, [5.0, 5.0, 30.0, 30.0], MarqueeMode::Enclosed, SelectOp::Replace);
    assert_eq!(queried(&mut session), vec![a]);
    let mut tool = MoveTool::default();
    let action = drag_marquee(&mut tool, [5.0, 5.0], [35.0, 35.0], false, true, false);
    assert!(matches!(action, MoveToolAction::MarqueeCommit { mode: MarqueeMode::Enclosed, op: SelectOp::Replace, .. }));
}

#[test]
fn test_marquee_shift_add() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    let mut tool = MoveTool::default();
    commit_marquee(&mut session, drag_marquee(&mut tool, [5.0, 5.0], [35.0, 35.0], false, false, false));
    assert_eq!(selected(&session), vec![a]);
    let action = drag_marquee(&mut tool, [45.0, 45.0], [65.0, 65.0], true, false, false);
    assert!(matches!(action, MoveToolAction::MarqueeCommit { op: SelectOp::Add, .. }));
    commit_marquee(&mut session, action);
    let mut ids = selected(&session);
    ids.sort();
    assert_eq!(ids, vec![a, b]);
}

#[test]
fn test_marquee_alt_does_not_subtract() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    set(&mut session, &[a, b], "Replace");
    let mut tool = MoveTool::default();
    let action = drag_marquee(&mut tool, [5.0, 5.0], [35.0, 35.0], false, false, true);
    assert!(matches!(action, MoveToolAction::MarqueeCommit { op: SelectOp::Replace, .. }));
    commit_marquee(&mut session, action);
    assert_eq!(selected(&session), vec![a]);
}

#[test]
fn test_selection_hit_click() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    let mut tool = MoveTool::default();
    tool.on_pointer(pointer(15.0, 15.0, true, false, false, false, false));
    let action = tool.on_pointer(pointer(15.0, 15.0, false, true, false, false, false));
    let MoveToolAction::HitSelect { point, cycle } = action else { panic!("click must hit-select") };
    assert!(!cycle);
    hit(&mut session, point, cycle);
    assert_eq!(queried(&mut session), vec![a]);
    hit(&mut session, [200.0, 200.0], false);
    assert!(queried(&mut session).is_empty());
}

#[test]
fn test_selection_hit_cycle_select_under() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 15.0, 15.0, 10.0, 10.0);
    hit(&mut session, [16.0, 16.0], false);
    let first = selected(&session);
    assert_eq!(first.len(), 1);
    assert!(first[0] == a || first[0] == b);
    hit(&mut session, [16.0, 16.0], true);
    let second = selected(&session);
    assert_eq!(second.len(), 1);
    assert_ne!(second, first);
    assert!(second[0] == a || second[0] == b);
}

#[test]
fn test_selection_enter_group() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 40.0, 10.0, 20.0, 20.0);
    let g = group(&mut session, &[a, b]);
    set(&mut session, &[g], "Replace");
    session.run(&Command::new("selection.enter")).expect("selection.enter failed");
    let mut ids = queried(&mut session);
    ids.sort();
    assert_eq!(ids, vec![a, b]);
    assert!(!ids.contains(&g));
}

#[test]
fn test_selection_exit_group() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let g = group(&mut session, &[a]);
    set(&mut session, &[g], "Replace");
    session.run(&Command::new("selection.enter")).expect("selection.enter failed");
    assert_eq!(selected(&session), vec![a]);
    session.run(&Command::new("selection.exit")).expect("selection.exit failed");
    assert_eq!(queried(&mut session), vec![g]);
}

#[test]
fn test_selection_nudge_1px() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    set(&mut session, &[a], "Replace");
    let tool = MoveTool::default();
    let args = tool.on_arrow(NudgeDirection::Right, false).expect("idle tool nudges");
    assert_eq!((args.dx, args.dy), (1.0, 0.0));
    nudge(&mut session, args.dx, args.dy);
    assert_eq!(pos(&session, a), [11.0, 10.0]);
    session.run(&Command::new("edit.undo")).expect("undo failed");
    assert_eq!(pos(&session, a), [10.0, 10.0]);
}

#[test]
fn test_selection_nudge_10x_shift() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    set(&mut session, &[a], "Replace");
    let tool = MoveTool::default();
    let args = tool.on_arrow(NudgeDirection::Right, true).unwrap();
    assert_eq!((args.dx, args.dy), (10.0, 0.0));
    nudge(&mut session, args.dx, args.dy);
    assert_eq!(pos(&session, a), [20.0, 10.0]);
    let up = tool.on_arrow(NudgeDirection::Up, true).unwrap();
    nudge(&mut session, up.dx, up.dy);
    assert_eq!(pos(&session, a), [20.0, 0.0]);
}

#[test]
fn test_move_gesture_one_undo_unit() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 100.0, 100.0);
    set(&mut session, &[a], "Replace");
    let mut tool = MoveTool::default();
    tool.set_selection_box(Some([10.0, 10.0, 100.0, 100.0]));
    assert!(matches!(tool.on_pointer(pointer(50.0, 50.0, true, false, false, false, false)), MoveToolAction::GestureStart { kind: GestureKind::Move }));
    tool.on_pointer(pointer(53.0, 53.0, false, false, false, false, false));
    tool.on_pointer(pointer(55.0, 55.0, false, false, false, false, false));
    assert_eq!(pos(&session, a), [10.0, 10.0]);
    let MoveToolAction::TransformCommit { args } = tool.on_pointer(pointer(55.0, 55.0, false, true, false, false, false)) else { panic!("drag must commit") };
    assert_eq!(args.translate, [5.0, 5.0]);
    transform(&mut session, &args);
    assert_eq!(pos(&session, a), [15.0, 15.0]);
    session.run(&Command::new("edit.undo")).expect("undo failed");
    assert_eq!(pos(&session, a), [10.0, 10.0]);
    assert!(session.run(&Command::new("edit.undo")).is_ok());
}

#[test]
fn test_move_cancel_does_not_commit() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 100.0, 100.0);
    set(&mut session, &[a], "Replace");
    let mut tool = MoveTool::default();
    tool.set_selection_box(Some([10.0, 10.0, 100.0, 100.0]));
    tool.on_pointer(pointer(50.0, 50.0, true, false, false, false, false));
    tool.on_pointer(pointer(55.0, 55.0, false, false, false, false, false));
    assert!(matches!(tool.escape(), MoveToolAction::TransformCancel));
    assert!(!tool.dragging);
    assert_eq!(pos(&session, a), [10.0, 10.0]);
    tool.on_pointer(pointer(50.0, 50.0, true, false, false, false, false));
    tool.on_pointer(pointer(60.0, 60.0, false, false, false, false, false));
    assert!(matches!(tool.on_focus_lost(), MoveToolAction::TransformCancel));
    assert_eq!(pos(&session, a), [10.0, 10.0]);
}

#[test]
fn test_escape_cancels_marquee() {
    let mut session = make_session();
    rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let mut tool = MoveTool::default();
    assert!(matches!(tool.on_pointer(pointer(5.0, 5.0, true, false, false, false, false)), MoveToolAction::MarqueeStart { .. }));
    assert!(matches!(tool.on_pointer(pointer(35.0, 35.0, false, false, false, false, false)), MoveToolAction::MarqueeUpdate { .. }));
    assert!(matches!(tool.escape(), MoveToolAction::MarqueeCancel));
    assert!(tool.marquee_start.is_none());
    assert!(queried(&mut session).is_empty());
}

#[test]
fn test_tab_does_not_cycle_selection() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    hit(&mut session, [15.0, 15.0], false);
    assert_eq!(selected(&session), vec![a]);
    let mut tool = MoveTool::default();
    assert!(matches!(tool.on_tab(), MoveToolAction::None));
    assert_eq!(queried(&mut session), vec![a]);
}

#[test]
fn test_selection_cycle_next_previous() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    hit(&mut session, [15.0, 15.0], false);
    assert_eq!(selected(&session), vec![a]);
    let mut next = Command::new("selection.cycle");
    next.params["direction"] = "Next".into();
    session.run(&next).expect("cycle next failed");
    let second = selected(&session);
    assert_eq!(second.len(), 1);
    assert_ne!(second, vec![a]);
    let mut previous = Command::new("selection.cycle");
    previous.params["direction"] = "Previous".into();
    session.run(&previous).expect("cycle previous failed");
    assert_eq!(queried(&mut session), vec![a]);
}

#[test]
fn test_selection_set_with_op() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    set(&mut session, &[a], "Replace");
    assert_eq!(selected(&session), vec![a]);
    set(&mut session, &[b], "Add");
    let mut ids = selected(&session);
    ids.sort();
    assert_eq!(ids, vec![a, b]);
    set(&mut session, &[a], "Subtract");
    assert_eq!(selected(&session), vec![b]);
    session.run(&Command::new("edit.undo")).expect("undo failed");
    let mut ids = selected(&session);
    ids.sort();
    assert_eq!(ids, vec![a, b]);
}

#[test]
fn test_edit_select_same_attribute() {
    let mut session = make_session();
    let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
    let green = Paint::Solid { colour: Colour::new(0, 255, 0, 255) };
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    let c = rect(&mut session, 80.0, 80.0, 15.0, 15.0);
    session.document.find_object_mut(a).unwrap().fill = red.clone();
    session.document.find_object_mut(b).unwrap().fill = red;
    session.document.find_object_mut(c).unwrap().fill = green;
    hit(&mut session, [15.0, 15.0], false);
    let mut command = Command::new("edit.select_same");
    command.params["attribute"] = "FillColour".into();
    session.run(&command).expect("edit.select_same failed");
    let mut ids = queried(&mut session);
    ids.sort();
    assert_eq!(ids, vec![a, b]);
    session.run(&Command::new("edit.undo")).expect("undo failed");
    assert_eq!(selected(&session), vec![a]);
    let mut width = Command::new("edit.select_same");
    width.params["attribute"] = "Width".into();
    session.run(&width).expect("select same width failed");
    assert_eq!(selected(&session), vec![a]);
    let mut unknown = Command::new("edit.select_same");
    unknown.params["attribute"] = "Unknown".into();
    assert_eq!(session.run(&unknown).unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn test_locked_object_not_selected_by_marquee() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 12.0, 12.0, 5.0, 5.0);
    session.document.find_object_mut(a).unwrap().locked = true;
    marquee(&mut session, [5.0, 5.0, 30.0, 30.0], MarqueeMode::Touch, SelectOp::Replace);
    assert_eq!(queried(&mut session), vec![b]);
    session.document.find_object_mut(b).unwrap().locked = true;
    marquee(&mut session, [5.0, 5.0, 30.0, 30.0], MarqueeMode::Touch, SelectOp::Replace);
    assert!(queried(&mut session).is_empty());
    set(&mut session, &[a], "Replace");
    let mut command = Command::new("selection.nudge");
    command.params["dx"] = 1.0_f64.into();
    command.params["dy"] = 0.0_f64.into();
    assert_eq!(session.run(&command).unwrap_err().code, "NOTHING_SELECTED");
    assert_eq!(pos(&session, a), [10.0, 10.0]);
}

#[test]
fn test_cancelled_rotate_does_not_commit() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    set(&mut session, &[a], "Replace");
    let bounds = [10.0, 10.0, 20.0, 20.0];
    let mut tool = MoveTool::default();
    tool.set_selection_box(Some(bounds));
    let grip = [bounds[0] + bounds[2] * 0.5, bounds[1] - tool.rotate_offset];
    assert_eq!(tool.hit_handle(bounds, grip), Some(Handle::Rotate));
    assert!(matches!(tool.on_pointer(pointer(grip[0], grip[1], true, false, false, false, false)), MoveToolAction::GestureStart { kind: GestureKind::Rotate }));
    tool.on_pointer(pointer(grip[0] + 30.0, grip[1] + 30.0, false, false, false, false, false));
    assert!(matches!(tool.escape(), MoveToolAction::TransformCancel));
    let object = session.document.get_object(ObjectId(a)).unwrap();
    assert_eq!(object.rotation_deg, 0.0);
    assert_eq!(object.pos, [10.0, 10.0]);
}

#[test]
fn test_move_tool_click_on_selection_without_drag_hit_selects() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 100.0, 100.0);
    let b = rect(&mut session, 40.0, 40.0, 30.0, 30.0);
    set(&mut session, &[a], "Replace");
    let mut tool = MoveTool::default();
    tool.set_selection_box(Some([10.0, 10.0, 100.0, 100.0]));
    tool.on_pointer(pointer(50.0, 50.0, true, false, false, false, false));
    let MoveToolAction::HitSelect { point, cycle } = tool.on_pointer(pointer(50.0, 50.0, false, true, false, false, true)) else { panic!("still click must hit-select") };
    assert!(cycle);
    hit(&mut session, point, cycle);
    let ids = selected(&session);
    assert_eq!(ids.len(), 1);
    assert!(ids[0] == a || ids[0] == b);
    assert!(tool.on_arrow(NudgeDirection::Left, false).is_some());
}
#[test]
fn test_alt_drag_reports_duplicate_offset() {
    let mut tool = MoveTool::default();
    tool.set_selection_box(Some([10.0, 10.0, 100.0, 100.0]));
    tool.on_pointer(pointer(50.0, 50.0, true, false, false, false, true));
    assert!(matches!(tool.on_pointer(pointer(60.0, 50.0, false, false, false, false, true)), MoveToolAction::DuplicatePreview { .. }));
    let action = tool.on_pointer(pointer(60.0, 50.0, false, true, false, false, true));
    assert_eq!(dtool_tools::move_tool::alt_drag_duplicate_offset(&action), Some([10.0, 0.0]));
}

#[test]
fn test_zero_size_marquee_becomes_hit() {
    let mut tool = MoveTool::default();
    tool.on_pointer(pointer(15.0, 15.0, true, false, false, false, false));
    let action = tool.on_pointer(pointer(15.0, 15.0, false, true, false, false, false));
    assert!(matches!(action, MoveToolAction::HitSelect { cycle: false, .. }));
    assert_eq!(dtool_tools::move_tool::selection_command_name(&action), Some("selection.hit"));
    assert!(dtool_tools::move_tool::selection_command_name(&MoveToolAction::ExitGroup) == Some("selection.exit"));
}

#[test]
fn test_selection_nudge_explicit_ids_and_redo() {
    let mut session = make_session();
    let a = rect(&mut session, 10.0, 10.0, 20.0, 20.0);
    let b = rect(&mut session, 50.0, 50.0, 10.0, 10.0);
    set(&mut session, &[a], "Replace");
    let mut command = Command::new("selection.nudge");
    command.params["dx"] = 0.0_f64.into();
    command.params["dy"] = 1.0_f64.into();
    command.params["ids"] = vec![b].into();
    session.run(&command).expect("nudge with ids failed");
    assert_eq!(pos(&session, a), [10.0, 10.0]);
    assert_eq!(pos(&session, b), [50.0, 51.0]);
    session.run(&Command::new("edit.undo")).expect("undo failed");
    assert_eq!(pos(&session, b), [50.0, 50.0]);
    session.run(&Command::new("edit.redo")).expect("redo failed");
    assert_eq!(pos(&session, b), [50.0, 51.0]);
    assert_eq!(selected(&session), vec![a]);
}