use dtool_design::{numeric_area_id, numeric_state};
use dtool_ui::transform_panel::{field_id, relative_field_id, PanelEdit, TransformField, TransformPanel, TransformPanelMode};
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};

const AWAY: Pos2 = Pos2::new(590.0, 790.0);

fn edits(panel: &TransformPanel) -> Vec<serde_json::Value> {
    panel.edits.iter().map(|e| match e {
        PanelEdit::Set(v) | PanelEdit::SelectionBox(v) | PanelEdit::Flip(v) | PanelEdit::Rotate90(v) => v.clone(),
    }).collect()
}

fn run(ctx: &Context, panel: &mut TransformPanel, events: Vec<Event>) -> Rect {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 800.0))), events, ..Default::default() };
    let mut rect = Rect::NOTHING;
    ctx.run_ui(input, |ui| {
        rect = panel.show(ui).rect;
    })
    .textures_delta
    .clear();
    rect
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn click(ctx: &Context, panel: &mut TransformPanel, at: Pos2) {
    run(ctx, panel, vec![Event::PointerMoved(at)]);
    run(ctx, panel, vec![press(at, true)]);
    run(ctx, panel, vec![press(at, false)]);
    run(ctx, panel, vec![]);
}

fn opened(panel: &mut TransformPanel) -> Context {
    let ctx = Context::default();
    let header = run(&ctx, panel, vec![]).center();
    click(&ctx, panel, header);
    run(&ctx, panel, vec![Event::PointerMoved(AWAY)]);
    for _ in 0..4 {
        run(&ctx, panel, vec![]);
    }
    ctx
}

fn area(ctx: &Context, id: Id) -> Rect {
    ctx.read_response(numeric_area_id(id)).expect("numeric field rendered").rect
}

fn type_into(ctx: &Context, panel: &mut TransformPanel, id: Id, typed: &str) {
    let at = area(ctx, id).center();
    click(ctx, panel, at);
    run(ctx, panel, vec![Event::Text(typed.to_string())]);
}

fn click_away(ctx: &Context, panel: &mut TransformPanel) {
    click(ctx, panel, AWAY);
    run(ctx, panel, vec![]);
}

#[test]
fn focus_loss_commits_changed_value_once() {
    let mut panel = TransformPanel::default();
    let ctx = opened(&mut panel);
    type_into(&ctx, &mut panel, field_id(TransformField::X), "50");
    assert_eq!(panel.focused, Some(TransformField::X));
    click_away(&ctx, &mut panel);
    let e = edits(&panel);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0]["x"], serde_json::json!(50.0));
    assert!(panel.focused.is_none());
}

#[test]
fn focus_loss_without_change_commits_nothing() {
    let mut panel = TransformPanel::default();
    let ctx = opened(&mut panel);
    let at = area(&ctx, field_id(TransformField::W)).center();
    click(&ctx, &mut panel, at);
    assert_eq!(panel.focused, Some(TransformField::W));
    click_away(&ctx, &mut panel);
    assert!(panel.edits.is_empty());
    assert!(panel.focused.is_none());
}

#[test]
fn escape_cancels_and_does_not_commit() {
    let mut panel = TransformPanel::default();
    let ctx = opened(&mut panel);
    type_into(&ctx, &mut panel, field_id(TransformField::Y), "77");
    run(&ctx, &mut panel, vec![key(Key::Escape, Modifiers::NONE)]);
    run(&ctx, &mut panel, vec![]);
    assert!(panel.edits.is_empty());
    assert_eq!(panel.y, 0.0);
    assert!(numeric_state(&ctx, field_id(TransformField::Y)).buffer.is_none());
}

#[test]
fn enter_commits() {
    let mut panel = TransformPanel::default();
    let ctx = opened(&mut panel);
    type_into(&ctx, &mut panel, field_id(TransformField::R), "15");
    run(&ctx, &mut panel, vec![key(Key::Enter, Modifiers::NONE)]);
    let e = edits(&panel);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0]["rotation_deg"], serde_json::json!(15.0));
}

#[test]
fn tab_commits_and_moves_to_next_field() {
    let mut panel = TransformPanel::default();
    let ctx = opened(&mut panel);
    type_into(&ctx, &mut panel, field_id(TransformField::X), "42");
    run(&ctx, &mut panel, vec![key(Key::Tab, Modifiers::NONE)]);
    for _ in 0..3 {
        run(&ctx, &mut panel, vec![]);
    }
    let e = edits(&panel);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0]["x"], serde_json::json!(42.0));
    assert_eq!(panel.focused, Some(TransformField::Y));
    assert!(numeric_state(&ctx, field_id(TransformField::Y)).buffer.is_some());
}

#[test]
fn pending_focus_starts_editing_the_field() {
    let mut panel = TransformPanel::default();
    let ctx = opened(&mut panel);
    panel.pending_focus = Some(TransformField::H);
    run(&ctx, &mut panel, vec![]);
    run(&ctx, &mut panel, vec![]);
    assert_eq!(panel.focused, Some(TransformField::H));
    run(&ctx, &mut panel, vec![Event::Text("7".into())]);
    run(&ctx, &mut panel, vec![key(Key::Enter, Modifiers::NONE)]);
    let e = edits(&panel);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0]["h"], serde_json::json!(7.0));
}

#[test]
fn mixed_field_blur_without_typing_commits_nothing() {
    let mut panel = TransformPanel::default();
    panel.mixed_x = true;
    let ctx = opened(&mut panel);
    let at = area(&ctx, field_id(TransformField::X)).center();
    click(&ctx, &mut panel, at);
    run(&ctx, &mut panel, vec![key(Key::Enter, Modifiers::NONE)]);
    click_away(&ctx, &mut panel);
    assert!(panel.edits.is_empty());
}

#[test]
fn relative_escape_keeps_neutral_and_enter_commits_once() {
    let mut panel = TransformPanel::default();
    panel.mode = TransformPanelMode::Relative;
    let ctx = opened(&mut panel);
    type_into(&ctx, &mut panel, relative_field_id(0), "25");
    run(&ctx, &mut panel, vec![key(Key::Escape, Modifiers::NONE)]);
    run(&ctx, &mut panel, vec![]);
    assert!(panel.edits.is_empty());
    assert_eq!(panel.relative[0], 0.0);
    type_into(&ctx, &mut panel, relative_field_id(1), "12");
    run(&ctx, &mut panel, vec![key(Key::Enter, Modifiers::NONE)]);
    let e = edits(&panel);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0]["translate"], serde_json::json!([0.0, 12.0]));
    assert_eq!(panel.relative[1], 0.0);
}

#[test]
fn commit_api_keeps_value_rules() {
    let mut panel = TransformPanel::default();
    panel.commit_relative(2, 100.0);
    panel.commit_relative(0, 0.0);
    assert!(panel.edits.is_empty());
    panel.commit_relative(2, 150.0);
    assert_eq!(edits(&panel)[0]["scale"], serde_json::json!([1.5, 1.5]));
    assert_eq!(panel.relative[2], 100.0);
    panel.commit_value(TransformField::W, 80.0);
    assert_eq!(edits(&panel)[1]["w"], serde_json::json!(80.0));
    panel.commit_value(TransformField::Anchor, 1.0);
    assert_eq!(panel.edits.len(), 2);
    panel.w = 3.0;
    panel.h = 4.0;
    panel.commit_length(5.0);
    assert_eq!(panel.edits.len(), 2);
    panel.commit_length(10.0);
    assert_eq!(edits(&panel)[2]["w"], serde_json::json!(6.0));
}