use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dtool_core::workspace::{HostWake, Workspace};
use dtool_mcp::handoff;
use dtool_mcp::http::{serve_http, HttpOptions, LiveMcp, STOP_WAIT};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn wake() -> HostWake { Arc::new(|| {}) }

fn endpoint(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("k16_live").join(format!("{name}_{}", dtool_mcp::http::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(handoff::FILE_NAME)
}

fn workspace() -> Workspace {
    let mut workspace = Workspace::default();
    workspace.new_document("k16_live");
    workspace.ai_connection().set_from_gui(true);
    workspace
}

fn status(id: u64) -> Value { json!({ "jsonrpc": "2.0", "id": id, "method": "dtool.run", "params": { "name": "app.status", "params": {} } }) }

fn post(addr: SocketAddr, token: &str, body: &Value) -> Option<Value> {
    let text = body.to_string();
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(stream, "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    serde_json::from_str(response.split_once("\r\n\r\n")?.1).ok()
}

fn read_json(path: &PathBuf) -> Value { serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap() }

fn run_with_drain(live: &LiveMcp, workspace: &mut Workspace, body: Value) -> Value {
    let addr = live.addr().unwrap();
    let token = live.token().unwrap().to_string();
    let client = std::thread::spawn(move || post(addr, &token, &body));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.is_finished() && Instant::now() < deadline {
        live.drain(workspace, 32);
        std::thread::sleep(Duration::from_millis(2));
    }
    client.join().unwrap().expect("reply")
}

fn wait_in_flight(probe: impl Fn() -> usize, at_least: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while probe() < at_least && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(probe() >= at_least);
}

#[test]
fn k16_live_toggle_starts_stops_and_restarts_without_restart_of_app() {
    let mut ws = workspace();
    let path = endpoint("toggle");
    let mut live = LiveMcp::new();
    assert!(!live.sync(false, &mut ws, wake(), Config::default(), Some(&path)));
    assert!(live.sync(true, &mut ws, wake(), Config::default(), Some(&path)));
    assert!(live.is_running());
    assert!(!live.sync(true, &mut ws, wake(), Config::default(), Some(&path)));
    let first_addr = live.addr().unwrap();
    let first_token = live.token().unwrap().to_string();
    assert!(handoff::is_live(&read_json(&path)));
    assert_eq!(read_json(&path)["token"], first_token.as_str());
    let reply = run_with_drain(&live, &mut ws, status(1));
    assert!(reply.get("error").is_none(), "{reply}");

    let started = Instant::now();
    assert!(live.sync(false, &mut ws, wake(), Config::default(), Some(&path)));
    assert!(started.elapsed() < STOP_WAIT + Duration::from_millis(500));
    assert!(!live.is_running());
    let report = live.last_stop().unwrap();
    assert!(report.listener_joined);
    assert_eq!(report.in_flight_left, 0);
    assert!(path.exists());
    let stopped = read_json(&path);
    assert!(!handoff::is_live(&stopped), "{stopped}");
    assert!(stopped["token"].is_null());
    assert_eq!(stopped["stopped"], true);
    assert!(TcpStream::connect_timeout(&first_addr, Duration::from_millis(300)).is_err() || post(first_addr, &first_token, &status(2)).is_none());

    assert!(live.sync(true, &mut ws, wake(), Config::default(), Some(&path)));
    assert_ne!(live.token().unwrap(), first_token);
    assert!(handoff::is_live(&read_json(&path)));
    let reply = run_with_drain(&live, &mut ws, status(3));
    assert!(reply.get("error").is_none(), "{reply}");
    assert!(live.stop(&mut ws, STOP_WAIT).is_some());
    assert!(live.stop(&mut ws, STOP_WAIT).is_none());
}

#[test]
fn k16_live_stop_completes_or_shuts_down_in_flight_host_request() {
    let mut ws = workspace();
    let mut live = LiveMcp::new();
    assert!(live.start(&mut ws, wake(), Config::default(), None));
    let addr = live.addr().unwrap();
    let token = live.token().unwrap().to_string();
    let client = std::thread::spawn(move || post(addr, &token, &status(7)));
    wait_in_flight(|| live.in_flight(), 1);
    std::thread::sleep(Duration::from_millis(50));
    let report = live.stop(&mut ws, STOP_WAIT).unwrap();
    assert!(report.elapsed < STOP_WAIT + Duration::from_millis(500));
    assert_eq!(report.in_flight_left, 0);
    let reply = client.join().unwrap().expect("in-flight request must be answered");
    match reply.get("error") {
        None => {}
        Some(error) => assert_eq!(error["code"], "SHUTDOWN", "{reply}"),
    }
}

#[test]
fn k16_request_arriving_after_shutdown_is_answered_shutdown() {
    let mut ws = Workspace::default();
    ws.new_document("k16_live");
    let mut server = Server::new(Config::default(), ws);
    server.gui_connection_handle().set_from_gui(true);
    let mut http = serve_http(&mut server, HttpOptions { addr: "127.0.0.1:0".parse().unwrap() }).unwrap();
    let body = status(9).to_string();
    let mut stream = TcpStream::connect_timeout(&http.addr, Duration::from_secs(2)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(stream, "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", http.token, body.len()).unwrap();
    stream.flush().unwrap();
    wait_in_flight(|| http.in_flight(), 1);
    http.begin_shutdown();
    assert!(http.is_shutting_down());
    stream.write_all(body.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let reply: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(reply["error"]["code"], "SHUTDOWN", "{reply}");
    assert_eq!(reply["id"], 9);
    let report = http.stop(STOP_WAIT);
    assert!(report.listener_joined);
    assert_eq!(report.in_flight_left, 0);
}

#[test]
fn k16_stalled_client_cannot_hang_stop() {
    let mut ws = workspace();
    let mut live = LiveMcp::new();
    assert!(live.start(&mut ws, wake(), Config::default(), None));
    let stalled = TcpStream::connect_timeout(&live.addr().unwrap(), Duration::from_secs(2)).unwrap();
    wait_in_flight(|| live.in_flight(), 1);
    let wait = Duration::from_millis(400);
    let started = Instant::now();
    let report = live.stop(&mut ws, wait).unwrap();
    assert!(started.elapsed() < wait + Duration::from_millis(600));
    assert!(report.listener_joined);
    assert!(report.in_flight_left <= 1);
    assert!(!live.is_running());
    drop(stalled);
}

#[test]
fn k16_invalidate_only_touches_own_handoff_and_never_deletes() {
    let path = endpoint("own");
    let addr: SocketAddr = "127.0.0.1:4567".parse().unwrap();
    handoff::write_endpoint(&path, addr, "other").unwrap();
    assert!(!handoff::invalidate_endpoint(&path, "mine"));
    assert_eq!(read_json(&path)["token"], "other");
    assert!(handoff::invalidate_endpoint(&path, "other"));
    assert!(path.exists());
    assert!(!handoff::is_live(&read_json(&path)));
    assert!(!handoff::invalidate_endpoint(&path, "other"));
    assert!(handoff::is_live(&handoff::endpoint_json(addr, "x")));
    assert!(!handoff::is_live(&handoff::stopped_json()));
}