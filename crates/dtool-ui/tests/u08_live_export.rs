#[path = "support/test_home.rs"]
mod test_home;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use dtool_core::document::Units;
use dtool_core::workspace::{host_channel, CloseAnswer, ContextId, HostHandle, HostInbox, HostReply, Scope};
use dtool_core::Command;
use dtool_ui::dialogs::export::{UI_EXPORT_CLOSE, UI_EXPORT_OPEN, UI_EXPORT_QUERY};
use dtool_ui::shell::{export_dialog, install_export_host, AppShell};
use serde_json::{json, Value};

static SERIAL: Mutex<()> = Mutex::new(());

struct Live {
    shell: AppShell,
    handle: HostHandle,
    inbox: HostInbox,
    _serial: MutexGuard<'static, ()>,
}

impl Live {
    fn new() -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        install_export_host();
        if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
        let (handle, inbox) = host_channel(Arc::new(|| {}));
        Live { shell: AppShell::new(), handle, inbox, _serial: serial }
    }

    fn mcp(&mut self, scope: Scope, name: &str, params: Value) -> HostReply {
        let handle = self.handle.clone();
        let command = Command::with_params(name, params);
        let worker = std::thread::spawn(move || handle.request(scope, command, None, Duration::from_secs(20)));
        while !worker.is_finished() {
            self.inbox.drain(&mut self.shell.workspace, 32);
            std::thread::yield_now();
        }
        worker.join().unwrap()
    }

    fn new_doc(&mut self, width: f64, height: f64) -> ContextId {
        let reply = self.mcp(Scope::active(), "doc.new", json!({ "width": width, "height": height, "units": Units::Px, "dpi": 72.0, "pages": 1 }));
        assert!(reply.result.is_ok(), "doc.new: {:?}", reply.result.err());
        let id = reply.context.expect("doc.new context");
        assert_eq!(self.shell.workspace.active(), Some(id));
        id
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if let Ok(mut dialog) = export_dialog().lock() { dialog.close(); }
    }
}

fn preview_ratio(value: &Value) -> f64 {
    let preview = &value["preview"];
    let width = preview["width"].as_f64().expect("preview width");
    let height = preview["height"].as_f64().expect("preview height");
    width / height
}

#[test]
fn live_mcp_export_open_and_query_read_the_scoped_document() {
    let _home = test_home::enter("live_mcp_export_open_and_query_read_the_scoped_document");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    let b = live.new_doc(300.0, 300.0);
    assert_eq!(live.shell.workspace.active(), Some(b));
    let opened = live.mcp(Scope::of(a), UI_EXPORT_OPEN, json!({ "format": "png" }));
    assert_eq!(opened.context, Some(a));
    let opened = opened.result.expect("ui.export.open on a");
    assert_eq!(opened["open"], true);
    let query = live.mcp(Scope::of(a), UI_EXPORT_QUERY, json!({}));
    assert_eq!(query.context, Some(a));
    let query = query.result.expect("ui.export.query on a");
    assert_eq!(query["open"], true);
    assert!((preview_ratio(&query) - 2.0).abs() < 0.05, "{query}");
    assert_eq!(query["preview"]["format"], "png");
    assert_eq!(live.shell.workspace.active(), Some(b));
    assert!(export_dialog().lock().unwrap().is_open());
    let closed = live.mcp(Scope::of(a), UI_EXPORT_CLOSE, json!({}));
    assert!(closed.result.is_ok());
    assert!(!export_dialog().lock().unwrap().is_open());
}

#[test]
fn live_mcp_export_refuses_a_closed_document() {
    let _home = test_home::enter("live_mcp_export_refuses_a_closed_document");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    let _b = live.new_doc(300.0, 300.0);
    live.shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    let reply = live.mcp(Scope::of(a), UI_EXPORT_OPEN, json!({}));
    assert_eq!(reply.result.err().expect("closed doc refused").code, "CONTEXT_NOT_FOUND");
    assert!(!export_dialog().lock().unwrap().is_open());
}

#[test]
fn live_shell_export_dialog_stays_on_opener_across_mcp_doc_new() {
    let _home = test_home::enter("live_shell_export_dialog_stays_on_opener_across_mcp_doc_new");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    live.shell.open_export_dialog();
    assert!(export_dialog().lock().unwrap().is_open());
    let b = live.new_doc(300.0, 300.0);
    assert_eq!(live.shell.workspace.active(), Some(b));
    assert_eq!(export_dialog().lock().unwrap().owner(), Some(a));
    live.shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    live.shell.drop_orphan_dialogs();
    assert!(!export_dialog().lock().unwrap().is_open());
    assert_eq!(live.shell.status_bar.hint_key, "error.context_not_found");
}

#[test]
fn live_mcp_export_query_from_other_document_is_refused() {
    let _home = test_home::enter("live_mcp_export_query_from_other_document_is_refused");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    let b = live.new_doc(300.0, 300.0);
    assert!(live.mcp(Scope::of(a), UI_EXPORT_OPEN, json!({})).result.is_ok());
    let reply = live.mcp(Scope::of(b), UI_EXPORT_QUERY, json!({}));
    assert_eq!(reply.result.err().expect("other doc refused").code, "CONTEXT_NOT_FOUND");
    let query = live.mcp(Scope::of(a), UI_EXPORT_QUERY, json!({})).result.expect("owner query");
    assert!((preview_ratio(&query) - 2.0).abs() < 0.05, "{query}");
}