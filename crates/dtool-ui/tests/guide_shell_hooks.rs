#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::{Document, Units};
use dtool_core::guides::GuideKind;
use dtool_core::Session;
use dtool_ui::rulers::{guide_drag_active, Rulers};
use dtool_ui::shell::AppShell;
use egui::{pos2, vec2, Event, Modifiers, PointerButton, Pos2, Rect};
use serde_json::json;

fn area() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
}

fn run(ctx: &egui::Context, rulers: &mut Rulers, session: &mut Session, events: Vec<Event>) -> bool {
    let raw = egui::RawInput { screen_rect: Some(area()), events, ..Default::default() };
    let mut owned = false;
    let mut output = ctx.run_ui(raw, |ui| {
        let view = session.view.clone();
        rulers.canvas_guides(ui, session, &view, area());
        rulers.paint_visible_guides(ui, session, &view, area());
        owned = guide_drag_active(ui.ctx());
    });
    output.textures_delta.clear();
    owned
}

#[test]
fn hidden_rulers_still_drag_guides() {
    let _home = test_home::enter("hidden_rulers_still_drag_guides");
    let ctx = egui::Context::default();
    let mut rulers = Rulers::new();
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    session.has_document = true;
    session.run_named("ruler.show", json!({ "show": false })).unwrap();
    session.run_named("guides.add", json!({ "kind": GuideKind::Horizontal, "position": 100.0 })).unwrap();
    run(&ctx, &mut rulers, &mut session, Vec::new());
    let start = dtool_ui::rulers::doc_to_screen(&session.view, area(), [200.0, 100.0]);
    let end = dtool_ui::rulers::doc_to_screen(&session.view, area(), [200.0, 170.0]);
    run(&ctx, &mut rulers, &mut session, vec![Event::PointerMoved(start)]);
    run(&ctx, &mut rulers, &mut session, vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() }]);
    let mut owned = false;
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        owned |= run(&ctx, &mut rulers, &mut session, vec![Event::PointerMoved(start + (end - start) * t)]);
    }
    assert!(owned);
    run(&ctx, &mut rulers, &mut session, vec![Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() }]);
    run(&ctx, &mut rulers, &mut session, Vec::new());
    let positions: Vec<f32> = session.guides.list().iter().map(|g| g.position).collect();
    assert_eq!(positions.len(), 1);
    assert!((positions[0] - 170.0).abs() < 0.5, "{positions:?}");
}

#[test]
fn shell_canvas_frame_with_hidden_rulers() {
    let _home = test_home::enter("shell_canvas_frame_with_hidden_rulers");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("guides");
    shell.workspace.activate(id).unwrap();
    shell.workspace.session_mut().unwrap().run_named("ruler.show", json!({ "show": false })).unwrap();
    for _ in 0..2 {
        let raw = egui::RawInput { screen_rect: Some(area()), events: vec![Event::PointerMoved(pos2(400.0, 300.0))], ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| shell.navigation_canvas(ui));
        output.textures_delta.clear();
    }
    assert!(!guide_drag_active(&ctx));
}
