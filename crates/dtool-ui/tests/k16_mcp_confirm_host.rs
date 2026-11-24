#[path = "support/test_home.rs"]
mod test_home;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dtool_core::workspace::{HostConfirm, HostConfirms};
use dtool_ui::shell::AppShell;

fn item(id: u64, class: &str, expires_at: Instant) -> HostConfirm {
    HostConfirm { id, class: class.to_string(), message_key: format!("mcp.confirm.{}", class.to_lowercase()), name: "sample.command".to_string(), context: None, expires_at }
}

#[test]
fn confirm_polls_pending_in_order_and_answers() {
    let _home = test_home::enter("confirm_polls_pending_in_order_and_answers");
    let confirms = HostConfirms::default();
    let answers = Arc::new(Mutex::new(Vec::new()));
    let capture = answers.clone();
    confirms.set_answer(Arc::new(move |id, approve| { capture.lock().unwrap().push((id, approve)); true }));
    confirms.push(item(1, "C1", Instant::now() + Duration::from_secs(10)));
    confirms.push(item(2, "C2", Instant::now() + Duration::from_secs(10)));
    let mut shell = AppShell::new();
    shell.workspace.set_host_confirms(Some(confirms.clone()));
    shell.poll_mcp_confirms();
    assert!(confirms.ui_attached());
    assert_eq!(shell.mcp_confirm.as_ref().map(|item| item.0), Some(1));
    assert!(shell.answer_mcp_confirm(&confirms, true));
    shell.poll_mcp_confirms();
    assert_eq!(shell.mcp_confirm.as_ref().map(|item| item.0), Some(2));
    assert!(shell.answer_mcp_confirm(&confirms, false));
    assert_eq!(*answers.lock().unwrap(), vec![(1, true), (2, false)]);
}

#[test]
fn expired_or_withdrawn_confirm_drops_dialog() {
    let _home = test_home::enter("expired_or_withdrawn_confirm_drops_dialog");
    let confirms = HostConfirms::default();
    confirms.push(item(1, "C3", Instant::now() + Duration::from_secs(10)));
    let mut shell = AppShell::new();
    shell.workspace.set_host_confirms(Some(confirms.clone()));
    shell.poll_mcp_confirms();
    confirms.withdraw(1);
    confirms.push(item(2, "C1", Instant::now() - Duration::from_secs(1)));
    shell.poll_mcp_confirms();
    assert!(shell.mcp_confirm.is_none());
    shell.workspace.set_host_confirms(None);
    assert!(shell.poll_mcp_confirms().is_none());
}
