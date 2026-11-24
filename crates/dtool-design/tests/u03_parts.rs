use dtool_design::tokens::{color, size, space};
use dtool_design::{
    button_with, frameless_button, frameless_button_with, list_row, list_row_with, number_drag, number_drag_with, numeric_area_id,
    small_icon_button, small_icon_button_side, small_icon_button_with, ControlState,
};
use egui::{Color32, Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Response, Shape, Ui};

fn frame(ctx: &Context, events: Vec<Event>, mut add: impl FnMut(&mut Ui) -> Response) -> (Response, Vec<Color32>) {
    let mut out = None;
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))), events, ..Default::default() };
    let mut full = ctx.run_ui(input, |ui| out = Some(add(ui)));
    full.textures_delta.clear();
    let mut fills = Vec::new();
    for clipped in full.shapes {
        collect(&clipped.shape, &mut fills);
    }
    (out.unwrap(), fills)
}

fn collect(shape: &Shape, fills: &mut Vec<Color32>) {
    match shape {
        Shape::Rect(rect) if rect.fill.a() > 0 || rect.stroke.width > 0.0 => fills.push(rect.fill),
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, fills)),
        _ => {}
    }
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn click(ctx: &Context, mut add: impl FnMut(&mut Ui) -> Response) -> Response {
    let at = frame(ctx, vec![], &mut add).0.rect.center();
    frame(ctx, vec![Event::PointerMoved(at)], &mut add);
    frame(ctx, vec![press(at, true)], &mut add);
    frame(ctx, vec![press(at, false)], &mut add).0
}

fn hover(ctx: &Context, mut add: impl FnMut(&mut Ui) -> Response) -> (Response, Vec<Color32>) {
    let at = frame(ctx, vec![], &mut add).0.rect.center();
    frame(ctx, vec![Event::PointerMoved(at)], &mut add);
    frame(ctx, vec![], &mut add)
}

#[test]
fn list_row_click_selects_and_paints_selected_fill() {
    let ctx = Context::default();
    let palette = dtool_design::theme::dark();
    dtool_design::theme::apply(&ctx, &palette);
    let mut selected = false;
    let response = click(&ctx, |ui| {
        let response = list_row(ui, selected, "row");
        if response.clicked() {
            selected = true;
        }
        response
    });
    assert!(response.clicked());
    assert!(selected);
    let (_, fills) = frame(&ctx, vec![Event::PointerMoved(Pos2::new(390.0, 290.0))], |ui| list_row(ui, true, "row"));
    assert!(fills.contains(&palette.accent), "{fills:?}");
    let (response, _) = frame(&ctx, vec![], |ui| list_row(ui, false, "row"));
    assert!(response.rect.height() >= size("size.list_row"));
}

#[test]
fn list_row_unselected_has_no_fill_until_hovered() {
    let ctx = Context::default();
    let (_, fills) = frame(&ctx, vec![], |ui| list_row(ui, false, "row"));
    assert!(!fills.contains(&color("accent")) && !fills.contains(&color("button.bg_hover")), "{fills:?}");
    let ctx = Context::default();
    let (response, fills) = hover(&ctx, |ui| list_row(ui, false, "row"));
    assert!(response.hovered());
    assert!(fills.contains(&color("button.bg_hover")), "{fills:?}");
}

#[test]
fn list_row_disabled_ignores_click() {
    let ctx = Context::default();
    let state = ControlState::disabled("locked");
    let response = click(&ctx, |ui| list_row_with(ui, false, "row", &state));
    assert!(!response.clicked());
    assert!(!response.enabled());
}

#[test]
fn small_icon_button_size_comes_from_tokens() {
    assert_eq!(small_icon_button_side(), size("icon.size.sm") + space("xs") * 2.0);
    let ctx = Context::default();
    let (response, fills) = frame(&ctx, vec![], |ui| small_icon_button(ui, "x"));
    assert_eq!(response.rect.size(), egui::Vec2::splat(small_icon_button_side()));
    assert!(!fills.contains(&color("button.bg_hover")));
    let ctx = Context::default();
    let (response, fills) = hover(&ctx, |ui| small_icon_button(ui, "x"));
    assert!(response.hovered());
    assert!(fills.contains(&color("button.bg_hover")), "{fills:?}");
}

#[test]
fn small_icon_button_clicks_and_disabled_ignores() {
    let ctx = Context::default();
    assert!(click(&ctx, |ui| small_icon_button(ui, "x")).clicked());
    let ctx = Context::default();
    let state = ControlState::disabled("first");
    let response = click(&ctx, |ui| small_icon_button_with(ui, "x", &state));
    assert!(!response.clicked());
    assert!(!response.enabled());
}

#[test]
fn frameless_button_paints_no_frame() {
    let ctx = Context::default();
    let (_, framed) = frame(&ctx, vec![], |ui| button_with(ui, "go", &ControlState::default()));
    assert!(!framed.is_empty());
    let ctx = Context::default();
    let (_, plain) = frame(&ctx, vec![], |ui| frameless_button(ui, "go"));
    assert!(plain.is_empty(), "{plain:?}");
    let ctx = Context::default();
    let (response, hovered) = hover(&ctx, |ui| frameless_button(ui, "go"));
    assert!(response.hovered());
    assert!(hovered.is_empty(), "{hovered:?}");
    let ctx = Context::default();
    assert!(click(&ctx, |ui| frameless_button(ui, "go")).clicked());
    let ctx = Context::default();
    let state = ControlState::disabled("none");
    let response = click(&ctx, |ui| frameless_button_with(ui, "go", &state));
    assert!(!response.clicked() && !response.enabled());
}

fn drag(value: f64, range: std::ops::RangeInclusive<f64>, speed: f64) -> f64 {
    let ctx = Context::default();
    let id = Id::NULL.with("u03.parts.drag");
    let mut v = value;
    let run = |events: Vec<Event>, v: &mut f64| {
        frame(&ctx, events, |ui| number_drag(ui, id, v, range.clone(), speed));
    };
    run(vec![], &mut v);
    let start = ctx.read_response(numeric_area_id(id)).unwrap().rect.center();
    run(vec![Event::PointerMoved(start)], &mut v);
    run(vec![press(start, true)], &mut v);
    for step in 1..=6 {
        run(vec![Event::PointerMoved(start + egui::vec2(step as f32 * 10.0, 0.0))], &mut v);
    }
    let end = start + egui::vec2(60.0, 0.0);
    run(vec![press(end, false)], &mut v);
    run(vec![], &mut v);
    v
}

#[test]
fn number_drag_speed_scales_the_drag() {
    let full = drag(10.0, 0.0..=1000.0, 1.0) - 10.0;
    let half = drag(10.0, 0.0..=1000.0, 0.5) - 10.0;
    assert!(full > 0.0, "{full}");
    assert!((full - 2.0 * half).abs() < 1e-9, "{full} {half}");
}

#[test]
fn number_drag_clamps_to_range() {
    assert_eq!(drag(10.0, 0.0..=20.0, 100.0), 20.0);
    let ctx = Context::default();
    let id = Id::NULL.with("u03.parts.type");
    let mut v = 5.0;
    let at = frame(&ctx, vec![], |ui| number_drag(ui, id, &mut v, 1.0..=50.0, 1.0)).0.rect.center();
    for events in [vec![Event::PointerMoved(at)], vec![press(at, true)], vec![press(at, false)], vec![], vec![Event::Text("999".into())]] {
        frame(&ctx, events, |ui| number_drag(ui, id, &mut v, 1.0..=50.0, 1.0));
    }
    frame(&ctx, vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], |ui| {
        number_drag(ui, id, &mut v, 1.0..=50.0, 1.0)
    });
    assert_eq!(v, 50.0);
}

#[test]
fn number_drag_disabled_does_not_change() {
    let ctx = Context::default();
    let id = Id::NULL.with("u03.parts.off");
    let state = ControlState::disabled("off");
    let mut v = 3.0;
    let at = frame(&ctx, vec![], |ui| number_drag_with(ui, id, &mut v, 0.0..=10.0, 1.0, &state)).0.rect.center();
    for events in [vec![Event::PointerMoved(at)], vec![press(at, true)], vec![Event::PointerMoved(at + egui::vec2(40.0, 0.0))], vec![press(at, false)]] {
        frame(&ctx, events, |ui| number_drag_with(ui, id, &mut v, 0.0..=10.0, 1.0, &state));
    }
    assert_eq!(v, 3.0);
}
