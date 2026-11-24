use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use dtool_core::workspace::{host_channel, Workspace};
use dtool_mcp::http::{new_token, serve_http, HttpOptions};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn post(addr: std::net::SocketAddr, auth: Option<&str>, body: &Value) -> (String, Value) {
    let text = body.to_string();
    let mut stream = TcpStream::connect(addr).unwrap();
    let auth = auth.map(|value| format!("Authorization: {value}\r\n")).unwrap_or_default();
    write!(stream, "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Length: {}\r\n\r\n{text}", text.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("null");
    (response.clone(), serde_json::from_str(body).unwrap_or(Value::Null))
}

fn headless() -> Server {
    let mut workspace = Workspace::default();
    workspace.new_document("k16");
    Server::new(Config::default(), workspace)
}

#[test]
fn k16_token_is_32_hex_and_differs() {
    let a = new_token();
    let b = new_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn k16_bad_or_missing_bearer_is_unauthorized_and_runs_nothing() {
    let mut server = headless();
    let shared = server.workspace();
    let http = serve_http(&mut server, HttpOptions { addr: "127.0.0.1:0".parse().unwrap() }).unwrap();
    let before = shared.lock().unwrap().contexts().len();
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "doc.new", "params": {} } });
    for auth in [None, Some("Bearer wrong")] {
        let (raw, body) = post(http.addr, auth, &request);
        assert!(raw.starts_with("HTTP/1.1 401"));
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
    }
    assert_eq!(shared.lock().unwrap().contexts().len(), before);
    let (raw, body) = post(http.addr, Some(&format!("bearer {}", http.token)), &json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.run", "params": { "name": "app.status", "params": {} } }));
    assert!(raw.starts_with("HTTP/1.1 200"));
    assert!(body.get("error").is_none(), "{body}");
}

#[test]
fn k16_http_and_stdio_give_same_error_for_same_request() {
    let mut server = headless();
    let http = serve_http(&mut server, HttpOptions { addr: "127.0.0.1:0".parse().unwrap() }).unwrap();
    let auth = format!("Bearer {}", http.token);
    let requests = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "doc.open", "params": { "path": "/definitely/not/allowed/x.dtool" } } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.query", "params": { "name": "object.create", "params": {} } }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "dtool.run", "params": { "name": "doc.save_as", "params": { "path": "/definitely/not/allowed/y.dtool" } } }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "dtool.run", "params": { "name": "no.such.command", "params": {} } }),
    ];
    for request in requests {
        let stdio = server.handle_request(request.clone());
        let (_, wire) = post(http.addr, Some(&auth), &request);
        assert!(stdio.get("error").is_some(), "{stdio}");
        assert_eq!(stdio["error"]["code"], wire["error"]["code"], "{request}");
    }
}

#[test]
fn k16_http_handle_drop_returns_without_a_client_and_frees_the_port() {
    let mut server = headless();
    let http = serve_http(&mut server, HttpOptions { addr: "127.0.0.1:0".parse().unwrap() }).unwrap();
    let addr = http.addr;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { drop(http); let _ = tx.send(()); });
    rx.recv_timeout(Duration::from_secs(2)).expect("drop returned");
    assert!(TcpListener::bind(addr).is_ok());
}

#[test]
fn k16_handoff_file_round_trip_and_removed_on_drop() {
    let dir = std::env::temp_dir().join(format!("k16_handoff_{}_{}", std::process::id(), new_token()));
    let path = dir.join("DTool").join(dtool_mcp::handoff::FILE_NAME);
    let mut server = headless();
    let mut http = serve_http(&mut server, HttpOptions { addr: "127.0.0.1:0".parse().unwrap() }).unwrap();
    http.publish_endpoint(&path).unwrap();
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["addr"], http.addr.to_string());
    assert_eq!(value["token"], http.token.as_str());
    assert_eq!(value["pid"], std::process::id());
    let instance = server.instance_reply().to_string();
    assert!(!instance.contains(&http.token));
    drop(http);
    let stopped: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!dtool_mcp::handoff::is_live(&stopped), "{stopped}");
    assert!(stopped["token"].is_null());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn k16_gui_mode_server_runs_commands_on_the_ui_owned_workspace() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("gui");
    let (handle, inbox) = host_channel(std::sync::Arc::new(|| {}));
    inbox.publish(&workspace);
    let connection = workspace.ai_connection();
    let mut server = Server::with_host(Config::default(), handle, connection.clone());
    assert!(server.is_host_mode());
    assert!(server.shared_workspace().is_none());
    let worker = std::thread::spawn(move || {
        let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "object.create", "params": { "name": "m", "pos": [0.0, 0.0], "size": [5.0, 5.0] } } }));
        connection.set_from_gui(false);
        let off = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.run", "params": { "name": "app.status", "params": {} } }));
        (reply, off)
    });
    let before = workspace.get(id).unwrap().session.generation;
    let mut applied = 0;
    for _ in 0..1000 {
        applied += inbox.drain(&mut workspace, 32);
        if applied > 0 { break; }
        std::thread::sleep(Duration::from_millis(2));
    }
    let (reply, off) = worker.join().unwrap();
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(reply["result"]["context_id"], id.0);
    assert!(workspace.get(id).unwrap().session.generation > before);
    assert_eq!(off["error"]["code"], "AI_CONNECTION_DISABLED");
}
