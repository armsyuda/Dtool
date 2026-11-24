#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::layer_style::{default_effect, EffectKind, EffectParams};
use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint};
use dtool_ui::canvas_input::{tool_kind, ToolKind};
use dtool_ui::shell::AppShell;
use egui::{pos2, Event, PointerButton, Pos2};

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<Event>) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn click(shell: &mut AppShell, ctx: &egui::Context, at: Pos2) {
    frame(shell, ctx, vec![Event::PointerMoved(at)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, Vec::new());
}

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

#[test]
fn eyedropper_click_picks_the_live_effect_colour() {
    let _home = test_home::enter("c07_shell_picker_live");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c07 picker");
    shell.workspace.activate(doc).unwrap();
    assert_eq!(tool_kind("tool.eyedropper"), ToolKind::Eyedropper);
    let target = {
        let session = shell.workspace.session_mut().unwrap();
        let mut overlay = default_effect(EffectKind::ColourOverlay);
        overlay.params = EffectParams::Overlay { fill: solid(0, 160, 0) };
        session.document.create_object(Object { pos: [-5000.0, -5000.0], size: [10000.0, 10000.0], fill: solid(255, 0, 0), effects: vec![overlay], ..Object::default() });
        let target = session.document.create_object(Object { pos: [20000.0, 20000.0], size: [10.0, 10.0], fill: solid(255, 255, 255), ..Object::default() });
        session.selection.ids = vec![target];
        session.document.version += 1;
        target
    };
    frame(&mut shell, &ctx, Vec::new());
    shell.pick_tool("tool.eyedropper").unwrap();
    frame(&mut shell, &ctx, Vec::new());
    click(&mut shell, &ctx, pos2(760.0, 470.0));
    let session = shell.workspace.session().unwrap();
    let fill = session.document.get_object(target).unwrap().fill.clone();
    assert_eq!(fill, solid(0, 160, 0), "eyedropper picked {fill:?}");
}