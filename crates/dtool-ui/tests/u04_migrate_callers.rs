use dtool_core::document::{Document, Units};
use dtool_core::Session;
use dtool_design::{numeric_area_id, number_field};
use dtool_ui::history_panel::HistoryPanel;
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui};
use serde_json::json;

const MIGRATED: [(&str, &str); 4] = [
    ("history_panel.rs", include_str!("../src/history_panel.rs")),
    ("resize_document_dialog.rs", include_str!("../src/resize_document_dialog.rs")),
    ("slices_panel.rs", include_str!("../src/slices_panel.rs")),
    ("window_menu.rs", include_str!("../src/window_menu.rs")),
];

fn run(ctx: &Context, events: Vec<Event>, mut add: impl FnMut(&mut Ui)) {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 800.0))), events, ..Default::default() };
    ctx.run_ui(input, |ui| add(ui)).textures_delta.clear();
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn enter() -> Event {
    Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn type_into(ctx: &Context, id: Id, typed: &str, mut add: impl FnMut(&mut Ui)) {
    run(ctx, vec![], &mut add);
    let at = ctx.read_response(numeric_area_id(id)).expect("number field rendered").rect.center();
    for events in [vec![Event::PointerMoved(at)], vec![press(at, true)], vec![press(at, false)], vec![], vec![Event::Text(typed.into())], vec![enter()], vec![]] {
        run(ctx, events, &mut add);
    }
}

#[test]
fn migrated_files_have_no_drag_value() {
    for (name, src) in MIGRATED {
        assert!(!src.contains("DragValue"), "{name}");
        assert!(src.contains("dtool_design::number_drag("), "{name}");
    }
    let design = include_str!("../../dtool-design/src/components.rs");
    assert!(!design.contains("DragValue"));
}

#[test]
fn history_steps_field_sets_budget_and_clamps() {
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1));
    let mut panel = HistoryPanel { show_advanced: true };
    let ctx = Context::default();
    let id = Id::NULL.with("dtool.history.max_steps");
    type_into(&ctx, id, "50", |ui| panel.show(ui, &mut session));
    let budget = session.run_named("history.budget.query", json!({})).unwrap();
    assert_eq!(budget["max_steps"], json!(50));
    type_into(&ctx, id, "0", |ui| panel.show(ui, &mut session));
    let budget = session.run_named("history.budget.query", json!({})).unwrap();
    assert_eq!(budget["max_steps"], json!(1));
}

#[test]
fn design_number_field_commits_typed_value() {
    let ctx = Context::default();
    let mut value = 3.0;
    let mut changed = false;
    let mut id = None;
    run(&ctx, vec![], |ui| {
        id = Some(ui.auto_id_with(("dtool.number_field", "label")));
        changed |= number_field(ui, &mut value, "label").changed();
    });
    type_into(&ctx, id.unwrap(), "12.5", |ui| {
        changed |= number_field(ui, &mut value, "label").changed();
    });
    assert_eq!(value, 12.5);
    assert!(changed);
}