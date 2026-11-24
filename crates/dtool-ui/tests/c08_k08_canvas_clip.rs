#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{ClipRole, Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::scene::Prim;
use dtool_ui::app_canvas::AppCanvas;

fn session() -> (Session, ObjectId, ObjectId) {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let parent = doc.create_object(Object { pos: [0.0, 0.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    let child = doc.create_object(Object { pos: [20.0, 20.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(0, 0, 255, 255) }, ..Object::default() });
    (Session::new(doc), parent, child)
}

fn paint(canvas: &mut AppCanvas, session: &Session) {
    let ctx = egui::Context::default();
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 400.0))), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| {
        let rect = ui.max_rect();
        canvas.paint(ui, rect, session);
    });
    output.textures_delta.clear();
}

fn fill_max_x(canvas: &AppCanvas, id: ObjectId) -> Option<f32> {
    canvas.scene().prims.iter().filter_map(|prim| match prim {
        Prim::Fill { object, contours, .. } if *object == id => contours.iter().flatten().map(|p| p[0]).reduce(f32::max),
        _ => None,
    }).reduce(f32::max)
}

#[test]
fn on_screen_canvas_clips_clip_child_and_hides_mask_path() {
    let _home = test_home::enter("c08_k08_canvas_clip");
    let (mut session, parent, child) = session();
    let mut canvas = AppCanvas::new();
    paint(&mut canvas, &session);
    assert!(fill_max_x(&canvas, child).unwrap() > 55.0);
    let child_obj = session.document.find_object_mut(child.0).unwrap();
    child_obj.parent_id = Some(parent);
    child_obj.clip_role = ClipRole::Clip;
    session.document.version += 1;
    paint(&mut canvas, &session);
    assert!(fill_max_x(&canvas, child).unwrap() <= 40.5, "clip child limited to parent on screen");
    let mask = session.document.create_object(Object { pos: [0.0, 0.0], size: [10.0, 40.0], parent_id: Some(parent), clip_role: ClipRole::Mask, fill: Paint::Solid { colour: Colour::new(0, 255, 0, 255) }, ..Object::default() });
    session.document.version += 1;
    paint(&mut canvas, &session);
    assert!(fill_max_x(&canvas, parent).unwrap() <= 10.5, "parent limited to its vector mask on screen");
    assert!(fill_max_x(&canvas, child).is_none(), "child is outside the mask");
    assert!(fill_max_x(&canvas, mask).is_none(), "mask path not drawn");
    session.view.canvas_flags.mask_view = Some(parent);
    let version = session.document.version;
    paint(&mut canvas, &session);
    let colours: Vec<Paint> = canvas.scene().prims.iter().filter_map(|prim| match prim { Prim::Fill { paint, .. } => Some(paint.clone()), _ => None }).collect();
    assert!(colours.contains(&Paint::Solid { colour: Colour::new(255, 255, 255, 255) }));
    assert!(!colours.contains(&Paint::Solid { colour: Colour::new(255, 0, 0, 255) }));
    assert_eq!(session.document.version, version);
}