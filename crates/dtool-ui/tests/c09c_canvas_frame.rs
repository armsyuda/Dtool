use dtool_core::document::Units;
use dtool_core::scene::{build_scene, Prim};
use dtool_core::view_state::ViewState;
use dtool_core::{CanvasFlags, Document, ObjectId, Session};
use dtool_ui::app_canvas::image_mesh;
use egui::{Color32, Mesh, Pos2, Rect};
use serde_json::json;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn framed(shape: &str) -> (Session, ObjectId) {
    let mut session = Session::new(Document::new([40.0, 40.0], Units::Px, 72.0, 1));
    let reply = session.run_named("frame.image.add", json!({"shape": shape, "pos": [0, 0], "size": [40.0, 40.0]})).unwrap();
    let id = ObjectId(reply["id"].as_u64().or_else(|| reply["object_id"].as_u64()).unwrap_or_else(|| session.selection.ids[0].0));
    let object = session.document.objects.iter_mut().find(|object| object.id == id).unwrap();
    object.raster_size = Some((2, 2));
    object.raster_data = Some([RED, GREEN, BLUE, WHITE].concat());
    session.run_named("frame.image.fit.set", json!({"object": id.0, "fit": "stretch"})).unwrap();
    (session, id)
}

fn canvas_mesh(session: &Session, id: ObjectId) -> (Mesh, Rect, ViewState) {
    let scene = build_scene(&session.document, &CanvasFlags::default());
    let quad = scene.prims.iter().find_map(|prim| match prim { Prim::Image { object, quad, .. } if *object == id => Some(*quad), _ => None }).unwrap();
    let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(400.0, 300.0));
    let view = ViewState::default();
    let mesh = image_mesh(&session.document, session.document.get_object(id).unwrap(), &quad, rect, &view, Color32::WHITE, egui::TextureId::Managed(7));
    (mesh, rect, view)
}

fn to_screen(rect: Rect, view: &ViewState, p: [f32; 2]) -> Pos2 {
    let v = dtool_core::doc_to_view(view, p);
    Pos2::new(rect.min.x + v[0], rect.min.y + v[1])
}

fn sample(mesh: &Mesh, at: Pos2) -> Option<[u8; 4]> {
    for tri in mesh.indices.chunks(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
        let d = (b.pos.y - c.pos.y) * (a.pos.x - c.pos.x) + (c.pos.x - b.pos.x) * (a.pos.y - c.pos.y);
        if d.abs() < 1e-9 {
            continue;
        }
        let wa = ((b.pos.y - c.pos.y) * (at.x - c.pos.x) + (c.pos.x - b.pos.x) * (at.y - c.pos.y)) / d;
        let wb = ((c.pos.y - a.pos.y) * (at.x - c.pos.x) + (a.pos.x - c.pos.x) * (at.y - c.pos.y)) / d;
        let wc = 1.0 - wa - wb;
        if wa >= 0.0 && wb >= 0.0 && wc >= 0.0 {
            let u = wa * a.uv.x + wb * b.uv.x + wc * c.uv.x;
            let v = wa * a.uv.y + wb * b.uv.y + wc * c.uv.y;
            let texel = [RED, GREEN, BLUE, WHITE];
            let x = ((u * 2.0) as usize).min(1);
            let y = ((v * 2.0) as usize).min(1);
            return Some(texel[y * 2 + x]);
        }
    }
    None
}

#[test]
fn c09c_live_canvas_clips_an_ellipse_frame_and_draws_rotated_content() {
    let (mut session, id) = framed("ellipse");
    let (mesh, rect, view) = canvas_mesh(&session, id);
    assert_eq!(mesh.texture_id, egui::TextureId::Managed(7));
    assert!(mesh.vertices.len() >= 32);
    assert_eq!(mesh.indices.len(), (mesh.vertices.len() - 2) * 3);
    for (x, y) in [(1.0, 1.0), (39.0, 1.0), (1.0, 39.0), (39.0, 39.0)] {
        assert_eq!(sample(&mesh, to_screen(rect, &view, [x, y])), None, "corner {x},{y}");
    }
    assert_eq!(sample(&mesh, to_screen(rect, &view, [12.0, 10.0])), Some(RED));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [28.0, 10.0])), Some(GREEN));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [12.0, 30.0])), Some(BLUE));

    session.run_named("frame.image.content.set", json!({"object": id.0, "rotation": 90.0})).unwrap();
    let (mesh, rect, view) = canvas_mesh(&session, id);
    for (x, y) in [(1.0, 1.0), (39.0, 39.0)] {
        assert_eq!(sample(&mesh, to_screen(rect, &view, [x, y])), None, "corner {x},{y}");
    }
    assert_eq!(sample(&mesh, to_screen(rect, &view, [12.0, 10.0])), Some(BLUE));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [28.0, 10.0])), Some(RED));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [28.0, 30.0])), Some(GREEN));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [12.0, 30.0])), Some(WHITE));
}

#[test]
fn c09c_live_canvas_applies_fit_anchor_and_crop_of_a_rect_frame() {
    let (mut session, id) = framed("rect");
    session.run_named("frame.image.fit.set", json!({"object": id.0, "fit": "min"})).unwrap();
    session.run_named("frame.image.content.set", json!({"object": id.0, "scale": [0.5, 0.5]})).unwrap();
    session.run_named("frame.image.anchor.set", json!({"object": id.0, "anchor": "top_left"})).unwrap();
    let (mesh, rect, view) = canvas_mesh(&session, id);
    assert_eq!(sample(&mesh, to_screen(rect, &view, [5.0, 5.0])), Some(RED));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [15.0, 15.0])), Some(WHITE));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [25.0, 5.0])), None);

    session.run_named("frame.image.content.set", json!({"object": id.0, "scale": [2.0, 2.0], "offset": [-20.0, -20.0]})).unwrap();
    let (mesh, rect, view) = canvas_mesh(&session, id);
    assert_eq!(sample(&mesh, to_screen(rect, &view, [5.0, 5.0])), Some(RED));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [35.0, 35.0])), Some(WHITE));
    assert_eq!(sample(&mesh, to_screen(rect, &view, [41.0, 20.0])), None);
    assert_eq!(sample(&mesh, to_screen(rect, &view, [20.0, -1.0])), None);
    for vertex in &mesh.vertices {
        assert!(vertex.uv.x >= 0.0 && vertex.uv.x <= 1.0 && vertex.uv.y >= 0.0 && vertex.uv.y <= 1.0);
    }
}