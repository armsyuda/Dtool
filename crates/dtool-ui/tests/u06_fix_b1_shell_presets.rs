#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{HostReply, HostRequest, Scope};
use dtool_protocol::Command;
use dtool_ui::menu_tree::{panels_action, preset_action};
use dtool_ui::presets::open_preset_dialog;
use dtool_ui::shell::AppShell;
use serde_json::json;

fn host(shell: &mut AppShell, name: &str, params: serde_json::Value) -> HostReply {
    let (reply, rx) = std::sync::mpsc::channel();
    let ctx = egui::Context::default();
    shell.apply_host_request(&ctx, HostRequest { scope: Scope::active(), command: Command::with_params(name, params), expected_active_path: None, reply });
    rx.try_recv().expect("reply sent on the UI thread")
}

#[test]
fn preset_menu_items_are_live_and_open_shell_dialogs() {
    let _home = test_home::enter("preset_menu_items_are_live_and_open_shell_dialogs");
    for id in ["window.panels.add_preset", "window.panels.manage_presets"] {
        assert!(preset_action(id));
        assert_eq!(panels_action(id), None);
    }
    let mut shell = AppShell::new();
    assert!(open_preset_dialog("window.panels.add_preset", &mut shell.add_preset, &mut shell.manage_presets));
    assert!(shell.add_preset.open);
    assert!(open_preset_dialog("window.panels.manage_presets", &mut shell.add_preset, &mut shell.manage_presets));
    assert!(shell.manage_presets.open);
}

#[test]
fn host_ui_preset_list_is_answered_from_shell_store() {
    let _home = test_home::enter("host_ui_preset_list_is_answered_from_shell_store");
    let mut shell = AppShell::new();
    let reply = host(&mut shell, "ui.preset.list", json!({}));
    let value = reply.result.expect("ui.preset.list ok");
    assert_eq!(value["presets"], json!(shell.preset_store.list()));
}

#[test]
fn host_ui_layout_and_panel_commands_use_shell_layout() {
    let _home = test_home::enter("host_ui_layout_and_panel_commands_use_shell_layout");
    let mut shell = AppShell::new();
    let panels = host(&mut shell, "ui.panel.list", json!({})).result.expect("ui.panel.list ok");
    assert!(panels.is_object());
    let layout = host(&mut shell, "ui.layout.get", json!({})).result;
    assert!(layout.is_ok(), "{layout:?}");
}

#[test]
fn host_non_ui_command_still_reaches_workspace() {
    let _home = test_home::enter("host_non_ui_command_still_reaches_workspace");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("a");
    let reply = host(&mut shell, "ui.tool.set", json!({ "id": "tool.move" }));
    assert_eq!(reply.context, Some(id));
}