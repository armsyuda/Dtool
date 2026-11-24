#[path = "support/test_home.rs"]
mod test_home;
use std::sync::Arc;
use std::time::Duration;

use dtool_core::document::Units;
use dtool_core::workspace::{host_channel, CloseAnswer, ContextId, HostHandle, HostInbox, HostReply, Scope};
use dtool_core::Command;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

struct Live {
    shell: AppShell,
    handle: HostHandle,
    inbox: HostInbox,
}

impl Live {
    fn new() -> Self {
        let (handle, inbox) = host_channel(Arc::new(|| {}));
        Live { shell: AppShell::new(), handle, inbox }
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
use dtool_ui::dialogs::{DialogKind, DialogRegistry};

fn guide_count(live: &Live, id: ContextId) -> usize {
    live.shell.workspace.get(id).unwrap().session.guides.items.len()
}

#[test]
fn live_guides_dialog_follows_opener_through_mcp_and_registry_mirrors_it() {
    let _home = test_home::enter("live_guides_dialog_follows_opener_through_mcp_and_registry_mirrors_it");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    assert!(live.shell.open_guides_manager());
    let b = live.new_doc(300.0, 300.0);
    assert_eq!(live.shell.guides_owner, Some(a));
    let added = live.mcp(Scope::of(a), "guides.add", json!({ "kind": "Horizontal", "position": 40.0, "name": "live" }));
    assert_eq!(added.context, Some(a));
    assert!(added.result.is_ok(), "{:?}", added.result.err());
    assert_eq!(guide_count(&live, a), 1);
    assert_eq!(guide_count(&live, b), 0);
    assert_eq!(live.shell.workspace.active(), Some(b));
    let mut registry = DialogRegistry::default();
    registry.sync_document(DialogKind::Guides, live.shell.guides_dialog.open, live.shell.guides_owner);
    assert_eq!(registry.kinds_for(a), vec![DialogKind::Guides]);
    assert!(registry.kinds_for(b).is_empty());
    assert!(registry.gone_kinds(&live.shell.workspace).is_empty());
    live.shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    assert_eq!(live.mcp(Scope::of(a), "guides.add", json!({ "kind": "Vertical", "position": 10.0 })).result.err().unwrap().code, "CONTEXT_NOT_FOUND");
    assert_eq!(registry.take_gone(&live.shell.workspace), vec![DialogKind::Guides]);
    assert!(!registry.is_open(DialogKind::Guides));
    live.shell.drop_orphan_dialogs();
    assert!(!live.shell.guides_dialog.open);
    assert_eq!(live.shell.guides_owner, None);
    assert_eq!(live.shell.status_bar.hint_key, "error.context_not_found");
    assert_eq!(guide_count(&live, b), 0);
}

#[test]
fn live_guides_dialog_on_surviving_document_is_kept() {
    let _home = test_home::enter("live_guides_dialog_on_surviving_document_is_kept");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    let b = live.new_doc(300.0, 300.0);
    assert!(live.shell.open_guides_manager());
    assert_eq!(live.shell.guides_owner, Some(b));
    live.shell.workspace.close_with(a, CloseAnswer::No).unwrap();
    let mut registry = DialogRegistry::default();
    registry.sync_document(DialogKind::Guides, live.shell.guides_dialog.open, live.shell.guides_owner);
    assert!(registry.take_gone(&live.shell.workspace).is_empty());
    live.shell.drop_orphan_dialogs();
    assert!(live.shell.guides_dialog.open);
    assert!(live.mcp(Scope::of(b), "guides.add", json!({ "kind": "Vertical", "position": 10.0 })).result.is_ok());
    assert_eq!(guide_count(&live, b), 1);
}

#[test]
fn live_shell_owns_a_dialog_registry() {
    let _home = test_home::enter("live_shell_owns_a_dialog_registry");
    let shell = include_str!("../src/shell.rs");
    assert!(shell.contains("pub dialogs: crate::dialogs::DialogRegistry"), "AppShell.dialogs missing; real test shell_registry_mirrors_document_dialogs ships in PR-U08-5");
    assert!(shell.contains("fn sync_dialog_registry"));
    assert!(shell.contains("take_gone(&self.workspace)"));
}