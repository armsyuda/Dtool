use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dtool_core::workspace::{host_channel, HostRequest, Scope, Workspace, ContextId};
use dtool_core::Command;

fn counter_wake() -> (Arc<AtomicUsize>, dtool_core::HostWake) {
    let count = Arc::new(AtomicUsize::new(0));
    let inner = Arc::clone(&count);
    (count, Arc::new(move || { inner.fetch_add(1, Ordering::SeqCst); }))
}

#[test]
fn k16_send_wakes_and_apply_replies_with_context_and_generation() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("host");
    let (count, wake) = counter_wake();
    let (handle, inbox) = host_channel(wake);
    let (reply, rx) = std::sync::mpsc::channel();
    handle.send(HostRequest { scope: Scope::of(id), command: Command::new("doc.query"), expected_active_path: None, reply }).ok().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(inbox.drain(&mut workspace, 32), 1);
    let reply = rx.recv().unwrap();
    assert_eq!(reply.context, Some(id));
    assert!(reply.result.is_ok());
    assert_eq!(reply.generation, Some(workspace.get(id).unwrap().session.generation));
    assert_eq!(handle.state().active, Some(id));
}

#[test]
fn k16_mutation_from_another_thread_changes_the_owned_workspace() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("host");
    let before = workspace.get(id).unwrap().session.generation;
    let (_count, wake) = counter_wake();
    let (handle, inbox) = host_channel(wake);
    let sender = handle.clone();
    let worker = std::thread::spawn(move || {
        sender.request(Scope::active(), Command::with_params("object.create", serde_json::json!({ "name": "r", "pos": [0.0, 0.0], "size": [10.0, 10.0] })), None, Duration::from_secs(5))
    });
    let mut applied = 0;
    for _ in 0..500 {
        applied += inbox.drain(&mut workspace, 32);
        if applied > 0 { break; }
        std::thread::sleep(Duration::from_millis(2));
    }
    let reply = worker.join().unwrap();
    assert_eq!(applied, 1);
    assert_eq!(reply.context, Some(id));
    let generation = workspace.get(id).unwrap().session.generation;
    assert!(reply.result.is_ok());
    assert!(generation > before);
    assert_eq!(reply.generation, Some(generation));
}

#[test]
fn k16_closed_context_and_dropped_inbox_never_report_success() {
    let mut workspace = Workspace::default();
    let (_count, wake) = counter_wake();
    let (handle, inbox) = host_channel(wake);
    let (reply, rx) = std::sync::mpsc::channel();
    handle.send(HostRequest { scope: Scope::of(ContextId(99)), command: Command::new("doc.query"), expected_active_path: None, reply }).ok().unwrap();
    inbox.drain(&mut workspace, 32);
    assert_eq!(rx.recv().unwrap().result.unwrap_err().code, "CONTEXT_NOT_FOUND");
    assert!(workspace.contexts().is_empty());
    drop(inbox);
    let (reply, _rx) = std::sync::mpsc::channel();
    assert!(handle.send(HostRequest { scope: Scope::active(), command: Command::new("doc.query"), expected_active_path: None, reply }).is_err());
    let late = handle.request(Scope::active(), Command::new("doc.query"), None, Duration::from_millis(50));
    assert_eq!(late.result.unwrap_err().code, "SHUTDOWN");
}

#[test]
fn k16_reply_times_out_when_ui_thread_does_not_drain() {
    let (_count, wake) = counter_wake();
    let (handle, _inbox) = host_channel(wake);
    let reply = handle.request(Scope::active(), Command::new("doc.query"), None, Duration::from_millis(30));
    assert_eq!(reply.result.unwrap_err().code, "UI_TIMEOUT");
}

#[test]
fn k16_stale_active_path_is_refused_before_running() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("host");
    let before = workspace.get(id).unwrap().session.generation;
    let (_count, wake) = counter_wake();
    let (handle, inbox) = host_channel(wake);
    let (reply, rx) = std::sync::mpsc::channel();
    handle.send(HostRequest { scope: Scope::active(), command: Command::new("edit.undo"), expected_active_path: Some("elsewhere.dtool".into()), reply }).ok().unwrap();
    inbox.drain(&mut workspace, 32);
    assert_eq!(rx.recv().unwrap().result.unwrap_err().code, "NEEDS_CONFIRMATION");
    assert_eq!(workspace.get(id).unwrap().session.generation, before);
}

#[test]
fn k16_workspace_run_refuses_uncatalogued_names_before_any_hook() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("gate");
    let before = workspace.get(id).unwrap().session.generation;
    for name in ["", "no.such.command", "doc.close.everything"] {
        let (_, result) = workspace.run(Scope::active(), Command::new(name));
        assert_eq!(result.unwrap_err().code, "UNKNOWN_COMMAND", "{name}");
    }
    assert_eq!(workspace.contexts().len(), 1);
    assert_eq!(workspace.get(id).unwrap().session.generation, before);
    let (_, preview) = workspace.run(Scope::active(), Command::new("render.node_preview"));
    assert_ne!(preview.unwrap_err().code, "UNKNOWN_COMMAND");
}
