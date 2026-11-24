use dtool_design::tokens::size;
use dtool_design::{icon_image_button, icon_image_button_selected, icon_texture_px, ControlState};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Response, Shape, TextureHandle, TextureId, Ui};

fn frame(ctx: &Context, events: Vec<Event>, mut add: impl FnMut(&mut Ui) -> Response) -> (Response, Vec<TextureId>) {
    let mut out = None;
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))), events, ..Default::default() };
    let mut full = ctx.run_ui(input, |ui| out = Some(add(ui)));
    full.textures_delta.clear();
    let mut ids = Vec::new();
    for clipped in full.shapes {
        collect(&clipped.shape, &mut ids);
    }
    (out.unwrap(), ids)
}

fn collect(shape: &Shape, ids: &mut Vec<TextureId>) {
    match shape {
        Shape::Mesh(mesh) => ids.push(mesh.texture_id),
        Shape::Rect(rect) => {
            if let Some(brush) = &rect.brush {
                ids.push(brush.fill_texture_id);
            }
        }
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, ids)),
        _ => {}
    }
}

fn texture(ctx: &Context) -> TextureHandle {
    ctx.load_texture("icons1.test", egui::ColorImage::filled([4, 4], egui::Color32::WHITE), egui::TextureOptions::LINEAR)
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

#[test]
fn icon_image_button_has_icon_size_and_draws_the_texture() {
    let ctx = Context::default();
    let handle = texture(&ctx);
    let (response, ids) = frame(&ctx, vec![], |ui| icon_image_button(ui, &handle, &ControlState::default()));
    let side = size("icon.size.md");
    assert!(response.rect.width() >= side && response.rect.height() >= side, "{:?}", response.rect);
    assert!(ids.contains(&handle.id()), "{ids:?}");
}

#[test]
fn icon_image_button_clicks_and_disabled_ignores() {
    let ctx = Context::default();
    let handle = texture(&ctx);
    assert!(click(&ctx, |ui| icon_image_button(ui, &handle, &ControlState::default())).clicked());
    let state = ControlState::disabled("no document");
    let response = click(&ctx, |ui| icon_image_button_selected(ui, &handle, true, &state));
    assert!(!response.clicked());
    assert!(!response.enabled());
}

#[test]
fn icon_texture_px_follows_pixels_per_point() {
    let ctx = Context::default();
    let mut one = 0;
    frame(&ctx, vec![], |ui| {
        one = icon_texture_px(ui);
        ui.label("")
    });
    assert_eq!(one, size("icon.size.md").ceil() as u32);
    ctx.set_pixels_per_point(2.0);
    let mut two = 0;
    frame(&ctx, vec![], |ui| {
        two = icon_texture_px(ui);
        ui.label("")
    });
    assert_eq!(two, (size("icon.size.md") * 2.0).ceil() as u32);
}