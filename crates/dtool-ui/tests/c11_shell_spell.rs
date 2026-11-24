#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::menu_tree;
use dtool_ui::shell::AppShell;
use dtool_ui::spell_host::{self, CHECK_SPELLING_MENU_ID, IGNORE_WORD_MENU_ID};
use dtool_ui::spelling_panel::SpellPanelAction;
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session_mut().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

fn story_text(shell: &mut AppShell, story: u64) -> String {
    let session = shell.workspace.session_mut().unwrap();
    session.document.stories.iter().find(|s| s.id.0 == story).map(|s| s.text.clone()).unwrap_or_default()
}

fn rows(shell: &AppShell) -> Vec<String> {
    shell.spell_host.rows()
}

fn menu(shell: &mut AppShell, id: &str) -> bool {
    let AppShell { spell_host, workspace, .. } = shell;
    spell_host.handle_menu(id, workspace)
}

fn apply(shell: &mut AppShell, action: SpellPanelAction) -> bool {
    let AppShell { spell_host, workspace, .. } = shell;
    spell_host.apply(workspace, action)
}

#[test]
fn c11_spelling_window_checks_replaces_and_ignores_on_the_live_story() {
    let _home = test_home::enter("c11_shell_spell");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert!(!menu_tree::row_is_local(CHECK_SPELLING_MENU_ID, false));
    let doc = shell.workspace.new_document("c11 spell");
    shell.workspace.activate(doc).unwrap();
    assert!(menu_tree::row_is_local(CHECK_SPELLING_MENU_ID, true));
    assert!(menu_tree::row_is_local(IGNORE_WORD_MENU_ID, true));
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "I recieved teh book" }));
    let object = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();
    run(&mut shell, "selection.set", json!({ "ids": [object], "op": "Replace" }));
    frame(&mut shell, &ctx);
    frame(&mut shell, &ctx);

    assert!(menu(&mut shell, CHECK_SPELLING_MENU_ID));
    assert!(shell.spell_host.open);
    assert_eq!(rows(&shell), vec!["recieved".to_string(), "teh".to_string()]);
    let panel = &shell.spell_host;
    assert_eq!(panel.selected, Some(0));
    assert!(panel.suggestions.iter().any(|s| s == "received"), "{:?}", panel.suggestions);
    for _ in 0..2 {
        frame(&mut shell, &ctx);
    }
    assert!(ctx.memory(|m| m.area_rect(spell_host::window_id())).is_some(), "spelling window drawn in a real shell frame");

    let before = undo_steps(&mut shell);
    assert!(apply(&mut shell, SpellPanelAction::Replace("received".into())));
    assert_eq!(story_text(&mut shell, story), "I received teh book");
    assert_eq!(undo_steps(&mut shell), before + 1, "one replace = one undo step");
    assert_eq!(rows(&shell), vec!["teh".to_string()]);
    frame(&mut shell, &ctx);

    run(&mut shell, "text.select", json!({ "story": story, "id": object, "range": { "start": 12, "end": 12 } }));
    let before = undo_steps(&mut shell);
    assert!(menu(&mut shell, IGNORE_WORD_MENU_ID));
    assert!(rows(&shell).is_empty());
    assert_eq!(undo_steps(&mut shell), before + 1, "document ignore is one undo step");
    assert!(shell.workspace.session().unwrap().document.spell_ignore.iter().any(|word| word == "teh"));
    assert_eq!(story_text(&mut shell, story), "I received teh book");
    frame(&mut shell, &ctx);

    assert!(!apply(&mut shell, SpellPanelAction::Recheck));
    assert!(rows(&shell).is_empty());
    run(&mut shell, "edit.undo", json!({}));
    assert!(!apply(&mut shell, SpellPanelAction::Recheck));
    assert_eq!(rows(&shell), vec!["teh".to_string()], "undo brings the ignored word back");
    assert_eq!(story_text(&mut shell, story), "I received teh book");

    let before = undo_steps(&mut shell);
    assert!(!apply(&mut shell, SpellPanelAction::AddWord));
    assert!(rows(&shell).is_empty(), "added word is no longer flagged");
    assert_eq!(undo_steps(&mut shell), before, "add to dictionary is not a document edit");
    let dictionary = std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("DTool").join("user_dictionary_en_US.txt");
    assert!(std::fs::read_to_string(&dictionary).unwrap().lines().any(|line| line.trim() == "teh"));
    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(story_text(&mut shell, story), "I recieved teh book");
    shell.spell_host.open = false;
    frame(&mut shell, &ctx);
}