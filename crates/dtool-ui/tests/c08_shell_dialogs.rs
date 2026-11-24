#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::blend::BlendMode;
use dtool_core::object::Object;
use dtool_ui::menu_tree::{row_is_local, BLEND_OPTIONS_MENU_ID, LAYER_EFFECTS_MENU_ID};
use dtool_ui::shell::AppShell;
use egui::Event;

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<Event>) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn escape() -> Event {
    Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() }
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

#[test]
fn layer_effects_and_blend_options_dialogs_mount_on_the_live_shell() {
    let _home = test_home::enter("c08_shell_dialogs");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c08 dialogs");
    shell.workspace.activate(doc).unwrap();
    assert!(row_is_local(LAYER_EFFECTS_MENU_ID, true));
    assert!(row_is_local(BLEND_OPTIONS_MENU_ID, true));
    frame(&mut shell, &ctx, Vec::new());
    assert!(!shell.open_layer_effects());
    let id = {
        let session = shell.workspace.session_mut().unwrap();
        let id = session.document.create_object(Object { pos: [100.0, 100.0], size: [200.0, 120.0], ..Object::default() });
        session.selection.ids = vec![id];
        session.document.version += 1;
        id
    };
    frame(&mut shell, &ctx, Vec::new());
    let (version, steps) = (shell.workspace.session().unwrap().document.version, undo_steps(&mut shell));

    assert!(shell.open_layer_effects());
    frame(&mut shell, &ctx, Vec::new());
    assert!(shell.layer_effects.open);
    assert_eq!(shell.layer_effects.object_id, Some(id.0));
    shell.layer_effects.enable_outer_shadow(shell.workspace.session_mut().unwrap());
    assert!(!dtool_core::commands::layer_style::staged(shell.workspace.session().unwrap(), id).is_none());
    frame(&mut shell, &ctx, Vec::new());
    frame(&mut shell, &ctx, vec![escape()]);
    frame(&mut shell, &ctx, Vec::new());
    assert!(!shell.layer_effects.open);
    let session = shell.workspace.session().unwrap();
    assert!(dtool_core::commands::layer_style::staged(session, id).is_none());
    assert!(session.document.get_object(id).unwrap().effects.is_empty());
    assert_eq!(session.document.version, version);
    assert_eq!(undo_steps(&mut shell), steps);

    assert!(shell.open_layer_effects());
    frame(&mut shell, &ctx, Vec::new());
    shell.layer_effects.enable_outer_shadow(shell.workspace.session_mut().unwrap());
    assert!(shell.layer_effects.ok(shell.workspace.session_mut().unwrap()));
    frame(&mut shell, &ctx, Vec::new());
    assert!(!shell.layer_effects.open);
    assert_eq!(shell.workspace.session().unwrap().document.get_object(id).unwrap().effects.len(), 1);
    assert_eq!(undo_steps(&mut shell), steps + 1);
    let steps = undo_steps(&mut shell);

    assert!(shell.open_blend_options());
    frame(&mut shell, &ctx, Vec::new());
    assert!(shell.blend_options.open);
    assert_eq!(shell.blend_options.mode, "Normal");
    shell.blend_options.set_mode("Multiply");
    assert!(shell.blend_options.apply(shell.workspace.session_mut().unwrap()));
    frame(&mut shell, &ctx, Vec::new());
    assert!(!shell.blend_options.open);
    assert_eq!(shell.workspace.session().unwrap().document.get_object(id).unwrap().blend_mode, BlendMode::Multiply);
    assert_eq!(undo_steps(&mut shell), steps + 1);

    assert!(shell.open_blend_options());
    assert_eq!(shell.blend_options.mode, "Multiply");
    shell.workspace.close_with(doc, dtool_core::workspace::CloseAnswer::No).unwrap();
    frame(&mut shell, &ctx, Vec::new());
    assert!(!shell.blend_options.open);
}
