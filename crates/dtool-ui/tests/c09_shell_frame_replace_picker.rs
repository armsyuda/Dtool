#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::picture_frame;
use dtool_ui::dialogs::FileDialogMode;
use dtool_ui::picture_frame_bar::FrameBarEdit;
use dtool_ui::shell::AppShell;
use egui::{pos2, Event, PointerButton, Pos2, Rect};

fn texts(shapes: &[egui::Shape], found: &mut Vec<(String, Rect)>) {
    for shape in shapes {
        match shape {
            egui::Shape::Text(text) if text.pos.x.is_finite() && text.pos.y.is_finite() => found.push((text.galley.text().to_string(), Rect::from_min_size(text.pos, text.galley.size()))),
            egui::Shape::Vec(inner) => texts(inner, found),
            _ => {}
        }
    }
}

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<Event>) -> Vec<(String, Rect)> {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
    let shapes: Vec<egui::Shape> = out.shapes.into_iter().map(|clipped| clipped.shape).collect();
    let mut found = Vec::new();
    texts(&shapes, &mut found);
    found
}

fn click(shell: &mut AppShell, ctx: &egui::Context, at: Pos2) -> Vec<(String, Rect)> {
    let none = egui::Modifiers::default();
    frame(shell, ctx, vec![Event::PointerMoved(at)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: none }]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: none }]);
    frame(shell, ctx, Vec::new())
}

fn drag(shell: &mut AppShell, ctx: &egui::Context, from: Pos2, to: Pos2) {
    let none = egui::Modifiers::default();
    frame(shell, ctx, vec![Event::PointerMoved(from)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: none }]);
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        frame(shell, ctx, vec![Event::PointerMoved(from + (to - from) * t)]);
    }
    frame(shell, ctx, vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: none }]);
    frame(shell, ctx, Vec::new());
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

fn find(found: &[(String, Rect)], label: &str) -> Rect {
    found.iter().rev().find(|(text, _)| text == label).map(|(_, rect)| *rect).unwrap_or_else(|| panic!("{label}: {:?}", found.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>()))
}

#[test]
fn c09_frame_replace_image_opens_the_file_picker_and_replaces_on_ok() {
    let _home = test_home::enter("c09_shell_frame_replace_picker");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c09 replace picker");
    shell.workspace.activate(doc).unwrap();
    for _ in 0..3 { frame(&mut shell, &ctx, Vec::new()); }
    shell.pick_tool("tool.picture_frame.rect").unwrap();
    frame(&mut shell, &ctx, Vec::new());
    drag(&mut shell, &ctx, pos2(700.0, 420.0), pos2(820.0, 520.0));
    let id = shell.picture_frame_bar.target().expect("new frame is selected");

    dtool_io::install_import_commands();
    let image = std::env::temp_dir().join(format!("dtool_c09_picker_{}.png", std::process::id()));
    let pixels = dtool_export::raster::RgbaImage::from_raw(8, 6, [0u8, 120, 255, 255].repeat(48)).unwrap();
    let png = dtool_export::raster::encode(&pixels, &dtool_export::raster::ExportOptions::for_format(dtool_export::ExportFormat::Png)).unwrap();
    std::fs::write(&image, png).unwrap();

    let found = frame(&mut shell, &ctx, Vec::new());
    let button = find(&found, &dtool_strings::text("frame.replace_image"));
    let steps = undo_steps(&mut shell);
    click(&mut shell, &ctx, button.center());
    frame(&mut shell, &ctx, Vec::new());
    let found = frame(&mut shell, &ctx, Vec::new());
    let state = shell.file_dialog.as_ref().expect("replace image opens the file picker");
    assert_eq!(state.mode, FileDialogMode::ReplaceImage(id.0));
    assert_eq!(state.owner.context(), shell.workspace.active());
    assert!(found.iter().any(|(text, _)| *text == dtool_strings::text("dialog.file.path")), "picker shows the path field");
    assert_eq!(undo_steps(&mut shell), steps, "opening the picker changes nothing");

    shell.file_dialog.as_mut().unwrap().path = image.to_string_lossy().to_string();
    let found = frame(&mut shell, &ctx, Vec::new());
    let ok = find(&found, &dtool_strings::text("button.ok"));
    click(&mut shell, &ctx, ok.center());
    assert!(shell.file_dialog.is_none(), "OK closes the picker");
    assert_eq!(undo_steps(&mut shell), steps + 1, "replace is one undo step");
    let session = shell.workspace.session().unwrap();
    let resource = session.document.placed_resource(id).expect("frame holds the replaced image");
    assert_eq!(session.document.image_resource(resource).expect("image resource").pixel_size, [8, 6]);
    assert!(picture_frame::get_frame(&session.document, id).is_some(), "still a picture frame");

    assert_eq!(shell.picture_frame_bar_edit(FrameBarEdit::PickImage).unwrap()["dialog"], "frame.replace_image");
    let found = frame(&mut shell, &ctx, Vec::new());
    click(&mut shell, &ctx, find(&found, &dtool_strings::text("button.cancel")).center());
    assert!(shell.file_dialog.is_none());
    assert_eq!(undo_steps(&mut shell), steps + 1, "cancel replaces nothing");
    let _ = std::fs::remove_file(&image);
}
