use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::Session;
use dtool_ui::dialogs::locked_delete::{LockedDeleteAnswer, LockedDeleteDialog};
use egui::{Context, Event, Modifiers, Pos2, RawInput, Rect};
use serde_json::json;

const ID: ObjectId = ObjectId(1);

fn session(locked: bool) -> Session {
    let mut s = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    s.document.objects.push(Object { id: ID, pos: [5.0, 5.0], size: [40.0, 30.0], locked, ..Object::default() });
    s.selection.ids = vec![ID];
    s
}

fn exists(s: &Session) -> bool {
    s.document.get_object(ID).is_some()
}

fn key(key: egui::Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn frame(ctx: &Context, dialog: &mut LockedDeleteDialog, s: &mut Session, events: Vec<Event>) {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 400.0))), events, ..Default::default() };
    ctx.run_ui(input, |ui| {
        dialog.show(ui, s);
    })
    .textures_delta
    .clear();
}

#[test]
fn unlocked_delete_runs_at_once_without_dialog() {
    let mut s = session(false);
    let before = s.undo.applied_steps();
    let mut dialog = LockedDeleteDialog::new();
    dialog.request_delete_selection(&mut s).unwrap();
    assert!(!exists(&s));
    assert!(!dialog.is_open());
    assert_eq!(s.undo.applied_steps(), before + 1);
}

#[test]
fn locked_delete_asks_then_enter_deletes_with_one_undo() {
    let mut s = session(true);
    let before = s.undo.applied_steps();
    let mut dialog = LockedDeleteDialog::new();
    let first = dialog.request_delete_selection(&mut s);
    assert!(LockedDeleteDialog::needs_confirm(&first));
    assert!(dialog.is_open());
    assert!(exists(&s));
    assert_eq!(s.undo.applied_steps(), before);
    let ctx = Context::default();
    frame(&ctx, &mut dialog, &mut s, vec![]);
    frame(&ctx, &mut dialog, &mut s, vec![key(egui::Key::Enter)]);
    assert_eq!(dialog.answer, Some(LockedDeleteAnswer::Yes));
    assert!(!exists(&s));
    assert_eq!(s.undo.applied_steps(), before + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert!(exists(&s));
    assert!(s.document.get_object(ID).unwrap().locked);
}

#[test]
fn locked_delete_escape_keeps_object() {
    let mut s = session(true);
    let before = s.undo.applied_steps();
    let mut dialog = LockedDeleteDialog::new();
    let _ = dialog.request_delete_selection(&mut s);
    let ctx = Context::default();
    frame(&ctx, &mut dialog, &mut s, vec![]);
    frame(&ctx, &mut dialog, &mut s, vec![key(egui::Key::Escape)]);
    assert_eq!(dialog.answer, Some(LockedDeleteAnswer::No));
    assert!(!dialog.is_open());
    assert!(exists(&s));
    assert_eq!(s.undo.applied_steps(), before);
}