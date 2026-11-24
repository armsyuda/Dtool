use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dtool_core::workspace::{host_channel, HostReply, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn request(method: &str, name: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": { "name": name, "params": {} } })
}

fn headless() -> Server {
    let mut workspace = Workspace::default();
    workspace.new_document("u12");
    Server::new(Config::default(), workspace)
}

#[test]
fn shared_backend_ui_commands_are_no_window() {
    let mut server = headless();
    for (method, name) in [("dtool.run", "ui.screenshot.capture"), ("dtool.run", "ui.state.query"), ("dtool.query", "ui.state.query")] {
        let reply = server.handle_request(request(method, name));
        assert_eq!(reply["error"]["code"], "NO_WINDOW", "{method} {name} {reply}");
        assert_eq!(reply["error"]["message_key"], "error.no_window");
        assert!(reply.get("result").is_none());
    }
}

#[test]
fn host_backend_forwards_ui_reply_unchanged() {
    let mut workspace = Workspace::default();
    workspace.new_document("host");
    let ai = workspace.ai_connection();
    let (handle, inbox) = host_channel(Arc::new(|| {}));
    inbox.publish(&workspace);
    let mut config = Config::default();
    config.busy_timeout = Duration::from_secs(5);
    let mut server = Server::with_host(config, handle, ai);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let names = Arc::clone(&seen);
    let ui = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !flag.load(Ordering::SeqCst) && Instant::now() < deadline {
            match inbox.try_next() {
                Some(req) => {
                    names.lock().unwrap().push(req.command.name.clone());
                    let kind = if req.command.name == "ui.screenshot.capture" { "ui_screenshot" } else { "ui_state" };
                    let _ = req.reply.send(HostReply { context: req.scope.context, result: Ok(json!({ "kind": kind, "w": 3 })), generation: None });
                }
                None => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    });
    let shot = server.handle_request(request("dtool.run", "ui.screenshot.capture"));
    assert_eq!(shot["result"], json!({ "kind": "ui_screenshot", "w": 3 }), "{shot}");
    let state = server.handle_request(request("dtool.query", "ui.state.query"));
    assert_eq!(state["result"], json!({ "kind": "ui_state", "w": 3 }), "{state}");
    stop.store(true, Ordering::SeqCst);
    ui.join().unwrap();
    assert_eq!(*seen.lock().unwrap(), vec!["ui.screenshot.capture".to_string(), "ui.state.query".to_string()]);
}

#[test]
fn ui_host_command_names() {
    assert!(dtool_mcp::is_ui_host_command("ui.state.query"));
    assert!(dtool_mcp::is_ui_host_command("ui.screenshot.capture"));
    assert!(!dtool_mcp::is_ui_host_command("doc.query"));
}