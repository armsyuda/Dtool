use dtool_core::curve::NodeKind;
use dtool_core::document::{Document, Units};
use dtool_core::{CanvasFlags, Session};
use dtool_ui::canvas_curves::{Pen, PenMode, PenStep};
use dtool_ui::canvas_input::{tool_kind, CanvasInput, ToolKind};
use egui::{vec2, Event, Key, Modifiers, PointerButton, Pos2, Rect, Sense};
use serde_json::json;

struct Rig {
    ctx: egui::Context,
    input: CanvasInput,
    session: Session,
}

fn area() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
}

impl Rig {
    fn new(tool: &str) -> Self {
        let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
        session.has_document = true;
        session.active_tool = tool.to_string();
        let mut rig = Rig { ctx: egui::Context::default(), input: CanvasInput::new(), session };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        let raw = egui::RawInput { screen_rect: Some(area()), events, ..Default::default() };
        let Rig { ctx, input, session } = self;
        let mut output = ctx.run_ui(raw, |ui| {
            let response = ui.allocate_rect(area(), Sense::click_and_drag());
            let scene = dtool_core::scene::estimate_scene(&session.document, &CanvasFlags::default());
            input.handle(ui, &response, session, &scene);
        });
        output.textures_delta.clear();
    }

    fn at(&self, doc: [f32; 2]) -> Pos2 {
        let v = dtool_core::doc_to_view(&self.session.view, doc);
        Pos2::new(v[0], v[1])
    }

    fn click(&mut self, doc: [f32; 2]) {
        let at = self.at(doc);
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() }]);
        self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() }]);
        for _ in 0..3 {
            self.frame(Vec::new());
        }
    }

    fn drag(&mut self, from: [f32; 2], to: [f32; 2]) {
        let (a, b) = (self.at(from), self.at(to));
        self.frame(vec![Event::PointerMoved(a)]);
        self.frame(vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() }]);
        for step in 1..=6 {
            self.frame(vec![Event::PointerMoved(a + (b - a) * (step as f32 / 6.0))]);
        }
        self.frame(vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() }]);
        self.frame(Vec::new());
    }

    fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() }]);
        self.frame(Vec::new());
    }

    fn objects(&self) -> usize {
        self.session.document.objects.len()
    }
}

#[test]
fn pen_escape_mid_path_cancels_and_leaves_document_unchanged() {
    let mut rig = Rig::new("tool.pen");
    rig.click([50.0, 50.0]);
    rig.click([150.0, 50.0]);
    rig.click([150.0, 150.0]);
    assert_eq!(rig.input.pen.nodes.len(), 3);
    rig.key(Key::Escape);
    assert!(!rig.input.pen.active());
    assert_eq!(rig.objects(), 0);
    assert!(!rig.session.undo.can_undo());
}

#[test]
fn pen_enter_commits_open_path_in_one_undo() {
    let mut rig = Rig::new("tool.pen");
    rig.click([50.0, 50.0]);
    rig.click([150.0, 50.0]);
    rig.key(Key::Enter);
    assert!(!rig.input.pen.active());
    assert_eq!(rig.objects(), 1);
    assert_eq!(rig.session.undo.applied_steps(), 1);
    rig.session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(rig.objects(), 0);
}

#[test]
fn pencil_escape_mid_stroke_leaves_document_unchanged() {
    let mut rig = Rig::new("tool.pen.pencil");
    let (a, b) = (rig.at([20.0, 20.0]), rig.at([200.0, 120.0]));
    rig.frame(vec![Event::PointerMoved(a)]);
    rig.frame(vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() }]);
    for step in 1..=6 {
        rig.frame(vec![Event::PointerMoved(a + (b - a) * (step as f32 / 6.0))]);
    }
    rig.key(Key::Escape);
    rig.frame(vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() }]);
    rig.frame(Vec::new());
    assert_eq!(rig.objects(), 0);
    assert!(!rig.session.undo.can_undo());
}

#[test]
fn smart_tool_is_live_and_builds_smart_interior_nodes() {
    assert_eq!(tool_kind("tool.pen.smart"), ToolKind::Pen { polyline: false });
    assert_eq!(PenMode::for_tool("tool.pen.smart", None), PenMode::Smart);
    let mut rig = Rig::new("tool.pen.smart");
    rig.drag([50.0, 50.0], [80.0, 90.0]);
    rig.click([150.0, 80.0]);
    rig.click([250.0, 50.0]);
    rig.key(Key::Enter);
    assert_eq!(rig.objects(), 1);
    let object = &rig.session.document.objects[0];
    let contour = &object.curve().unwrap().contour;
    assert_eq!(contour.points.len(), 3);
    assert_eq!(contour.points[1].kind, NodeKind::Smart);
    assert!(contour.points[1].handle_in.is_some() && contour.points[1].handle_out.is_some());
    assert_eq!(contour.points[0].handle_out, None);
}

#[test]
fn polygon_mode_option_ignores_drag_handles() {
    let mut rig = Rig::new("tool.pen");
    rig.session.run_named("ui.context_option.set", json!({ "tool": "tool.pen", "id": "pen.mode", "value": "polygon" })).unwrap();
    rig.drag([50.0, 50.0], [90.0, 90.0]);
    assert_eq!(rig.input.pen.mode, PenMode::Polygon);
    assert_eq!(rig.input.pen.nodes.len(), 1);
    assert!(rig.input.pen.nodes[0].handle_out.is_none());
}

#[test]
fn line_mode_commits_after_second_point() {
    let mut rig = Rig::new("tool.pen");
    rig.session.run_named("ui.context_option.set", json!({ "tool": "tool.pen", "id": "pen.mode", "value": "line" })).unwrap();
    rig.click([50.0, 50.0]);
    assert_eq!(rig.objects(), 0);
    rig.click([150.0, 80.0]);
    assert_eq!(rig.objects(), 1);
    assert!(!rig.input.pen.active());
    assert_eq!(rig.session.document.objects[0].curve().unwrap().contour.points.len(), 2);
}

#[test]
fn rubber_band_option_is_read() {
    let mut rig = Rig::new("tool.pen");
    rig.click([50.0, 50.0]);
    assert!(!rig.input.pen.rubber_band);
    rig.session.run_named("ui.context_option.set", json!({ "tool": "tool.pen", "id": "pen.rubber_band", "value": true })).unwrap();
    rig.frame(Vec::new());
    assert!(rig.input.pen.rubber_band);
}

#[test]
fn shift_snaps_new_handle_to_45_degrees() {
    let mut pen = Pen { shift: true, ..Pen::default() };
    assert_eq!(pen.press([0.0, 0.0], 3.0), PenStep::Added);
    pen.drag([10.0, 9.0]);
    let out = pen.nodes[0].handle_out.unwrap();
    assert!((out[0] - out[1]).abs() < 1e-4, "{out:?}");
    pen.release();
    pen.press([100.0, 7.0], 3.0);
    assert!((pen.nodes[1].pos[1]).abs() < 1e-4, "{:?}", pen.nodes[1].pos);
}

#[test]
fn space_moves_the_new_point_while_dragging() {
    let mut pen = Pen::default();
    pen.press([0.0, 0.0], 3.0);
    pen.drag([20.0, 0.0]);
    pen.space = true;
    pen.drag([30.0, 10.0]);
    let node = &pen.nodes[0];
    assert_eq!(node.pos, [10.0, 10.0]);
    assert_eq!(node.handle_out, Some([30.0, 10.0]));
    pen.space = false;
    pen.drag([40.0, 10.0]);
    assert_eq!(pen.nodes[0].pos, [10.0, 10.0]);
    assert_eq!(pen.nodes[0].handle_out, Some([40.0, 10.0]));
}

#[test]
fn close_action_runs_path_close_on_selected_curve_with_one_undo() {
    let mut s = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    s.has_document = true;
    s.active_tool = "tool.pen".to_string();
    s.run_named("curve.add", json!({ "points": [[10.0, 10.0], [100.0, 10.0], [100.0, 80.0]] })).unwrap();
    let steps = s.undo.applied_steps();
    s.run_named("ui.context_option.set", json!({ "tool": "tool.pen", "id": "pen.close_curve", "value": null })).unwrap();
    assert!(s.document.objects[0].curve().unwrap().contour.closed);
    assert_eq!(s.undo.applied_steps(), steps + 1);
    s.run_named("edit.undo", json!({})).unwrap();
    assert!(!s.document.objects[0].curve().unwrap().contour.closed);
}

#[test]
fn close_pen_closes_the_draft() {
    let mut rig = Rig::new("tool.pen");
    rig.click([50.0, 50.0]);
    rig.click([150.0, 50.0]);
    rig.click([150.0, 150.0]);
    let Rig { input, session, .. } = &mut rig;
    assert!(input.close_pen(session));
    assert!(session.document.objects[0].curve().unwrap().contour.closed);
    assert_eq!(session.undo.applied_steps(), 1);
}