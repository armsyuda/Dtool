#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::picture_frame::{self, FrameShape, ImageFit};
use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_ui::picture_frame_bar::FrameBarEdit;
use dtool_ui::shell::AppShell;
use dtool_ui::shortcut_commands::ShortcutTarget;
use dtool_ui::tools_panel;
use egui::{pos2, Event, PointerButton, Pos2};

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<Event>) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn drag(shell: &mut AppShell, ctx: &egui::Context, from: Pos2, to: Pos2) {
    frame(shell, ctx, vec![Event::PointerMoved(from)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() }]);
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        frame(shell, ctx, vec![Event::PointerMoved(from + (to - from) * t)]);
    }
    frame(shell, ctx, vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, Vec::new());
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session_mut().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

fn selected(shell: &AppShell) -> Vec<dtool_core::object::ObjectId> {
    shell.workspace.session().unwrap().selection.ids.clone()
}

#[test]
fn picture_frame_tools_draw_frames_and_the_frame_bar_edits_them() {
    let _home = test_home::enter("c09_shell_picture_frame");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c09 frames");
    shell.workspace.activate(doc).unwrap();
    frame(&mut shell, &ctx, Vec::new());

    assert_eq!(shell.handle_shortcut_key(Focus::Canvas, "F", Modifiers::default()), Some(ShortcutTarget::Tool("tool.picture_frame.rect")));
    assert_eq!(shell.workspace.session().unwrap().active_tool, "tool.picture_frame.rect");
    let column = tools_panel::tool_column_ids("tool.picture_frame.rect");
    let listed = tools_panel::PICTURE_FRAME_KEYS.iter().all(|key| dtool_strings::has(key));
    assert_eq!(tools_panel::picture_frame_tools_listed(), listed);
    for id in ["tool.picture_frame", "tool.picture_frame.rect", "tool.picture_frame.ellipse"] {
        assert_eq!(column.contains(&id), listed, "{id}");
        assert!(dtool_design::icons::tool_icon_for(id).is_some() || id == "tool.picture_frame", "{id}");
    }
    assert!(dtool_ui::menu_tree::missing_b11_keys().iter().all(|key| !key.contains("picture_frame")));

    let before = undo_steps(&mut shell);
    drag(&mut shell, &ctx, pos2(700.0, 420.0), pos2(820.0, 520.0));
    let ids = selected(&shell);
    assert_eq!(ids.len(), 1);
    let rect_id = ids[0];
    assert_eq!(undo_steps(&mut shell), before + 1);
    assert_eq!(picture_frame::get_frame(&shell.workspace.session().unwrap().document, rect_id).map(|frame| frame.shape), Some(FrameShape::Rect));
    assert_eq!(shell.picture_frame_bar.target(), Some(rect_id));

    shell.workspace.session_mut().unwrap().selection.ids.clear();
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(shell.picture_frame_bar.target(), None);
    shell.pick_tool("tool.picture_frame.ellipse").unwrap();
    frame(&mut shell, &ctx, Vec::new());
    drag(&mut shell, &ctx, pos2(880.0, 430.0), pos2(960.0, 520.0));
    let ids = selected(&shell);
    assert_eq!(ids.len(), 1);
    let ellipse_id = ids[0];
    assert_ne!(ellipse_id, rect_id);
    assert_eq!(picture_frame::get_frame(&shell.workspace.session().unwrap().document, ellipse_id).map(|frame| frame.shape), Some(FrameShape::Ellipse));
    assert_eq!(shell.picture_frame_bar.target(), Some(ellipse_id));

    let steps = undo_steps(&mut shell);
    shell.picture_frame_bar_edit(FrameBarEdit::Fit(ImageFit::Native)).unwrap();
    assert_eq!(picture_frame::get_frame(&shell.workspace.session().unwrap().document, ellipse_id).unwrap().fit, ImageFit::Native);
    shell.picture_frame_bar_edit(FrameBarEdit::Scale(2.0)).unwrap();
    assert_eq!(picture_frame::get_frame(&shell.workspace.session().unwrap().document, ellipse_id).unwrap().content.scale, [2.0, 2.0]);
    assert_eq!(undo_steps(&mut shell), steps + 2);
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(shell.picture_frame_bar.frame.as_ref().unwrap().content.scale, [2.0, 2.0]);
    shell.picture_frame_bar_edit(FrameBarEdit::Reset).unwrap();
    assert_eq!(picture_frame::get_frame(&shell.workspace.session().unwrap().document, ellipse_id).unwrap().content.scale, [1.0, 1.0]);

    dtool_io::install_import_commands();
    let image = std::env::temp_dir().join(format!("dtool_c09_replace_{}.png", std::process::id()));
    let pixels = dtool_export::raster::RgbaImage::from_raw(8, 6, [0u8, 120, 255, 255].repeat(48)).unwrap();
    let png = dtool_export::raster::encode(&pixels, &dtool_export::raster::ExportOptions::for_format(dtool_export::ExportFormat::Png)).unwrap();
    std::fs::write(&image, png).unwrap();
    assert_eq!(shell.picture_frame_bar_edit(FrameBarEdit::ReplaceImage(String::new())).is_err(), true);
    shell.picture_frame_bar.replace_path = image.to_string_lossy().to_string();
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(shell.picture_frame_bar.replace_path, image.to_string_lossy());
    let steps = undo_steps(&mut shell);
    shell.picture_frame_bar_edit(FrameBarEdit::ReplaceImage(image.to_string_lossy().to_string())).unwrap();
    assert_eq!(undo_steps(&mut shell), steps + 1);
    assert!(shell.picture_frame_bar.replace_path.is_empty());
    let session = shell.workspace.session().unwrap();
    let resource = session.document.placed_resource(ellipse_id).expect("frame holds the replaced image");
    let placed = session.document.image_resource(resource).expect("image resource");
    assert!(!placed.is_linked());
    let _ = std::fs::remove_file(&image);
}
