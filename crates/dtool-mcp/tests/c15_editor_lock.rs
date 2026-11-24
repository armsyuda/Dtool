use dtool_core::workspace::SharedWorkspace;
use dtool_core::Workspace;
use dtool_mcp::{is_catalog_mutating, Config, Server};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fixture(timeout: Duration) -> (Server, SharedWorkspace) {
    let mut workspace = Workspace::default();
    workspace.new_document("c15");
    let shared = workspace.into_shared();
    let server = Server::with_workspace(Config { busy_timeout: timeout, ..Config::default() }, Arc::clone(&shared));
    server.gui_connection_handle().set_from_gui(true);
    (server, shared)
}

fn generation(shared: &SharedWorkspace) -> u64 {
    shared.lock().unwrap().session().unwrap().generation
}

fn object_count(shared: &SharedWorkspace) -> usize {
    shared.lock().unwrap().session().unwrap().document.objects.len()
}

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }))
}

fn create() -> Value {
    json!({ "name": "c15", "pos": [0.0, 0.0], "size": [10.0, 10.0] })
}

#[test]
fn catalog_flags_decide_which_commands_take_the_lock() {
    assert!(is_catalog_mutating("object.create"));
    assert!(is_catalog_mutating("object.update"));
    assert!(!is_catalog_mutating("app.about"));
    assert!(!is_catalog_mutating("b15.not_a_command"));
}

#[test]
fn failing_mutating_command_is_stopped_by_preflight_and_changes_nothing() {
    let (mut server, shared) = fixture(Duration::from_millis(200));
    let before = (generation(&shared), object_count(&shared));
    let reply = run(&mut server, "object.update", json!({ "id": 999_999, "name": "ghost" }));
    assert_eq!(reply["error"]["code"], "INVALID_PARAMS", "{reply}");
    assert_eq!((generation(&shared), object_count(&shared)), before);
    assert!(!server.gui_connection_handle().editor_lock().is_locked());
}

#[test]
fn mutating_command_runs_under_the_lock_and_releases_it_after_the_reply() {
    let (mut server, shared) = fixture(Duration::from_millis(200));
    let before = generation(&shared);
    let reply = run(&mut server, "object.create", create());
    assert!(reply.get("error").is_none(), "{reply}");
    assert!(generation(&shared) > before);
    assert_eq!(object_count(&shared), 1);
    assert!(!server.gui_connection_handle().editor_lock().is_locked());
}

#[test]
fn held_lock_gives_editor_busy_and_leaves_the_document_unchanged() {
    let (mut server, shared) = fixture(Duration::from_millis(60));
    let lock = server.gui_connection_handle().editor_lock();
    assert!(lock.try_acquire());
    let before = (generation(&shared), object_count(&shared));
    let started = Instant::now();
    let reply = run(&mut server, "object.create", create());
    assert!(started.elapsed() >= Duration::from_millis(60));
    assert_eq!(reply["error"]["code"], "EDITOR_BUSY", "{reply}");
    assert_eq!(reply["error"]["message_key"], "error.editor_busy");
    assert_eq!((generation(&shared), object_count(&shared)), before);
    let read = run(&mut server, "app.about", json!({}));
    assert!(read.get("error").is_none(), "{read}");
    let undo = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo" }));
    assert!(undo.to_string().contains("EDITOR_BUSY"), "{undo}");
    assert!(lock.is_locked());
    lock.release();
    let reply = run(&mut server, "object.create", create());
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(object_count(&shared), 1);
}

#[test]
fn busy_lock_is_waited_for_up_to_the_timeout() {
    let (mut server, shared) = fixture(Duration::from_secs(3));
    let lock = server.gui_connection_handle().editor_lock();
    assert!(lock.try_acquire());
    let releaser = {
        let lock = Arc::clone(&lock);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            lock.release();
        })
    };
    let reply = run(&mut server, "object.create", create());
    releaser.join().unwrap();
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(object_count(&shared), 1);
    assert!(!lock.is_locked());
}