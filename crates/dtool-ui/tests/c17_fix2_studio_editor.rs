use dtool_ui::{dock, persona, studio_editor};

use dock::PanelId;
use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::{Command, CommandResult};
use persona::{create_studio, delete_studio, host_action, install, rename_studio, switch_persona, update_studio, PersonaEntry, PersonaId, PersonaState, PersonaStore};
use serde_json::json;
use studio_editor::{StudioEditorState, StudioManagerState};

fn vector_tools() -> Vec<&'static str> {
    persona::persona_catalog().into_iter().find(|def| def.entry == PersonaEntry::Builtin(PersonaId::Vector)).unwrap().tools.iter().map(|tool| tool.0).collect()
}

fn host() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("dtool_c17_fix2_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        install(PersonaStore { dir }).unwrap();
    });
}

fn run(command: &Command) -> CommandResult {
    host_action(command)
}

#[test]
fn c17_fix2_create_rename_delete_in_persona_state() {
    let tools = vector_tools();
    let mut state = PersonaState::default();
    let (studio, skipped) = create_studio(&mut state, &json!({ "name": "내 벡터", "base": "vector", "tools": [tools[0], tools[1], "tool.liquify_push"], "panels": ["layers", "swatches", "liquify_mesh"] })).unwrap();
    assert_eq!(studio.tools.iter().map(|tool| tool.0).collect::<Vec<_>>(), vec![tools[0], tools[1]]);
    assert_eq!(studio.panels, vec![PanelId::Layers, PanelId::Swatches]);
    assert_eq!(skipped, vec!["tool.liquify_push".to_string(), "liquify_mesh".to_string()]);
    assert!(studio.panels.iter().all(|panel| !matches!(panel, PanelId::Other(_))));
    switch_persona(&mut state, PersonaEntry::Custom("내 벡터".into())).unwrap();
    assert_eq!(state.active, PersonaEntry::Custom("내 벡터".into()));
    rename_studio(&mut state, &json!({ "name": "내 벡터", "new_name": "벡터 2" })).unwrap();
    assert_eq!(state.active, PersonaEntry::Custom("벡터 2".into()));
    assert_eq!(state.custom_studios[0].name, "벡터 2");
    let skipped = update_studio(&mut state, &json!({ "name": "벡터 2", "panels": ["unknown_panel", "history"] })).unwrap();
    assert_eq!(skipped, vec!["unknown_panel".to_string()]);
    assert_eq!(state.custom_studios[0].panels, vec![PanelId::History]);
    delete_studio(&mut state, &json!({ "name": "벡터 2" })).unwrap();
    assert!(state.custom_studios.is_empty());
    assert_eq!(state.active, PersonaEntry::Builtin(PersonaId::Vector));
    assert_eq!(create_studio(&mut state, &json!({ "name": "x", "base": "liquify" })).unwrap_err().code, "NOT_IMPLEMENTED");
    assert_eq!(create_studio(&mut state, &json!({ "name": "x", "base": "typography" })).unwrap_err().code, "NOT_IMPLEMENTED");
    assert_eq!(create_studio(&mut state, &json!({ "name": "x", "base": "pixel" })).unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn c17_fix2_custom_studios_persist_and_old_state_json_loads() {
    let mut state = PersonaState::default();
    create_studio(&mut state, &json!({ "name": "저장", "tools": [vector_tools()[0]], "panels": ["layers"] })).unwrap();
    let text = serde_json::to_string(&state).unwrap();
    let back: PersonaState = serde_json::from_str(&text).unwrap();
    assert_eq!(back.custom_studios.len(), 1);
    assert_eq!(back.custom_studios[0].name, "저장");
    let mut old = serde_json::to_value(&PersonaState::default()).unwrap();
    old.as_object_mut().unwrap().remove("custom_studios");
    let loaded: PersonaState = serde_json::from_value(old).unwrap();
    assert!(loaded.custom_studios.is_empty());
    let wire = json!({ "name": "s", "base": "vector", "tools": ["tool.unknown", vector_tools()[0]], "toolbar": [], "panels": [{ "Other": "bogus" }, "Layers"] });
    let studio: persona::CustomStudio = serde_json::from_value(wire).unwrap();
    assert_eq!(studio.tools.len(), 1);
    assert_eq!(studio.panels, vec![PanelId::Layers]);
}

#[test]
fn c17_fix2_editor_and_manager_drive_the_host_without_touching_the_document() {
    host();
    let session = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    let version = session.document.version;
    let mut manager = StudioManagerState::new();
    let mut runner = |command: &Command| run(command);
    manager.open_editor(PersonaId::Vector);
    {
        let editor = manager.editing.as_mut().unwrap();
        editor.name = "내 벡터".into();
        let keep: Vec<_> = editor.tools.iter().take(3).cloned().collect();
        editor.tools = keep;
        editor.panels = vec![PanelId::Layers, PanelId::Other("not_a_panel".into())];
    }
    let created = manager.close_editor(&mut runner).unwrap().unwrap();
    assert_eq!(created.tools.len(), 3);
    assert_eq!(created.panels, vec![PanelId::Layers]);
    assert_eq!(manager.skipped, vec!["not_a_panel".to_string()]);
    assert!(manager.studios.iter().any(|studio| studio.name == "내 벡터"));
    let listed = run(&Command::new("ui.persona.list")).unwrap();
    assert!(listed.as_array().unwrap().iter().any(|item| item["id"] == "custom:내 벡터"));
    manager.activate(&mut runner, "내 벡터").unwrap();
    assert_eq!(run(&Command::new("ui.persona.query")).unwrap()["id"], "custom:내 벡터");
    manager.rename(&mut runner, "내 벡터", "벡터 B").unwrap();
    assert!(manager.studios.iter().any(|studio| studio.name == "벡터 B"));
    assert_eq!(run(&Command::new("ui.persona.query")).unwrap()["id"], "custom:벡터 B");
    manager.open_existing("벡터 B").unwrap();
    manager.editing.as_mut().unwrap().toggle_panel(&PanelId::History);
    let edited = manager.close_editor(&mut runner).unwrap().unwrap();
    assert!(edited.panels.contains(&PanelId::History));
    manager.delete(&mut runner, "벡터 B").unwrap();
    assert!(!manager.studios.iter().any(|studio| studio.name == "벡터 B"));
    assert_eq!(run(&Command::new("ui.persona.query")).unwrap()["id"], "vector");
    assert_eq!(session.document.version, version);
    assert_eq!(run(&Command::with_params("ui.persona.set", json!({ "id": "liquify_only_planned" }))).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(run(&Command::with_params("ui.persona.set", json!({ "id": "typography" }))).unwrap_err().code, "NOT_IMPLEMENTED");
    assert_eq!(run(&Command::with_params("ui.studio.create", json!({ "name": "y", "base": "liquify" }))).unwrap_err().code, "NOT_IMPLEMENTED");
}

#[test]
fn c17_fix2_editor_commands_use_keys_not_other() {
    let mut editor = StudioEditorState::new(PersonaId::Vector);
    editor.name = "k".into();
    editor.panels = vec![PanelId::Colour];
    let commands = editor.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "ui.studio.create");
    assert_eq!(commands[0].params["panels"], json!(["colour"]));
    assert_eq!(commands[0].params["base"], "vector");
    let ctx = egui::Context::default();
    let mut manager = StudioManagerState::new();
    manager.open_editor(PersonaId::Vector);
    let mut calls = 0;
    let mut runner = |_: &Command| { calls += 1; Ok(json!([])) };
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| studio_editor::show_manager(ui, &mut manager, &mut runner));
    output.textures_delta.clear();
    assert_eq!(calls, 0);
}
