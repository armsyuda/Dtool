use dtool_core::command::anchor_fraction;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::Session;
use dtool_design::{numeric_area_id, NumericEvent};
use dtool_ui::transform_panel::{field_id, TransformField, TransformPanel};
use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use serde_json::json;

const ID: ObjectId = ObjectId(1);

fn session() -> Session {
    let mut s = Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1));
    s.document.objects.push(Object { id: ID, pos: [10.0, 20.0], size: [100.0, 50.0], ..Object::default() });
    s.selection.ids = vec![ID];
    s
}

fn pos(s: &Session) -> [f32; 2] {
    s.document.get_object(ID).unwrap().pos
}

fn size(s: &Session) -> [f32; 2] {
    s.document.get_object(ID).unwrap().size
}

fn steps(s: &Session) -> usize {
    s.undo.applied_steps()
}

fn panel(s: &Session) -> TransformPanel {
    let mut p = TransformPanel::default();
    p.sync_from_session(s);
    p
}

fn commit(p: &mut TransformPanel, field: TransformField, value: f64) {
    p.handle_event(field, NumericEvent::Commit(value));
}

fn run(ctx: &Context, p: &mut TransformPanel, events: Vec<Event>) -> Rect {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 800.0))), events, ..Default::default() };
    let mut rect = Rect::NOTHING;
    ctx.run_ui(input, |ui| {
        rect = p.show(ui).rect;
    })
    .textures_delta
    .clear();
    rect
}

fn click(ctx: &Context, p: &mut TransformPanel, at: Pos2) {
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    run(ctx, p, vec![Event::PointerMoved(at)]);
    run(ctx, p, vec![button(true)]);
    run(ctx, p, vec![button(false)]);
    run(ctx, p, vec![]);
}

fn key(key: Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn type_and_enter(ctx: &Context, p: &mut TransformPanel, field: TransformField, events: Vec<Event>) {
    let at = ctx.read_response(numeric_area_id(field_id(field))).expect("field rendered").rect.center();
    click(ctx, p, at);
    run(ctx, p, events);
    run(ctx, p, vec![key(Key::Enter)]);
    run(ctx, p, vec![]);
}

#[test]
fn enter_commit_moves_object_with_one_undo() {
    let mut s = session();
    let mut p = panel(&s);
    let frac = anchor_fraction(p.anchor);
    let before = (pos(&s), steps(&s));
    commit(&mut p, TransformField::X, 300.0);
    p.apply_edits(&mut s);
    assert!(p.error.is_none(), "{:?}", p.error);
    assert!((pos(&s)[0] + size(&s)[0] * frac[0] - 300.0).abs() < 0.01, "{:?}", pos(&s));
    assert_eq!(pos(&s)[1], before.0[1]);
    assert_eq!(steps(&s), before.1 + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pos(&s), before.0);
}

#[test]
fn focus_loss_commit_moves_object_once() {
    let mut s = session();
    let mut p = panel(&s);
    let before = steps(&s);
    commit(&mut p, TransformField::X, 250.0);
    p.apply_edits(&mut s);
    p.apply_edits(&mut s);
    assert_ne!(pos(&s)[0], 10.0);
    assert_eq!(steps(&s), before + 1);
    assert_eq!(p.x, 250.0);
}

#[test]
fn escape_leaves_document_untouched() {
    let mut s = session();
    let mut p = panel(&s);
    let before = (pos(&s), steps(&s), p.x);
    p.handle_event(TransformField::X, NumericEvent::Preview(999.0));
    p.handle_event(TransformField::X, NumericEvent::Cancel);
    p.apply_edits(&mut s);
    assert_eq!((pos(&s), steps(&s), p.x), before);
}

#[test]
fn invalid_text_never_reaches_document() {
    let mut s = session();
    let mut p = panel(&s);
    let before = (pos(&s), size(&s), steps(&s));
    let ctx = Context::default();
    let header = run(&ctx, &mut p, vec![]).center();
    click(&ctx, &mut p, header);
    for _ in 0..4 {
        run(&ctx, &mut p, vec![]);
    }
    type_and_enter(&ctx, &mut p, TransformField::X, vec![Event::Text("abc".into())]);
    type_and_enter(&ctx, &mut p, TransformField::W, vec![key(Key::Backspace)]);
    assert!(p.edits.is_empty());
    p.apply_edits(&mut s);
    assert_eq!((pos(&s), size(&s), steps(&s)), before);
}

#[test]
fn width_commit_resizes_object() {
    let mut s = session();
    let mut p = panel(&s);
    commit(&mut p, TransformField::W, 200.0);
    p.apply_edits(&mut s);
    assert!(p.error.is_none(), "{:?}", p.error);
    assert!((size(&s)[0] - 200.0).abs() < 0.01, "{:?}", size(&s));
    assert_eq!(p.w, 200.0);
}

#[test]
fn relative_move_shifts_object_by_offset() {
    let mut s = session();
    let mut p = panel(&s);
    let before = (pos(&s), steps(&s));
    p.commit_relative(0, 15.0);
    p.apply_edits(&mut s);
    assert!(p.error.is_none(), "{:?}", p.error);
    assert!((pos(&s)[0] - (before.0[0] + 15.0)).abs() < 0.01, "{:?}", pos(&s));
    assert!((pos(&s)[1] - before.0[1]).abs() < 0.01);
    assert_eq!(steps(&s), before.1 + 1);
    assert_eq!(p.relative[0], 0.0);
}