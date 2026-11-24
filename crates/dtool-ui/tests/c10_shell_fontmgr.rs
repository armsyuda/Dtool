#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::font_combo::{FontComboState, RECENT_LIMIT};
use dtool_ui::font_manager::FontManagerAction;
use dtool_ui::shell::{AppShell, FONT_MANAGER_MENU_ID};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn families(shell: &mut AppShell) -> Vec<(String, String)> {
    let query = run(shell, "font.usage.query", json!({}));
    query["usages"].as_array().unwrap().iter().map(|u| (u["family"].as_str().unwrap().to_string(), u["style"].as_str().unwrap_or("").to_string())).collect()
}

#[test]
fn c10_font_manager_runs_on_the_live_session() {
    let _home = test_home::enter("c10_shell_fontmgr");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert_eq!(FONT_MANAGER_MENU_ID, "document.font_manager");
    assert_eq!(shell.open_font_manager().unwrap_err().code, "NO_DOCUMENT");
    assert!(!shell.font_manager.open);

    let doc = shell.workspace.new_document("c10 font manager");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "hello" }));
    let object = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();
    run(&mut shell, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "family": "SourceSans", "style": "Regular" } }));
    run(&mut shell, "selection.set", json!({ "ids": [], "op": "Replace" }));

    shell.open_font_manager().expect("opens with a document");
    assert!(shell.font_manager.open);
    assert!(shell.dialog_open(&ctx));
    frame(&mut shell, &ctx);
    assert!(shell.font_manager.open, "window stays open after a frame");
    let index = shell.font_manager.usages.iter().position(|row| row.family == "SourceSans").expect("usage row");
    shell.font_manager.selected_index = Some(index);

    shell.font_manager_action(FontManagerAction::Locate).unwrap();
    let selected = run(&mut shell, "selection.query", json!({}));
    assert!(selected.to_string().contains(&object.to_string()), "{selected}");

    shell.font_manager_action(FontManagerAction::OpenReplace).unwrap();
    assert!(shell.font_replace.open);
    assert_eq!(shell.font_replace.from_family, "SourceSans");
    frame(&mut shell, &ctx);
    assert!(shell.font_replace.open);
    shell.font_manager_action(FontManagerAction::CancelReplace).unwrap();
    assert!(!shell.font_replace.open);
    assert_eq!(families(&mut shell), vec![("SourceSans".to_string(), "Regular".to_string())]);

    shell.font_manager_action(FontManagerAction::OpenReplace).unwrap();
    shell.font_replace.family = "TargetSerif".into();
    shell.font_replace.style = "Bold".into();
    let version = shell.workspace.session_mut().unwrap().document.version;
    shell.font_manager_action(FontManagerAction::ConfirmReplace).unwrap();
    assert!(!shell.font_replace.open);
    assert!(shell.workspace.session_mut().unwrap().document.version > version);
    assert_eq!(families(&mut shell), vec![("TargetSerif".to_string(), "Bold".to_string())]);
    assert_eq!(shell.font_manager.usages[0].family, "TargetSerif");

    frame(&mut shell, &ctx);
    shell.font_manager_action(FontManagerAction::Close).unwrap();
    assert!(!shell.font_manager.open);
    assert!(!shell.dialog_open(&ctx));

    shell.open_font_manager().unwrap();
    shell.workspace.close_with(doc, dtool_core::workspace::CloseAnswer::No).ok();
    frame(&mut shell, &ctx);
    assert!(!shell.font_manager.open, "closing the owner document closes the manager");
}

#[test]
fn c10_font_combo_recent_list() {
    let mut state = FontComboState::default();
    for index in 0..RECENT_LIMIT + 3 {
        state.remember(&format!("Family {index}"));
    }
    assert_eq!(state.recent_families.len(), RECENT_LIMIT);
    assert_eq!(state.recent_families[0], format!("Family {}", RECENT_LIMIT + 2));
    state.remember("family 5");
    assert_eq!(state.recent_families[0], "family 5");
    assert_eq!(state.recent_families.iter().filter(|f| f.eq_ignore_ascii_case("family 5")).count(), 1);
    state.filter = "SANS".into();
    assert!(state.matches("Source Sans"));
    assert!(!state.matches("Serif"));
}
