use dtool_design::{mark_multiline, multiline_focused, text_input_multiline, number_field, button_with, colour_well_with, dialog_keys, dropdown_value, slider_with, toggle_with, ControlState, DialogKey, WellValue, MIXED_TEXT};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Response, Ui};

fn frame(ctx: &Context, events: Vec<Event>, mut add: impl FnMut(&mut Ui) -> Response) -> Response {
    let mut out = None;
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))), events, ..Default::default() };
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

#[test]
fn button_enabled_clicks_and_disabled_ignores_click() {
    let ctx = Context::default();
    assert!(click(&ctx, |ui| button_with(ui, "go", &ControlState::default())).clicked());
    let ctx = Context::default();
    let state = ControlState::disabled("no document");
    let response = click(&ctx, |ui| button_with(ui, "go", &state));
    assert!(!response.clicked());
    assert!(!response.enabled());
}

#[test]
fn toggle_mixed_click_sets_true_and_plain_toggle_flips() {
    let ctx = Context::default();
    let mut on = false;
    let state = ControlState::default().mixed(true);
    let response = click(&ctx, |ui| toggle_with(ui, &mut on, "a", &state));
    assert!(response.clicked());
    assert!(on);
    let ctx = Context::default();
    let mut on = true;
    click(&ctx, |ui| toggle_with(ui, &mut on, "a", &ControlState::default()));
    assert!(!on);
    let ctx = Context::default();
    let mut on = false;
    click(&ctx, |ui| toggle_with(ui, &mut on, "a", &ControlState::enabled(false)));
    assert!(!on);
}

#[test]
fn mixed_is_distinct_from_disabled() {
    let mixed = ControlState::default().mixed(true);
    let disabled = ControlState::enabled(false);
    assert!(mixed.enabled && mixed.mixed);
    assert!(!disabled.enabled && !disabled.mixed);
    assert_ne!(WellValue::Mixed, WellValue::None);
    assert_eq!(MIXED_TEXT, "\u{2014}");
    let ctx = Context::default();
    let response = click(&ctx, |ui| colour_well_with(ui, WellValue::Mixed, "w", &mixed));
    assert!(response.clicked());
    let ctx = Context::default();
    let response = click(&ctx, |ui| colour_well_with(ui, WellValue::Colour(egui::Color32::RED), "w", &disabled));
    assert!(!response.clicked());
}

#[test]
fn colour_well_uses_id_and_is_focusable() {
    let ctx = Context::default();
    let a = frame(&ctx, vec![], |ui| colour_well_with(ui, WellValue::None, "a", &ControlState::default()));
    let b = frame(&ctx, vec![], |ui| colour_well_with(ui, WellValue::None, "b", &ControlState::default()));
    assert_ne!(a.id, b.id);
    let ctx = Context::default();
    let response = click(&ctx, |ui| colour_well_with(ui, WellValue::Colour(egui::Color32::from_rgba_unmultiplied(0, 0, 255, 128)), "c", &ControlState::default()));
    assert!(response.clicked());
    assert!(response.sense.senses_click());
}

#[test]
fn dropdown_value_keeps_value_without_input() {
    let ctx = Context::default();
    let mut value = "b".to_string();
    let options = [("a".to_string(), "A"), ("b".to_string(), "B")];
    let response = frame(&ctx, vec![], |ui| dropdown_value(ui, "dd", &mut value, &options, &ControlState::default()));
    assert!(!response.changed());
    assert_eq!(value, "b");
    let response = frame(&ctx, vec![], |ui| dropdown_value(ui, "dd", &mut value, &options, &ControlState::enabled(false)));
    assert!(!response.enabled());
}

#[test]
fn slider_mixed_and_disabled_do_not_write() {
    let ctx = Context::default();
    let mut value = 0.5;
    frame(&ctx, vec![], |ui| slider_with(ui, &mut value, 0.0..=1.0, "s", &ControlState::default().mixed(true)));
    assert_eq!(value, 0.5);
    let response = click(&ctx, |ui| slider_with(ui, &mut value, 0.0..=1.0, "s", &ControlState::enabled(false)));
    assert!(!response.enabled());
    assert_eq!(value, 0.5);
}

#[test]
fn tooltip_and_reason_are_kept() {
    let state = ControlState::default().tooltip("tip").reason("why");
    assert_eq!(state.tooltip.as_deref(), Some("tip"));
    assert_eq!(state.disabled_reason.as_deref(), Some("why"));
    let ctx = Context::default();
    frame(&ctx, vec![], |ui| button_with(ui, "go", &state));
}

#[test]
fn dialog_keys_map_enter_and_escape() {
    let ctx = Context::default();
    let mut got = None;
    let input = RawInput { events: vec![key(egui::Key::Escape)], ..Default::default() };
    ctx.run_ui(input, |ui| got = dialog_keys(ui)).textures_delta.clear();
    assert_eq!(got, Some(DialogKey::Cancel));
    let input = RawInput { events: vec![key(egui::Key::Enter)], ..Default::default() };
    ctx.run_ui(input, |ui| got = dialog_keys(ui)).textures_delta.clear();
    assert_eq!(got, Some(DialogKey::Confirm));
    ctx.run_ui(RawInput::default(), |ui| got = dialog_keys(ui)).textures_delta.clear();
    assert_eq!(got, None);
}

#[test]
fn components_do_not_reference_session() {
    let src = include_str!("../src/components.rs");
    for word in ["Session", "run_named", "std::fs"] {
        assert!(!src.contains(word), "{word}");
    }
}

#[test]
fn number_field_returns_inner_drag_value() {
    let ctx = Context::default();
    let mut value = 3.0;
    let response = frame(&ctx, vec![], |ui| number_field(ui, &mut value, "label"));
    assert!(response.sense.senses_drag());
    let plain = frame(&ctx, vec![], |ui| number_field(ui, &mut value, ""));
    assert!(plain.sense.senses_drag());
}

fn key_mod(key: egui::Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn keys_with(ctx: &Context, events: Vec<Event>, modifiers: Modifiers, multiline: bool) -> Option<DialogKey> {
    let mut got = None;
    let mut text = String::new();
    let _ = modifiers;
    let input = RawInput { events, ..Default::default() };
    ctx.run_ui(input, |ui| {
        let response = text_input_multiline(ui, &mut text);
        if multiline {
            response.request_focus();
            mark_multiline(ui, &response);
        }
        got = dialog_keys(ui);
    })
    .textures_delta
    .clear();
    got
}

#[test]
fn dialog_keys_accept_shift_and_ctrl_enter() {
    for m in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::CTRL, Modifiers::COMMAND] {
        let ctx = Context::default();
        assert_eq!(keys_with(&ctx, vec![key_mod(egui::Key::Enter, m)], m, false), Some(DialogKey::Confirm), "{m:?}");
    }
    let ctx = Context::default();
    assert_eq!(keys_with(&ctx, vec![key_mod(egui::Key::Enter, Modifiers::ALT)], Modifiers::ALT, false), None);
}

#[test]
fn dialog_keys_ignore_enter_in_focused_multiline() {
    let ctx = Context::default();
    keys_with(&ctx, vec![], Modifiers::NONE, true);
    keys_with(&ctx, vec![], Modifiers::NONE, true);
    for m in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::CTRL] {
        assert_eq!(keys_with(&ctx, vec![key_mod(egui::Key::Enter, m)], m, true), None, "{m:?}");
    }
    assert_eq!(keys_with(&ctx, vec![key_mod(egui::Key::Escape, Modifiers::NONE)], Modifiers::NONE, true), Some(DialogKey::Cancel));
    let mut focused = true;
    ctx.run_ui(RawInput::default(), |ui| {
        ui.memory_mut(|m| m.surrender_focus(m.focused().unwrap_or(egui::Id::NULL)));
        focused = multiline_focused(ui);
    })
    .textures_delta
    .clear();
    let after = keys_with(&ctx, vec![key_mod(egui::Key::Enter, Modifiers::NONE)], Modifiers::NONE, false);
    assert_eq!(after, Some(DialogKey::Confirm), "{focused}");
}
