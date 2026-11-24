use dtool_core::commands::{persona, prefs_cmd};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, CommandResult};
use serde_json::json;
use std::sync::Once;

fn echo(command: &Command) -> CommandResult {
    Ok(json!({ "name": command.name }))
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("u_accept2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(persona::register_action(echo));
        assert!(prefs_cmd::install(dir.join("prefs.json")));
    });
}

fn run(workspace: &mut Workspace, name: &str, params: serde_json::Value) -> CommandResult {
    workspace.run(Scope::active(), Command { name: name.into(), params }).1
}

fn workspace() -> Workspace {
    setup();
    let mut workspace = Workspace::default();
    workspace.new_document("u_accept2");
    workspace
}

#[test]
fn persona_commands_reach_the_installed_handler() {
    let mut ws = workspace();
    for name in ["ui.persona.set", "ui.persona.query", "ui.persona.list", "ui.persona.enter_sub", "ui.persona.leave_sub"] {
        assert_eq!(run(&mut ws, name, json!({})).unwrap()["name"], name);
    }
}

#[test]
fn pref_query_and_set_run_on_the_session_path() {
    let mut ws = workspace();
    let all = run(&mut ws, "pref.query", json!({})).unwrap();
    let (key, value) = all.as_object().unwrap().iter().find(|(_, value)| value.is_boolean()).map(|(k, v)| (k.clone(), v.clone())).unwrap();
    let flipped = json!(!value.as_bool().unwrap());
    let after = run(&mut ws, "pref.set", json!({ key.clone(): flipped })).unwrap();
    assert_eq!(after[&key], flipped);
    assert_eq!(run(&mut ws, "pref.query", json!({ "keys": [key.clone()] })).unwrap()[&key], flipped);
    assert_eq!(prefs_cmd::snapshot().map(|prefs| serde_json::to_value(prefs).unwrap()[&key].clone()), Some(flipped));
    assert_eq!(run(&mut ws, "pref.set", json!({ "no_such_pref": 1 })).unwrap_err().code, "INVALID_PARAMS");
    assert!(run(&mut ws, "pref.set", json!({ "mcp_canva_ai": true })).is_err());
}

const DOCK: [&str; 12] = ["ui.layout.get", "ui.layout.set", "ui.panel.list", "ui.panel.open", "ui.panel.query", "ui.panel.state", "ui.panel.set", "ui.panel.float", "ui.preset.list", "ui.preset.save", "ui.preset.load", "ui.preset.delete"];

fn dock_route(_slot: &mut dtool_core::workspace::UiSessionSlot, command: &Command) -> Option<CommandResult> {
    DOCK.contains(&command.name.as_str()).then(|| Ok(json!({ "routed": command.name })))
}

#[test]
fn dock_commands_are_implemented_and_reach_the_session_route() {
    let mut ws = workspace();
    assert!(dtool_core::workspace::register_ui_session_route(dock_route));
    let catalog = dtool_protocol::CommandCatalog::new();
    for name in DOCK {
        assert!(catalog.resolve(name).unwrap().implemented, "{name}");
        assert_eq!(run(&mut ws, name, json!({})).unwrap()["routed"], name);
    }
}

#[test]
fn export_options_schema_is_implemented_and_requires_an_installed_provider() {
    let mut ws = workspace();
    assert!(dtool_protocol::CommandCatalog::new().resolve("export.options.schema").unwrap().implemented);
    assert_eq!(run(&mut ws, "export.options.schema", json!({})).unwrap_err().code, "NOT_AVAILABLE");
}
