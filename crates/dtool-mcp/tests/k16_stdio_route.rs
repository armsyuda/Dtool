#[path = "../../dtool-cli/src/harness.rs"]
#[allow(dead_code)]
mod profile_fixture;

use dtool_core::workspace::{run_shared, SharedWorkspace};
use dtool_core::{Command, Scope, Workspace};
use dtool_mcp::{serve_stdio, Config, Server};
use dtool_protocol::CommandCatalog;
use profile_fixture::TestProfile;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

fn fixture() -> (Server, SharedWorkspace, TestProfile) {
    let profile = TestProfile::new();
    let mut workspace = Workspace::default();
    workspace.new_document("k16-stdio");
    let shared = workspace.into_shared();
    let server = Server::with_workspace(Config { allow_dirs: vec![profile.path().to_path_buf()], ..Config::default() }, Arc::clone(&shared));
    server.gui_connection_handle().set_from_gui(true);
    (server, shared, profile)
}

fn stdio(server: &mut Server, request: &Value) -> Value {
    let mut bytes = serde_json::to_vec(request).unwrap();
    bytes.push(b'\n');
    let mut output = Vec::new();
    serve_stdio(server, std::io::Cursor::new(bytes), &mut output);
    serde_json::from_slice(&output).unwrap()
}

fn run_request(name: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } })
}

#[test]
fn stdio_catalog_matches_the_shared_workspace_catalog() {
    let (mut server, shared, _profile) = fixture();
    let wire = stdio(&mut server, &json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.metadata" }));
    let stdio_names: Vec<String> = serde_json::from_value(wire["result"]["catalog"].clone()).unwrap();
    let session_names = shared.lock().unwrap().session().unwrap().catalog.names();
    assert!(stdio_names.iter().any(|name| name == "app.about"));
    assert_eq!(stdio_names.iter().collect::<BTreeSet<_>>().len(), stdio_names.len());
    assert_eq!(stdio_names, session_names);
    assert_eq!(stdio_names, CommandCatalog::new().names());
}

#[test]
fn stdio_app_about_equals_the_direct_workspace_result() {
    let (mut server, shared, _profile) = fixture();
    let direct = run_shared(&shared, Scope::active(), Command::with_params("app.about", json!({}))).1.unwrap();
    let wire = stdio(&mut server, &run_request("app.about", json!({})));
    assert!(wire.get("error").is_none(), "{wire}");
    assert_eq!(wire["result"], direct);
}

#[test]
fn stdio_bad_app_about_payload_is_invalid_params_like_the_direct_route() {
    let (mut server, shared, _profile) = fixture();
    let direct = run_shared(&shared, Scope::active(), Command::with_params("app.about", json!({ "invalid": true }))).1.unwrap_err();
    let wire = stdio(&mut server, &run_request("app.about", json!({ "invalid": true })));
    assert_eq!(direct.code, "INVALID_PARAMS");
    assert_eq!(wire["error"]["code"], "INVALID_PARAMS");
}

#[test]
fn endpoint_handoff_path_stays_inside_the_test_profile() {
    let (_server, _shared, profile) = fixture();
    let path = dtool_mcp::handoff::default_endpoint_path().unwrap();
    assert!(path.starts_with(profile.path()));
}