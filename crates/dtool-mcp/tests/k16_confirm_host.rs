use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dtool_core::workspace::{host_channel, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(name: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } })
}

#[test]
fn k16_gated_request_reaches_host_slot_and_ui_answer_releases_token() {
    let dir = std::env::temp_dir().join(format!("k16_confirm_host_{}_{}", std::process::id(), dtool_mcp::http::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut workspace = Workspace::default();
    workspace.new_document("host");
    let ai = workspace.ai_connection();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let (handle, inbox) = host_channel(Arc::new(move || { counter.fetch_add(1, Ordering::SeqCst); }));
    inbox.publish(&workspace);
    workspace.set_host_confirms(Some(inbox.confirms()));
    let mut config = Config::default();
    config.allow_dirs = vec![dir.clone()];
    let mut server = Server::with_host(config, handle, ai);

    let policy = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "mcp.confirm.policy.query", "params": {} }));
    assert_eq!(policy["result"]["token_execution_available"], false, "{policy}");
    workspace.host_confirms().unwrap().set_ui_attached(true);
    let policy = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "mcp.confirm.policy.query", "params": {} }));
    assert_eq!(policy["result"]["token_execution_available"], true, "{policy}");

    let path = dir.join("host.dtool");
    let params = json!({ "path": path.to_string_lossy() });
    let reply = server.handle_request(run("doc.save_as", params.clone()));
    assert_eq!(reply["error"]["code"], "NEEDS_CONFIRMATION", "{reply}");
    let id = reply["error"]["confirm_id"].as_u64().unwrap();
    assert!(wakes.load(Ordering::SeqCst) >= 1);
    let pending = workspace.host_confirms().unwrap().pending();
    let item = pending.iter().find(|item| item.id == id).expect("host slot");
    assert_eq!(item.name, "doc.save_as");
    assert_eq!(item.class, "C2Overwrite");
    assert_eq!(item.message_key, "mcp.confirm.c2");
    assert!(workspace.host_confirms().unwrap().answer(id, true));
    assert!(!workspace.host_confirms().unwrap().answer(id, true));
    assert!(workspace.host_confirms().unwrap().pending().iter().all(|item| item.id != id));
    let status = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "mcp.confirm.status", "params": { "confirm_id": id } }));
    let token = status["result"]["confirm_token"].as_str().expect("approved").to_string();

    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let ui = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            inbox.drain(&mut workspace, 32);
            std::thread::sleep(Duration::from_millis(2));
        }
        workspace
    });
    let mut with_token = params.clone();
    with_token["confirm_token"] = json!(token);
    let saved = server.handle_request(run("doc.save_as", with_token));
    stop.store(true, Ordering::SeqCst);
    ui.join().unwrap();
    assert!(saved.get("error").is_none(), "{saved}");
    assert!(path.exists());
}

#[test]
fn k16_ui_decline_marks_declined() {
    let mut workspace = Workspace::default();
    workspace.new_document("host");
    let ai = workspace.ai_connection();
    let (handle, inbox) = host_channel(Arc::new(|| {}));
    inbox.publish(&workspace);
    let mut server = Server::with_host(Config::default(), handle, ai);
    let reply = server.handle_request(run("doc.save_as", json!({ "path": "/definitely/not/allowed/z.dtool" })));
    let id = reply["error"]["confirm_id"].as_u64().unwrap();
    assert!(inbox.confirms().answer(id, false));
    let status = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "mcp.confirm.status", "params": { "confirm_id": id } }));
    assert_eq!(status["result"]["state"], "declined", "{status}");
}
