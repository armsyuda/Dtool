#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::object::ObjectId;
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint, Transparency};
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;
use egui::{Color32, Pos2, Rect, Shape};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn flatten(shapes: Vec<Shape>, out: &mut Vec<Shape>) {
    for shape in shapes {
        match shape {
            Shape::Vec(inner) => flatten(inner, out),
            other => out.push(other),
        }
    }
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) -> Vec<Shape> {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
    let mut shapes = Vec::new();
    flatten(out.shapes.into_iter().map(|clipped| clipped.shape).collect(), &mut shapes);
    shapes
}

fn polygon_filled(shapes: &[Shape], colour: Color32) -> bool {
    shapes.iter().any(|shape| matches!(shape, Shape::Path(path) if path.fill == colour))
}

fn rect_filled(shapes: &[Shape], colour: Color32) -> usize {
    shapes.iter().filter(|shape| matches!(shape, Shape::Rect(rect) if rect.fill == colour)).count()
}

fn textured_meshes(shapes: &[Shape]) -> usize {
    shapes.iter().filter(|shape| matches!(shape, Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default())).count()
}

fn create(shell: &mut AppShell, name: &str, x: f32, colour: Colour) -> ObjectId {
    run(shell, "object.create", json!({ "name": name, "pos": [x, 40.0], "size": [160.0, 120.0] }));
    let session = shell.workspace.session_mut().unwrap();
    let object = session.document.objects.iter_mut().find(|o| o.name == name).unwrap();
    object.fill = Paint::Solid { colour };
    object.id
}

#[test]
fn rt2_solid_fill_with_transparency_gradient_skips_the_solid_fast_path() {
    let _home = test_home::enter("c10_rt_hp2_canvas_rt");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("rt2");
    shell.workspace.activate(doc).unwrap();
    let red = Colour::new(220, 20, 20, 255);
    let blue = Colour::new(20, 20, 220, 255);
    let graded = create(&mut shell, "graded", 40.0, red);
    create(&mut shell, "plain", 260.0, blue);
    let stops = vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 0) }, GradientStop { offset: 1.0, colour: Colour::new(0, 0, 0, 255) }];
    let geom = GradientGeom { start: [0.0, 0.0], end: [160.0, 0.0], ratio: 1.0, ..GradientGeom::default() };
    shell.workspace.session_mut().unwrap().document.find_object_mut(graded.0).unwrap().transparency = Transparency::Gradient { kind: GradientKind::Linear, stops, geom };
    for _ in 0..2 { frame(&mut shell, &ctx); }
    let shapes = frame(&mut shell, &ctx);
    let solid = |c: Colour| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
    assert!(polygon_filled(&shapes, solid(blue)), "Transparency::None keeps the solid fast path");
    assert!(!polygon_filled(&shapes, solid(red)), "a transparency gradient is not drawn as a flat solid");
    assert!(textured_meshes(&shapes) >= 1, "the graded fill is drawn from a rasterized texture");

    shell.workspace.session_mut().unwrap().document.find_object_mut(graded.0).unwrap().transparency = Transparency::None;
    let shapes = frame(&mut shell, &ctx);
    assert!(polygon_filled(&shapes, solid(red)), "clearing the gradient returns to the solid path");
}

#[test]
fn hp2_missing_char_highlight_paints_only_when_the_view_flag_is_on() {
    let _home = test_home::enter("c10_rt_hp2_canvas_hp");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("hp2");
    shell.workspace.activate(doc).unwrap();
    run(&mut shell, "text.art.add", json!({ "pos": [60.0, 80.0], "text": "A\u{10FFFD}B" }));
    let mark = dtool_design::color("error").gamma_multiply(0.45);
    for _ in 0..2 { frame(&mut shell, &ctx); }
    assert_eq!(rect_filled(&frame(&mut shell, &ctx), mark), 0, "no highlight while the flag is off");
    run(&mut shell, "view.flags.set", json!({ "highlight_missing_chars": true }));
    let shapes = frame(&mut shell, &ctx);
    assert_eq!(rect_filled(&shapes, mark), 1, "one translucent box over the one missing character");
    run(&mut shell, "view.flags.set", json!({ "highlight_missing_chars": false }));
    assert_eq!(rect_filled(&frame(&mut shell, &ctx), mark), 0);
}
