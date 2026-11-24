use dtool_core::commands::prefs_cmd;
use dtool_core::prefs::Prefs;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, CommandResult};
use serde_json::{json, Value};

fn run(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    workspace.run(Scope::active(), Command { name: name.into(), params }).1
}

fn flip(prefs: &Prefs, key: &str) -> Prefs {
    let mut value = serde_json::to_value(prefs).unwrap();
    value[key] = json!(!value[key].as_bool().unwrap());
    serde_json::from_value(value).unwrap()
}

#[test]
fn replace_and_reload_refresh_pref_query() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("u_prefs_reload_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prefs.json");
    assert!(!prefs_cmd::replace(Prefs::default()));
    assert_eq!(prefs_cmd::reload_from_disk().unwrap_err().code, "NOT_AVAILABLE");
    assert!(prefs_cmd::install(path.clone()));
    let mut ws = Workspace::default();
    ws.new_document("u_prefs_reload");
    let all = run(&mut ws, "pref.query", json!({})).unwrap();
    let key = all.as_object().unwrap().iter().find(|(_, v)| v.is_boolean()).map(|(k, _)| k.clone()).unwrap();
    let start = all[&key].as_bool().unwrap();
    let replaced = flip(&prefs_cmd::snapshot().unwrap(), &key);
    assert!(prefs_cmd::replace(replaced));
    assert_eq!(run(&mut ws, "pref.query", json!({ "keys": [key.clone()] })).unwrap()[&key], json!(!start));
    let on_disk = flip(&prefs_cmd::snapshot().unwrap(), &key);
    on_disk.save(&path).unwrap();
    let loaded = prefs_cmd::reload_from_disk().unwrap();
    assert_eq!(serde_json::to_value(&loaded).unwrap()[&key], json!(start));
    assert_eq!(run(&mut ws, "pref.query", json!({ "keys": [key.clone()] })).unwrap()[&key], json!(start));
}
