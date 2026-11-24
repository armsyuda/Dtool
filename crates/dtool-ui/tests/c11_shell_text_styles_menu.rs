#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::text_styles::StyleKind;
use dtool_core::{Command, Scope};
use dtool_ui::dock::PanelId;
use dtool_ui::menu_tree::{self, TEXT_STYLES_MENU_ROWS};
use dtool_ui::panels::text_styles::TEXT_STYLES_PANEL_KEY;
use dtool_ui::shell::AppShell;
use egui::{Pos2, Rect};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn names(shell: &mut AppShell, kind: &str) -> Vec<String> {
    run(shell, "text.styles.list", json!({ "kind": kind }))["styles"].as_array().unwrap().iter().filter_map(|style| style["name"].as_str().map(str::to_string)).collect()
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

#[test]
fn c11_text_styles_menu_opens_bot07_panel_and_runs_through_it() {
    let _home = test_home::enter("c11_shell_text_styles_menu");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("styles menu");
    shell.workspace.activate(doc).unwrap();
    for id in TEXT_STYLES_MENU_ROWS {
        assert!(menu_tree::row_is_local(id, true), "{id}");
    }
    assert!(!TEXT_STYLES_MENU_ROWS.contains(&"window.text.text_styles"), "Window toggle stays with window_panel");
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "hello world" }));
    let (object, story) = (added["id"].as_u64().unwrap(), added["story"].as_u64().unwrap());
    run(&mut shell, "text.select", json!({ "story": story, "id": object, "range": { "start": 0, "end": 11 } }));
    frame(&mut shell, &ctx);
    let panel = PanelId::Other(TEXT_STYLES_PANEL_KEY.to_string());
    if shell.layout.is_panel_shown(&panel) {
        dtool_ui::panels::toggle_panel(&mut shell.layout, &panel);
    }
    assert!(!shell.layout.is_panel_shown(&panel));

    let steps = undo_steps(&mut shell);
    shell.run_text_styles_menu("text.text_styles.create_character_style").expect("create");
    assert!(shell.layout.is_panel_shown(&panel), "menu row opens the Text Styles panel");
    let created = shell.panel_registry.text_styles.selected.clone().expect("panel selects the new style");
    assert_eq!(created.0, StyleKind::Character);
    assert!(names(&mut shell, "character").contains(&created.1));
    assert_eq!(undo_steps(&mut shell), steps + 1);
    frame(&mut shell, &ctx);
    shell.run_text_styles_menu("text.text_styles.create_character_style").expect("create again");
    frame(&mut shell, &ctx);
    assert!(shell.layout.is_panel_shown(&panel), "a second menu use keeps the panel open");

    run(&mut shell, "text.style", json!({ "story": story, "range": { "start": 0, "end": 11 }, "patch": { "size_pt": 31.0 } }));
    shell.panel_registry.text_styles.selected = Some(created.clone());
    shell.run_text_styles_menu("text.text_styles.update_character_style").expect("update");
    let list = run(&mut shell, "text.styles.list", json!({ "kind": "character" }));
    let style = list["styles"].as_array().unwrap().iter().find(|style| style["name"] == json!(created.1)).unwrap().clone();
    assert_eq!(style["props"]["size_pt"].as_f64().map(|v| v.round()), Some(31.0), "{style}");

    run(&mut shell, "text.style.apply_named", json!({ "story": story, "range": { "start": 0, "end": 11 }, "id": created.1, "kind": "character" }));
    shell.panel_registry.text_styles.selected = None;
    let before = undo_steps(&mut shell);
    shell.run_text_styles_menu("text.text_styles.delete_character_style").expect("delete uses the style at the caret");
    assert!(!names(&mut shell, "character").contains(&created.1));
    assert_eq!(undo_steps(&mut shell), before + 1);
    run(&mut shell, "edit.undo", json!({}));
    assert!(names(&mut shell, "character").contains(&created.1));
    frame(&mut shell, &ctx);
}
