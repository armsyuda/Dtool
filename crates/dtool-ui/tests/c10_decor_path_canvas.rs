use dtool_core::command::Session;
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow, PathTextData};
use dtool_core::paint::{Colour, Paint};
use dtool_ui::app_canvas::AppCanvas;
use egui::{Color32, Context, Pos2, RawInput, Rect, Shape};
use serde_json::json;

const FROM: [f64; 2] = [20.0, 150.0];
const TO: [f64; 2] = [180.0, 80.0];

fn session() -> Session {
    let mut doc = Document::new([200.0, 160.0], Units::Px, 72.0, 1);
    let red = serde_json::to_value(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }).unwrap();
    doc.stories.push(Story { id: StoryId(1), text: "Hello world".into(), ranges: vec![json!({"start": 0, "end": 11, "family": "Arial", "style": "Regular", "size_pt": 18.0, "underline": {"on": true, "colour": red}})], ..Story::default() });
    doc.objects.push(Object { id: ObjectId(1), pos: [0.0, 0.0], size: [200.0, 160.0], kind: ObjectKind::Curve(Curve { contour: Contour { points: vec![PathPoint::sharp(PointId(1), FROM), PathPoint::sharp(PointId(2), TO)], closed: false }, holes: Vec::new(), source_object: None }), ..Object::default() });
    doc.objects.push(Object { id: ObjectId(2), pos: [20.0, 150.0], size: [160.0, 20.0], fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) }, kind: ObjectKind::PathText(PathTextData { path: ObjectId(1), story: StoryId(1), flows: vec![PathFlow::default()], source_paint: None }), ..Object::default() });
    doc.next_object_id = 3;
    Session::new(doc)
}

fn red_rects(session: &Session) -> Vec<Rect> {
    let ctx = Context::default();
    let area = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0));
    let mut canvas = AppCanvas::new();
    let mut out = ctx.run_ui(RawInput { screen_rect: Some(area), ..Default::default() }, |ui| canvas.paint(ui, area, session));
    out.textures_delta.clear();
    let mut found = Vec::new();
    fn walk(shape: &Shape, found: &mut Vec<Rect>) {
        match shape {
            Shape::Vec(items) => items.iter().for_each(|s| walk(s, found)),
            Shape::Rect(r) if r.fill == Color32::from_rgb(255, 0, 0) => found.push(r.rect),
            _ => {}
        }
    }
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut found));
    found
}

#[test]
fn c10_live_canvas_path_text_underline_follows_the_path() {
    let session = session();
    let rects = red_rects(&session);
    assert!(rects.len() >= 5, "{rects:?}");
    let view = &session.view;
    let to_doc = |p: Pos2| dtool_core::view_to_doc(view, [p.x, p.y]);
    let centres: Vec<[f32; 2]> = rects.iter().map(|r| to_doc(r.center())).collect();
    let (a, b) = ([FROM[0] as f32, FROM[1] as f32], [TO[0] as f32, TO[1] as f32]);
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    for c in &centres {
        let off = ((c[0] - a[0]) * d[1] - (c[1] - a[1]) * d[0]).abs() / len;
        assert!(off < 12.0, "{c:?} is {off} from the path");
    }
    let left = centres.iter().filter(|c| c[0] < 100.0).map(|c| c[1]).fold(f32::MIN, f32::max);
    let right = centres.iter().filter(|c| c[0] > 100.0).map(|c| c[1]).fold(f32::MAX, f32::min);
    assert!(left > right, "underline does not climb with the path: {left} {right}");
}