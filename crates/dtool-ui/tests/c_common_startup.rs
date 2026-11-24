#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};
fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}
#[test]
fn common_commands_work_before_first_canvas_paint_with_ai_connection_off() {
    let _home = test_home::enter("common_startup_no_ai");
    let mut shell = AppShell::new(); shell.workspace.ai_connection().set_from_gui(false);
    shell.workspace.new_document("startup");
    assert!(dtool_core::commands::macro_host::installed());
    assert!(dtool_core::commands::text_host::installed());
    let added = run(&mut shell, "text.art.add", json!({"pos": [0, 0], "text": "cat"}));
    let story = added["story"].as_u64().unwrap();
    assert_eq!(run(&mut shell, "text.find", json!({"pattern": "cat"}))["match"]["story"], story);
    run(&mut shell, "macro.record.start", json!({}));
    run(&mut shell, "text.style", json!({"story": story, "range": {"start": 0, "end": 3}, "patch": {"weight": 700}}));
    let stop = run(&mut shell, "macro.record.stop", json!({}));
    assert_eq!(stop["steps"].as_array().unwrap().len(), 1);
    assert!(!shell.workspace.ai_connection().enabled());
}
