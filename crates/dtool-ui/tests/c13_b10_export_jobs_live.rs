use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::{Command, CommandResult};
use dtool_ui::shell::install_export_host;
use serde_json::{json, Value};
use std::path::PathBuf;

fn fresh() -> Workspace {
    let _ = install_export_host();
    let mut workspace = Workspace::default();
    workspace.new_document("c13_b10_jobs");
    workspace.session_mut().unwrap().document.create_object(dtool_core::object::Object { name: "box".into(), pos: [5.0, 5.0], size: [20.0, 10.0], ..Default::default() });
    workspace
}

fn call(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params(name, params)).1
}

#[test]
fn c13_b10_export_jobs_are_implemented_and_route_on_session_path() {
    let catalog = dtool_protocol::catalog::CommandCatalog::new();
    for name in ["export.job.run", "export.job.list", "export.job.delete"] {
        assert!(catalog.get(name).unwrap_or_else(|| panic!("{name} missing")).implemented, "{name}");
    }
    assert!(catalog.get("export.options.schema").unwrap().implemented);
    let mut workspace = fresh();
    let schema = call(&mut workspace, "export.options.schema", json!({})).expect("real writer schema");
    assert_eq!(schema["formats"].as_array().unwrap().iter().filter(|f| f["available"] == true).count(), 6);
    let listed = call(&mut workspace, "export.job.list", json!({})).expect("export.job.list");
    assert!(listed["items"].is_array(), "{listed}");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_b10_job_{}.png", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let ran = call(&mut workspace, "export.job.run", json!({ "path": out, "overwrite": "replace" })).expect("export.job.run");
    assert!(ran.get("ok").is_some(), "{ran}");
    assert!(ran["written"].as_array().is_some_and(|items| !items.is_empty()), "{ran}");
    assert!(out.exists(), "{ran}");
    let after = call(&mut workspace, "export.job.list", json!({})).unwrap();
    let ids: Vec<u64> = after["items"].as_array().unwrap().iter().filter_map(|item| item["id"].as_u64()).collect();
    let deleted = call(&mut workspace, "export.job.delete", json!({ "all": true })).expect("export.job.delete");
    assert!(deleted.is_object(), "{deleted}");
    let empty = call(&mut workspace, "export.job.list", json!({})).unwrap();
    assert!(empty["items"].as_array().unwrap().is_empty(), "{ids:?} {empty}");
    let _ = std::fs::remove_file(&out);
}