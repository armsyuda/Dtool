use std::path::PathBuf;
use std::time::Duration;

use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::permission::{confirm_store, ConfirmStore, GateClass};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("k16_confirm_{tag}_{}_{}", std::process::id(), dtool_mcp::http::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn server(dir: &PathBuf) -> Server {
    let mut workspace = Workspace::default();
    workspace.new_document("k16c");
    let mut config = Config::default();
    config.allow_dirs = vec![dir.clone()];
    Server::new(config, workspace)
}

fn run(name: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } })
}

fn with_token(name: &str, mut params: Value, token: &str) -> Value {
    params["confirm_token"] = json!(token);
    run(name, params)
}

fn approve(server: &mut Server, reply: &Value) -> String {
    assert_eq!(reply["error"]["code"], "NEEDS_CONFIRMATION", "{reply}");
    let id = reply["error"]["confirm_id"].as_u64().expect("confirm_id");
    assert!(reply.to_string().find("confirm_token").is_none());
    confirm_store().approve(id).unwrap();
    let status = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "mcp.confirm.status", "params": { "confirm_id": id } }));
    assert_eq!(status["result"]["state"], "approved", "{status}");
    status["result"]["confirm_token"].as_str().unwrap().to_string()
}

#[test]
fn k16_save_as_needs_confirmation_then_runs_once_with_token() {
    let dir = temp_dir("save");
    let mut server = server(&dir);
    let path = dir.join("out.dtool");
    let params = json!({ "path": path.to_string_lossy() });
    let reply = server.handle_request(run("doc.save_as", params.clone()));
    assert!(!path.exists());
    let id = reply["error"]["confirm_id"].as_u64().unwrap();
    let list = server.handle_request(json!({ "jsonrpc": "2.0", "id": 3, "method": "mcp.confirm.list", "params": {} }));
    let pending = list["result"]["pending"].as_array().unwrap();
    let item = pending.iter().find(|item| item["confirm_id"] == id).expect("listed");
    assert_eq!(item["name"], "doc.save_as");
    assert!(item.get("confirm_token").is_none() && item.get("token").is_none());
    let token = approve(&mut server, &reply);
    let ok = server.handle_request(with_token("doc.save_as", params.clone(), &token));
    assert!(ok.get("error").is_none(), "{ok}");
    assert!(path.exists());
    let other = json!({ "path": dir.join("other.dtool").to_string_lossy() });
    let again = server.handle_request(with_token("doc.save_as", other, &token));
    assert_eq!(again["error"]["code"], "CONFIRM_INVALID", "{again}");
    assert!(!dir.join("other.dtool").exists());
    let same_path = server.handle_request(run("doc.save_as", params));
    assert!(same_path.get("error").is_none(), "{same_path}");
}

#[test]
fn k16_token_is_bound_to_params() {
    let dir = temp_dir("bind");
    let mut server = server(&dir);
    let a = json!({ "path": dir.join("a.dtool").to_string_lossy() });
    let b = json!({ "path": dir.join("b.dtool").to_string_lossy() });
    let reply = server.handle_request(run("doc.save_as", a));
    let token = approve(&mut server, &reply);
    let wrong = server.handle_request(with_token("doc.save_as", b, &token));
    assert_eq!(wrong["error"]["code"], "CONFIRM_INVALID", "{wrong}");
    assert!(!dir.join("b.dtool").exists());
}

#[test]
fn k16_document_edit_after_request_is_stale() {
    let dir = temp_dir("stale");
    let mut server = server(&dir);
    let params = json!({ "path": dir.join("s.dtool").to_string_lossy() });
    let reply = server.handle_request(run("doc.save_as", params.clone()));
    let token = approve(&mut server, &reply);
    let edit = server.handle_request(run("object.create", json!({ "pos": [0.0, 0.0], "size": [10.0, 10.0] })));
    assert!(edit.get("error").is_none(), "{edit}");
    let stale = server.handle_request(with_token("doc.save_as", params, &token));
    assert_eq!(stale["error"]["code"], "CONFIRM_STALE", "{stale}");
    assert!(!dir.join("s.dtool").exists());
}

#[test]
fn k16_client_cannot_approve_and_query_is_ungated() {
    let dir = temp_dir("deny");
    let mut server = server(&dir);
    for name in ["mcp.confirm.approve", "mcp.confirm.decline"] {
        let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": name, "params": { "confirm_id": 1 } }));
        assert_eq!(reply["error"]["code"], "PERMISSION_DENIED", "{reply}");
        let wrapped = server.handle_request(run(name, json!({ "confirm_id": 1 })));
        assert_eq!(wrapped["error"]["code"], "PERMISSION_DENIED", "{wrapped}");
    }
    let query = server.handle_request(run("export.batch.query", json!({})));
    assert_ne!(query["error"]["code"], "NEEDS_CONFIRMATION", "{query}");
    let policy = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "mcp.confirm.policy.query", "params": {} }));
    assert_eq!(policy["result"]["token_execution_available"], confirm_store().approver_attached());
}

#[test]
fn k16_store_expiry_and_decline() {
    let store = ConfirmStore::new(Duration::from_millis(1));
    let request = run("doc.save_as", json!({ "path": "x.dtool" }));
    let id = store.request(&request, GateClass::C2Overwrite, "mcp.confirm.c2", Some(ContextId(1)), Some(0));
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(store.approve(id), Err("CONFIRM_EXPIRED"));
    let store = ConfirmStore::new(Duration::from_secs(60));
    let id = store.request(&request, GateClass::C2Overwrite, "mcp.confirm.c2", Some(ContextId(1)), Some(0));
    store.approve(id).unwrap();
    let token = store.status(id)["confirm_token"].as_str().unwrap().to_string();
    assert_eq!(store.take(&request, &token, Some(ContextId(2)), Some(0)), Err("CONFIRM_INVALID"));
    let id = store.request(&request, GateClass::C2Overwrite, "mcp.confirm.c2", Some(ContextId(1)), Some(0));
    store.decline(id);
    assert_eq!(store.status(id)["state"], "declined");
    assert_eq!(store.approve(id), Err("CONFIRM_INVALID"));
    assert!(store.list().is_empty());
}
