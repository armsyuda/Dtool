use dtool_core::document::{Document, Units};
use dtool_core::guides::GuideKind;
use dtool_core::{CanvasFlags, Session};
use dtool_ui::canvas_input::CanvasInput;
use dtool_ui::rulers::{guide_drag_active, Rulers};
use egui::{pos2, vec2, Event, Modifiers, PointerButton, Pos2, Rect, Sense};
use serde_json::json;

struct Rig {
    ctx: egui::Context,
    rulers: Rulers,
    input: CanvasInput,
    session: Session,
    owned: Vec<bool>,
}

fn area() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
}

fn inner() -> Rect {
    Rulers::canvas_rect(area(), true)
}

impl Rig {
    fn new() -> Self {
        let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
        session.has_document = true;
        session.run_named("ruler.show", json!({ "show": true })).unwrap();
        session.active_tool = "tool.shape.rectangle".to_string();
        let mut rig = Rig { ctx: egui::Context::default(), rulers: Rulers::new(), input: CanvasInput::new(), session, owned: Vec::new() };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        let raw = egui::RawInput { screen_rect: Some(area()), events, ..Default::default() };
        let Rig { ctx, rulers, input, session, owned } = self;
        let mut output = ctx.run_ui(raw, |ui| {
            let view = session.view.clone();
            rulers.show(ui, session, &view, area());
            let response = ui.allocate_rect(inner(), Sense::click_and_drag());
            let scene = dtool_core::scene::estimate_scene(&session.document, &CanvasFlags::default());
            input.handle(ui, &response, session, &scene);
            rulers.paint_visible_guides(ui, session, &view, inner());
            owned.push(guide_drag_active(ui.ctx()));
        });
        output.textures_delta.clear();
    }

    fn press(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() }]);
    }

    fn drag_to(&mut self, from: Pos2, to: Pos2) {
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            self.frame(vec![Event::PointerMoved(from + (to - from) * t)]);
        }
    }

    fn release(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() }]);
        self.frame(Vec::new());
    }

    fn escape(&mut self) {
        self.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() }]);
    }

    fn doc_to_screen(&self, doc: [f32; 2]) -> Pos2 {
        dtool_ui::rulers::doc_to_screen(&self.session.view, inner(), doc)
    }

    fn add_guide(&mut self, position: f32) {
        self.session.run_named("guides.add", json!({ "kind": GuideKind::Horizontal, "position": position })).unwrap();
    }

    fn positions(&self) -> Vec<f32> {
        self.session.guides.list().iter().map(|g| g.position).collect()
    }
}

#[test]
fn drag_from_ruler_previews_then_adds_one_undoable_guide() {
    let mut rig = Rig::new();
    let objects = rig.session.document.objects.len();
    let start = pos2(300.0, Rulers::thickness() * 0.5);
    let end = rig.doc_to_screen([300.0, 150.0]);
    rig.press(start);
    rig.drag_to(start, end);
    let drag = rig.rulers.drag.expect("ruler drag owns the gesture");
    assert!(!drag.over_ruler);
    assert!((drag.doc_point[1] - 150.0).abs() < 0.5, "{:?}", drag.doc_point);
    assert!(rig.owned.last().copied().unwrap_or(false));
    rig.release(end);
    assert!(rig.rulers.drag.is_none());
    let positions = rig.positions();
    assert_eq!(positions.len(), 1);
    assert!((positions[0] - 150.0).abs() < 0.5, "{positions:?}");
    assert_eq!(rig.session.document.objects.len(), objects);
    rig.session.run_named("edit.undo", json!({})).unwrap();
    assert!(rig.positions().is_empty());
}

#[test]
fn drag_existing_guide_on_canvas_moves_it_with_one_undo_and_canvas_tool_gets_nothing() {
    let mut rig = Rig::new();
    rig.add_guide(100.0);
    let objects = rig.session.document.objects.len();
    let start = rig.doc_to_screen([200.0, 100.0]);
    let end = rig.doc_to_screen([200.0, 160.0]);
    rig.press(start);
    assert!(rig.rulers.drag.is_some());
    rig.drag_to(start, end);
    assert!(rig.input.drag.is_none() && !rig.input.gesture_active());
    rig.release(end);
    let positions = rig.positions();
    assert_eq!(positions.len(), 1);
    assert!((positions[0] - 160.0).abs() < 0.5, "{positions:?}");
    assert_eq!(rig.session.document.objects.len(), objects);
    rig.session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(rig.positions(), vec![100.0]);
}

#[test]
fn escape_cancels_guide_drag_without_change() {
    let mut rig = Rig::new();
    rig.add_guide(100.0);
    let version = rig.session.document.version;
    let start = rig.doc_to_screen([200.0, 100.0]);
    let end = rig.doc_to_screen([200.0, 180.0]);
    rig.press(start);
    rig.drag_to(start, end);
    rig.escape();
    assert!(rig.rulers.drag.is_none());
    rig.release(end);
    assert_eq!(rig.positions(), vec![100.0]);
    assert_eq!(rig.session.document.version, version);
    assert_eq!(rig.session.document.objects.len(), 0);
}

#[test]
fn click_on_guide_without_moving_adds_no_undo_step() {
    let mut rig = Rig::new();
    rig.add_guide(100.0);
    let at = rig.doc_to_screen([200.0, 100.0]);
    rig.press(at);
    rig.release(at);
    assert_eq!(rig.positions(), vec![100.0]);
    rig.session.run_named("edit.undo", json!({})).unwrap();
    assert!(rig.positions().is_empty());
}

#[test]
fn canvas_drag_away_from_guides_still_reaches_the_tool() {
    let mut rig = Rig::new();
    rig.add_guide(100.0);
    let start = rig.doc_to_screen([50.0, 200.0]);
    let end = rig.doc_to_screen([150.0, 260.0]);
    rig.press(start);
    assert!(rig.rulers.drag.is_none());
    rig.drag_to(start, end);
    rig.release(end);
    assert_eq!(rig.session.document.objects.len(), 1);
    assert_eq!(rig.positions(), vec![100.0]);
}
