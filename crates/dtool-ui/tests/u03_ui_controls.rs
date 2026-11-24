use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint};
use dtool_core::Session;
use dtool_design::WellValue;
use dtool_ui::dialogs::locked_delete::{LockedDeleteAnswer, LockedDeleteDialog};
use dtool_ui::fill_stroke::{mixed_well, paint_well_colour, well_value, WellState};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Response, Ui};

fn screen() -> Option<Rect> {
    Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 400.0)))
}

fn frame(ctx: &Context, events: Vec<Event>, mut add: impl FnMut(&mut Ui) -> Response) -> Response {
    let mut out = None;
    let input = RawInput { screen_rect: screen(), events, ..Default::default() };
    ctx.run_ui(input, |ui| out = Some(add(ui))).textures_delta.clear();
    out.unwrap()
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn click(ctx: &Context, mut add: impl FnMut(&mut Ui) -> Response) -> Response {
    let at = frame(ctx, vec![], &mut add).rect.center();
    frame(ctx, vec![Event::PointerMoved(at)], &mut add);
    frame(ctx, vec![press(at, true)], &mut add);
    frame(ctx, vec![press(at, false)], &mut add)
}

fn key(key: egui::Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1))
}

fn show(ctx: &Context, dialog: &mut LockedDeleteDialog, session: &mut Session, events: Vec<Event>) {
    let input = RawInput { screen_rect: screen(), events, ..Default::default() };
    ctx.run_ui(input, |ui| {
        dialog.show(ui, session);
    })
    .textures_delta
    .clear();
}

#[test]
fn well_value_maps_mixed_none_and_colour() {
    assert_eq!(well_value(&WellState::Mixed), WellValue::Mixed);
    assert_eq!(well_value(&WellState::None), WellValue::None);
    assert_eq!(well_value(&WellState::Single(Paint::None)), WellValue::None);
    let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
    let shown = paint_well_colour(&red).expect("solid paint shows a colour");
    assert_eq!(well_value(&WellState::Single(red)), WellValue::Colour(shown));
    assert_ne!(well_value(&WellState::Mixed), well_value(&WellState::None));
}

#[test]
fn mixed_well_stays_enabled_and_clickable() {
    let ctx = Context::default();
    let response = click(&ctx, |ui| mixed_well(ui, "fill.well", "mixed"));
    assert!(response.enabled());
    assert!(response.clicked());
}

#[test]
fn locked_delete_escape_answers_no() {
    let ctx = Context::default();
    let mut s = session();
    let mut dialog = LockedDeleteDialog::new();
    dialog.open_for(vec![7]);
    show(&ctx, &mut dialog, &mut s, vec![]);
    assert!(dialog.is_open());
    show(&ctx, &mut dialog, &mut s, vec![key(egui::Key::Escape)]);
    assert_eq!(dialog.answer, Some(LockedDeleteAnswer::No));
    assert!(!dialog.is_open());
    assert!(dialog.nodes.is_empty());
}

#[test]
fn locked_delete_enter_answers_yes() {
    let ctx = Context::default();
    let mut s = session();
    let mut dialog = LockedDeleteDialog::new();
    dialog.open_for(vec![7]);
    show(&ctx, &mut dialog, &mut s, vec![]);
    show(&ctx, &mut dialog, &mut s, vec![key(egui::Key::Enter)]);
    assert_eq!(dialog.answer, Some(LockedDeleteAnswer::Yes));
    assert!(!dialog.is_open());
}

#[test]
fn closed_locked_delete_ignores_keys() {
    let ctx = Context::default();
    let mut s = session();
    let mut dialog = LockedDeleteDialog::new();
    show(&ctx, &mut dialog, &mut s, vec![key(egui::Key::Enter)]);
    show(&ctx, &mut dialog, &mut s, vec![key(egui::Key::Escape)]);
    assert_eq!(dialog.answer, None);
    assert!(!dialog.is_open());
}

#[test]
fn dialogs_route_enter_and_escape_through_dialog_keys() {
    let sources = [
        ("dialogs.rs", include_str!("../src/dialogs.rs")),
        ("dialogs/export.rs", include_str!("../src/dialogs/export.rs")),
        ("dialogs/document_setup.rs", include_str!("../src/dialogs/document_setup.rs")),
        ("dialogs/locked_delete.rs", include_str!("../src/dialogs/locked_delete.rs")),
        ("dialogs/resource_manager.rs", include_str!("../src/dialogs/resource_manager.rs")),
    ];
    for (name, src) in sources {
        assert!(src.contains("dtool_design::dialog_keys(ui)"), "{name}");
    }
}

#[test]
fn context_bar_and_wells_use_design_components() {
    let bar = include_str!("../src/context_bar.rs");
    assert!(bar.contains("dtool_design::dropdown_value("));
    assert!(!bar.contains("egui::ComboBox"));
    let wells = include_str!("../src/fill_stroke.rs");
    assert!(wells.contains("dtool_design::colour_well_with("));
    assert!(!wells.contains("egui::ComboBox"));
}