#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::{apply_bindings, menu_binding, MenuBinding, MenuTree, PERSONAS, TOP_LEVEL_ORDER};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn find<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
    for item in items.as_array()? {
        if item.get("id").and_then(Value::as_str) == Some(id) { return Some(item); }
        if let Some(found) = item.get("children").and_then(|c| find(c, id)) { return Some(found); }
    }
    None
}

fn shell_with_document() -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("menu");
    shell.workspace.activate(id).unwrap();
    shell
}

#[test]
fn view_leaves_are_bound_not_planned() {
    let _home = test_home::enter("view_leaves_are_bound_not_planned");
    let tree = MenuTree::for_persona("pixel");
    for id in ["view.zoom.to_fit", "view.zoom.n100pct", "view.zoom.in", "view.zoom.out", "view.rotate.reset", "view.show.rulers", "file.save_history_with_document"] {
        let item = tree.find(id).expect(id);
        assert!(!item.planned, "{id}");
        assert!(menu_binding(id).is_some(), "{id}");
    }
    for id in ["view.clip_to_canvas", "view.preview_mode", "view.mode.pixels", "view.mode.pixels_retina", "view.mode.vector", "view.mode.wireframe.outline", "view.mode.wireframe.last_used", "view.show.guides", "view.show.column_guides", "view.lock_guides", "view.show.bleed", "text.highlights.highlight_missing_fonts", "text.highlights.highlight_missing_characters"] {
        assert!(!tree.find(id).expect(id).planned, "{id}");
        assert!(menu_binding(id).is_some(), "{id}");
    }
    for id in ["view.mode.split_view", "view.new_view", "view.mode.hairline"] {
        assert!(tree.find(id).expect(id).planned, "{id}");
    }
}

#[test]
fn shared_tree_for_every_persona() {
    let _home = test_home::enter("shared_tree_for_every_persona");
    let pixel = MenuTree::for_persona("pixel").top_level_ids();
    assert_eq!(pixel, TOP_LEVEL_ORDER.to_vec());
    for persona in PERSONAS {
        assert_eq!(MenuTree::for_persona(persona).top_level_ids(), pixel, "{persona}");
    }
}

#[test]
fn bound_items_resolve_enabled_and_checked() {
    let _home = test_home::enter("bound_items_resolve_enabled_and_checked");
    let mut shell = shell_with_document();
    let session = shell.workspace.session_mut().unwrap();
    let mut tree = MenuTree::for_persona("pixel").resolved_json(session);
    apply_bindings(&mut tree, session);
    let rulers = find(&tree, "view.show.rulers").unwrap();
    assert_eq!(rulers["enabled"], json!(true));
    assert_eq!(rulers["checked"], json!(session.document.rulers.show));
    let fit = find(&tree, "view.zoom.to_fit").unwrap();
    assert_eq!(fit["planned"], json!(false));
    assert_eq!(fit["enabled"], json!(true));
    let planned = find(&tree, "view.new_view").unwrap();
    assert_eq!(planned["planned"], json!(true));
    assert_eq!(planned["enabled"], json!(false));
    assert!(find(&tree, "edit.cut").is_some());
}

#[test]
fn zoom_rotate_rulers_without_undo() {
    let _home = test_home::enter("zoom_rotate_rulers_without_undo");
    let mut shell = shell_with_document();
    shell.canvas_size = [400.0, 300.0];
    let (zoom, undo) = { let s = shell.workspace.session_mut().unwrap(); (s.view.zoom, s.can_execute_named("edit.undo")) };
    shell.run_menu_binding(MenuBinding::ZoomBy(2.0)).unwrap();
    shell.run_menu_binding(MenuBinding::RotateBy(15.0)).unwrap();
    let rulers = shell.workspace.session_mut().unwrap().document.rulers.show;
    shell.run_menu_binding(MenuBinding::RulerShow).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    assert!((session.view.zoom - dtool_core::clamp_zoom(zoom * 2.0)).abs() < 1e-4);
    assert!((session.view.rotation_deg - 15.0).abs() < 1e-3);
    assert_eq!(session.document.rulers.show, !rulers);
    assert_eq!(session.can_execute_named("edit.undo"), undo);
    shell.run_menu_binding(MenuBinding::ZoomActual).unwrap();
    assert!((shell.workspace.session_mut().unwrap().view.zoom - 1.0).abs() < 1e-4);
    shell.run_menu_binding(MenuBinding::RotateReset).unwrap();
    assert!(shell.workspace.session_mut().unwrap().view.rotation_deg.abs() < 1e-3);
}

#[test]
fn save_history_toggle_follows_flag() {
    let _home = test_home::enter("save_history_toggle_follows_flag");
    let mut shell = shell_with_document();
    let before = shell.workspace.session_mut().unwrap().history.save_with_document;
    shell.run_menu_binding(MenuBinding::SaveHistory).unwrap();
    let session = shell.workspace.session_mut().unwrap();
    assert_eq!(session.history.save_with_document, !before);
    assert_eq!(MenuBinding::SaveHistory.checked(session), Some(!before));
    let queried = session.run_named("history.save_with_doc.query", json!({})).unwrap();
    assert_eq!(queried["enabled"], json!(!before));
}

fn checked_in_tree(shell: &mut AppShell, id: &str) -> Value {
    let session = shell.workspace.session_mut().unwrap();
    let mut tree = MenuTree::for_persona("pixel").resolved_json(session);
    apply_bindings(&mut tree, session);
    let item = find(&tree, id).unwrap().clone();
    assert_eq!(item["enabled"], json!(true), "{id}");
    item["checked"].clone()
}

#[test]
fn c02_view_flags_guides_and_highlight_rows_toggle_on_the_live_shell_without_undo() {
    let _home = test_home::enter("c02_view_flags_guides_and_highlight_rows_toggle_on_the_live_shell_without_undo");
    let ctx = egui::Context::default();
    let mut shell = shell_with_document();
    let run_frame = |shell: &mut AppShell| {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| shell.show(ui));
        out.textures_delta.clear();
    };
    run_frame(&mut shell);
    let (version, undo) = { let s = shell.workspace.session_mut().unwrap(); (s.document.version, s.can_execute_named("edit.undo")) };
    let toggles: [(&str, fn(&dtool_core::Session) -> bool); 9] = [
        ("view.clip_to_canvas", |s| s.view.canvas_flags.clip_to_canvas),
        ("view.preview_mode", |s| s.view.canvas_flags.preview_mode),
        ("view.show.bleed", |s| s.view.canvas_flags.show_bleed),
        ("view.show.guides", |s| s.guides.show),
        ("view.show.column_guides", |s| s.guides.show_columns),
        ("view.lock_guides", |s| s.guides.locked),
        ("text.highlights.highlight_missing_fonts", |s| s.view.highlight().0),
        ("text.highlights.highlight_missing_characters", |s| s.view.highlight().1),
        ("view.mode.vector", |s| s.view.canvas_flags.view_mode == dtool_core::ViewMode::Normal),
    ];
    for (id, read) in toggles {
        let before = read(shell.workspace.session_mut().unwrap());
        assert_eq!(checked_in_tree(&mut shell, id), json!(before), "{id}");
        if id == "view.mode.vector" { continue; }
        shell.run_menu_binding(menu_binding(id).unwrap()).unwrap();
        run_frame(&mut shell);
        let after = read(shell.workspace.session_mut().unwrap());
        assert_eq!(after, !before, "{id}");
        assert_eq!(checked_in_tree(&mut shell, id), json!(after), "{id}");
    }
    for (id, mode) in [("view.mode.pixels", dtool_core::ViewMode::Pixel), ("view.mode.pixels_retina", dtool_core::ViewMode::Pixel2x), ("view.mode.wireframe.outline", dtool_core::ViewMode::Outline), ("view.mode.wireframe.last_used", dtool_core::ViewMode::Wireframe), ("view.mode.vector", dtool_core::ViewMode::Normal)] {
        shell.run_menu_binding(menu_binding(id).unwrap()).unwrap();
        run_frame(&mut shell);
        assert_eq!(shell.workspace.session_mut().unwrap().view.canvas_flags.view_mode, mode, "{id}");
        assert_eq!(checked_in_tree(&mut shell, id), json!(true), "{id}");
        assert_eq!(checked_in_tree(&mut shell, "view.mode.pixels_retina"), json!(mode == dtool_core::ViewMode::Pixel2x), "{id}");
    }
    let session = shell.workspace.session_mut().unwrap();
    assert_eq!(session.document.version, version);
    assert_eq!(session.can_execute_named("edit.undo"), undo);
}
