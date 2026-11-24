#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::object::{ClipRole, ObjectId};
use dtool_core::{Command, Scope};
use dtool_ui::panels::layers::{clip_badge, mask_view_command, thumb_drop_command};
use dtool_ui::shell::AppShell;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

#[derive(Default)]
struct Frame {
    texts: Vec<(String, Rect)>,
    tiles: Vec<Rect>,
    images: Vec<Rect>,
}

fn rendered(shapes: &[egui::Shape], found: &mut Frame) {
    for shape in shapes {
        match shape {
            egui::Shape::Text(text) if text.pos.x.is_finite() && text.pos.y.is_finite() => {
                found.texts.push((text.galley.text().to_string(), Rect::from_min_size(text.pos, text.galley.size())));
            }
            egui::Shape::Rect(tile) if tile.fill_texture_id() != egui::TextureId::default() => {
                found.images.push(tile.rect);
            }
            egui::Shape::Rect(tile) if tile.fill.a() != 0 => found.tiles.push(tile.rect),
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => found.images.push(mesh.calc_bounds()),
            egui::Shape::Vec(inner) => rendered(inner, found),
            _ => {}
        }
    }
}

fn frame(shell: &mut AppShell, ctx: &egui::Context, mut events: Vec<Event>, modifiers: Modifiers) -> Frame {
    events.insert(0, Event::ModifiersChanged(modifiers));
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
    let shapes: Vec<egui::Shape> = out.shapes.into_iter().map(|clipped| clipped.shape).collect();
    let mut found = Frame::default();
    rendered(&shapes, &mut found);
    found
}

fn click(shell: &mut AppShell, ctx: &egui::Context, at: Pos2, modifiers: Modifiers) {
    frame(shell, ctx, vec![Event::PointerMoved(at)], modifiers);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers }], modifiers);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers }], modifiers);
    frame(shell, ctx, Vec::new(), modifiers);
}

fn drag(shell: &mut AppShell, ctx: &egui::Context, from: Pos2, to: Pos2) {
    let none = Modifiers::default();
    frame(shell, ctx, vec![Event::PointerMoved(from)], none);
    frame(shell, ctx, vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: none }], none);
    for step in 1..=8 {
        let t = step as f32 / 8.0;
        frame(shell, ctx, vec![Event::PointerMoved(from + (to - from) * t)], none);
    }
    frame(shell, ctx, vec![Event::PointerMoved(to)], none);
    frame(shell, ctx, vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: none }], none);
    frame(shell, ctx, Vec::new(), none);
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

fn row_rect(found: &Frame, name: &str) -> Rect {
    found.texts.iter().find(|(text, _)| text == name).map(|(_, rect)| *rect)
        .unwrap_or_else(|| panic!("missing layers row {name}"))
}

fn thumbnail_rect(found: &Frame, row: Rect) -> Rect {
    found.tiles.iter().copied()
        .filter(|rect| (0.0..=6.0).contains(&(row.left() - rect.right()))
            && row.y_range().contains(rect.center().y)
            && (20.0..=28.0).contains(&rect.width()) && (20.0..=24.0).contains(&rect.height()))
        .max_by(|a, b| a.right().total_cmp(&b.right()))
        .unwrap_or_else(|| panic!("missing thumbnail tile next to {row:?}"))
}

fn create(shell: &mut AppShell, name: &str, x: f32) -> u64 {
    run(shell, "object.create", json!({ "name": name, "pos": [x, 20.0], "size": [40.0, 40.0] }));
    shell.workspace.session().unwrap().document.objects.iter().find(|o| o.name == name).unwrap().id.0
}

#[test]
fn k08_drop_on_thumbnail_masks_and_alt_click_toggles_mask_view() {
    let _home = test_home::enter("c08_k08_layers_shell");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("k08 layers");
    shell.workspace.activate(doc).unwrap();
    let target = create(&mut shell, "k08target", 20.0);
    let shape = create(&mut shell, "k08shape", 200.0);
    let found = frame(&mut shell, &ctx, Vec::new(), Modifiers::default());
    let target_row = row_rect(&found, "k08target");
    let shape_row = row_rect(&found, "k08shape");
    let target_thumb = thumbnail_rect(&found, target_row).center();

    let before = undo_steps(&mut shell);
    drag(&mut shell, &ctx, shape_row.center(), target_thumb);
    let session = shell.workspace.session().unwrap();
    let dropped = session.document.get_object(ObjectId(shape)).unwrap();
    assert_eq!(dropped.clip_role, ClipRole::Mask, "a shape dropped on a thumbnail becomes a vector mask");
    assert_eq!(dropped.parent_id, Some(ObjectId(target)));
    assert_eq!(undo_steps(&mut shell), before + 1);

    let found = frame(&mut shell, &ctx, Vec::new(), Modifiers::default());
    let mask_thumb = thumbnail_rect(&found, row_rect(&found, "k08shape"));
    assert!(found.images.iter().any(|rect| rect.width() == 12.0 && rect.height() == 12.0
        && rect.intersects(mask_thumb) && rect.center().x > mask_thumb.center().x
        && rect.center().y > mask_thumb.center().y), "mask badge is drawn over the thumbnail");
    let target_row = row_rect(&found, "k08target");
    let target_thumb = thumbnail_rect(&found, target_row).center();
    let steps = undo_steps(&mut shell);
    click(&mut shell, &ctx, target_thumb, Modifiers::ALT);
    assert_eq!(shell.workspace.session().unwrap().view.canvas_flags.mask_view, Some(ObjectId(target)));
    click(&mut shell, &ctx, target_thumb, Modifiers::ALT);
    assert_eq!(shell.workspace.session().unwrap().view.canvas_flags.mask_view, None, "alt-click again clears mask view");
    assert_eq!(undo_steps(&mut shell), steps, "mask view adds no undo step");

    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(shell.workspace.session().unwrap().document.get_object(ObjectId(shape)).unwrap().clip_role, ClipRole::None);
}

#[test]
fn k08_non_vector_drop_clips_and_mask_view_command_toggles() {
    let _home = test_home::enter("c08_k08_layers_cmd");
    let mut shell = AppShell::new();
    let doc = shell.workspace.new_document("k08 cmd");
    shell.workspace.activate(doc).unwrap();
    let target = create(&mut shell, "base", 0.0);
    let text = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "clip me" }))["id"].as_u64().unwrap();
    let session = shell.workspace.session().unwrap();
    let command = thumb_drop_command(&session.document, ObjectId(text), ObjectId(target));
    assert_eq!(command.name, "layer.clip.set");
    shell.workspace.run(Scope::active(), command).1.expect("clip set");
    let session = shell.workspace.session().unwrap();
    assert_eq!(session.document.get_object(ObjectId(text)).unwrap().clip_role, ClipRole::Clip);
    assert_eq!(clip_badge(&session.document, ObjectId(text)), "◧");
    let on = mask_view_command(session, ObjectId(target));
    assert_eq!(on.params["id"], json!(target));
    assert!(shell.workspace.run(Scope::active(), on).1.is_err(), "clipping alone has no mask to view");
    shell.workspace.session_mut().unwrap().view.canvas_flags.mask_view = Some(ObjectId(target));
    let off = mask_view_command(shell.workspace.session().unwrap(), ObjectId(target));
    assert_eq!(off.params["id"], Value::Null);
}
