#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;

#[test]
fn persona_pick_command_goes_through_workspace_like_session() {
    let _home = test_home::enter("persona_pick_command_goes_through_workspace_like_session");
    let mut shell = AppShell::new();
    shell.workspace.new_document("a");
    let direct = shell.workspace.session_mut().unwrap().run(&Command::new("ui.persona.query"));
    let routed = shell.workspace.run(Scope::active(), Command::new("ui.persona.query")).1;
    assert_eq!(direct.is_ok(), routed.is_ok());
    let command = dtool_ui::persona_bar::selection_command("vector");
    assert_eq!(command.name, "ui.persona.set");
    let via_shell = shell.run_and_reconcile(command.clone());
    let via_session = shell.workspace.session_mut().unwrap().run(&command);
    let pick = |result: dtool_core::CommandResult| result.map(|value| (value["id"].clone(), value["active_tool"].clone(), value["tools"].clone())).map_err(|error| error.code);
    assert_eq!(pick(via_shell), pick(via_session));
}