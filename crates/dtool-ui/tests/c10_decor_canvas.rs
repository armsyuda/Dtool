use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_core::view_state::ViewState;
use dtool_tools::dtool_text::decorations::{object_decorations, DecorationKind};
use dtool_ui::app_canvas::{decoration_shape, AppCanvas};
use egui::{Color32, Context, Pos2, RawInput, Rect, Shape};
use serde_json::{json, Value};

const RED: Color32 = Color32::from_rgb(255, 0, 0);
const GREEN: Color32 = Color32::from_rgb(0, 255, 0);
const BLUE: Color32 = Color32::from_rgb(0, 0, 255);

fn solid(r: u8, g: u8, b: u8) -> Value {
    serde_json::to_value(Paint::Solid { colour: Colour::new(r, g, b, 255) }).unwrap()
}

fn base(start: usize, end: usize) -> Value {
    json!({"start": start, "end": end, "family": "Arial", "style": "Regular", "size_pt": 24.0})
}

fn decorated() -> Vec<Value> {
    let mut first = base(0, 5);
    first["underline"] = json!({"on": true, "colour": solid(255, 0, 0)});
    first["background"] = solid(0, 255, 0);
    let mut second = base(5, 11);
    second["strike"] = json!({"on": true, "colour": solid(0, 0, 255)});
    vec![first, second]
}

fn session(kind: fn(StoryId) -> ObjectKind, ranges: Vec<Value>) -> (Session, ObjectId) {
    dtool_tools::frame_text::install_flow_ranges();
    dtool_tools::frame_text::install_canvas_text();
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    let story = StoryId(91);
    session.document.stories.push(Story { id: story, text: "hello world".to_string(), ranges, specials: Vec::new(), paras: Vec::new() });
    let id = session.document.create_object(Object { kind: kind(story), pos: [40.0, 50.0], size: [300.0, 100.0], ..Object::default() });
    (session, id)
}

fn art(story: StoryId) -> ObjectKind {
    ObjectKind::ArtText { story }
}

fn frame(story: StoryId) -> ObjectKind {
    ObjectKind::FrameText { story }
}

fn flatten(shape: &Shape, out: &mut Vec<Shape>) {
    match shape {
        Shape::Vec(items) => items.iter().for_each(|s| flatten(s, out)),
        other => out.push(other.clone()),
    }
}

fn painted(session: &Session, canvas: &mut AppCanvas) -> Vec<Shape> {
    let ctx = Context::default();
    let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0));
    let mut out = ctx.run_ui(RawInput { screen_rect: Some(area), ..Default::default() }, |ui| canvas.paint(ui, area, session));
    out.textures_delta.clear();
    let mut shapes = Vec::new();
    out.shapes.iter().for_each(|clipped| flatten(&clipped.shape, &mut shapes));
    shapes
}

fn rect_index(shapes: &[Shape], colour: Color32) -> Option<usize> {
    shapes.iter().position(|s| matches!(s, Shape::Rect(r) if r.fill == colour))
}

fn text_index(shapes: &[Shape]) -> Option<usize> {
    shapes.iter().position(|s| matches!(s, Shape::Text(_)) || matches!(s, Shape::Mesh(m) if m.texture_id != egui::TextureId::default()))
}

#[test]
fn c10_decor_provider_places_rects_in_document_space() {
    for kind in [art as fn(StoryId) -> ObjectKind, frame] {
        let (session, id) = session(kind, decorated());
        let rects = object_decorations(&session.document, id);
        let of = |k: DecorationKind| rects.iter().filter(|r| r.kind == k).cloned().collect::<Vec<_>>();
        let (under, strike, back) = (of(DecorationKind::Underline), of(DecorationKind::Strike), of(DecorationKind::Background));
        assert_eq!((under.len(), strike.len(), back.len()), (1, 1, 1), "{rects:?}");
        assert_eq!(rects[0].kind, DecorationKind::Background);
        assert!(under[0].rect[0] >= 40.0 && under[0].rect[1] > 50.0 && under[0].rect[1] < 150.0, "{under:?}");
        assert!(strike[0].rect[0] >= under[0].rect[0] + under[0].rect[2] - 0.5, "{strike:?} {under:?}");
        assert!(strike[0].rect[1] < under[0].rect[1]);
        assert!(back[0].rect[1] < strike[0].rect[1] && back[0].rect[1] + back[0].rect[3] > under[0].rect[1]);
        assert_eq!(under[0].paint, Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
    }
}

#[test]
fn c10_decor_plain_text_has_no_rects() {
    let (session, id) = session(art, vec![base(0, 11)]);
    assert!(object_decorations(&session.document, id).is_empty());
}

#[test]
fn c10_decor_canvas_paints_background_under_and_lines_over_text() {
    for kind in [art as fn(StoryId) -> ObjectKind, frame] {
        let (mut session, _) = session(kind, decorated());
        let mut canvas = AppCanvas::new();
        let shapes = painted(&session, &mut canvas);
        let text = text_index(&shapes).expect("text painted");
        let back = rect_index(&shapes, GREEN).expect("background painted");
        let under = rect_index(&shapes, RED).expect("underline painted");
        let strike = rect_index(&shapes, BLUE).expect("strike painted");
        assert!(back < text && text < under && text < strike, "{back} {text} {under} {strike}");
        session.document.stories[0].ranges = vec![base(0, 11)];
        session.document.version += 1;
        let shapes = painted(&session, &mut canvas);
        assert!(text_index(&shapes).is_some());
        assert!(rect_index(&shapes, RED).is_none() && rect_index(&shapes, GREEN).is_none() && rect_index(&shapes, BLUE).is_none());
    }
}

#[test]
fn c10_decor_shape_follows_view_and_keeps_thin_lines_visible() {
    let (session, id) = session(art, decorated());
    let rects = object_decorations(&session.document, id);
    let under = rects.iter().find(|r| r.kind == DecorationKind::Underline).unwrap();
    let area = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(500.0, 400.0));
    let view = ViewState::default();
    let (shape, colour) = decoration_shape(area, &view, under, 1.0).unwrap();
    let at = dtool_core::doc_to_view(&view, [under.rect[0], under.rect[1]]);
    assert!((shape.min.x - (10.0 + at[0])).abs() < 0.01);
    assert_eq!(colour, RED);
    let far = ViewState { zoom: 0.01, ..ViewState::default() };
    let (thin, _) = decoration_shape(area, &far, under, 1.0).unwrap();
    assert!(thin.height() >= 1.0);
    let half = decoration_shape(area, &view, under, 0.5).unwrap().1;
    assert!(half.a() < 255);
}