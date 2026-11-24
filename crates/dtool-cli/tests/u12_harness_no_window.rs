use dtool_cli::harness::{is_ui_host_command, run_cli_args, InProcessHarness, TestFixture, TestProfile, UI_HOST_COMMANDS};
use dtool_core::workspace::Workspace;
use dtool_protocol::{Command, CommandResult};
use serde_json::json;

fn assert_no_window(result: CommandResult) {
    let error = result.expect_err("headless harness must not answer a UI host command");
    assert_eq!(error.code, "NO_WINDOW");
    assert_eq!(error.message_key, "error.no_window");
}

#[test]
fn headless_harness_and_fixture_return_no_window_for_ui_host_commands() {
    assert_eq!(UI_HOST_COMMANDS, ["ui.state.query", "ui.screenshot.capture"]);
    assert!(!is_ui_host_command("dtool.preview"));
    let mut fixture = TestFixture::new();
    assert_eq!(fixture.kind(), "headless");
    let before = fixture.session().map(|session| session.generation);
    assert_no_window(fixture.run_command(Command::new("ui.state.query")));
    assert_no_window(fixture.run_command(Command::with_params("ui.screenshot.capture", json!({"viewport": "root"}))));
    assert_eq!(fixture.session().map(|session| session.generation), before);
    assert!(fixture.app_status().is_ok());
    drop(fixture);
    let profile = TestProfile::new();
    let mut in_process = InProcessHarness::new(&profile);
    assert_no_window(in_process.run_command(Command::new("ui.screenshot.capture")));
    assert!(in_process.app_status().is_ok());
    let mut workspace = Workspace::default();
    workspace.new_document("Untitled");
    let mut shared = InProcessHarness::from_shared(workspace.into_shared());
    assert_no_window(shared.run_command(Command::new("ui.state.query")));
}

#[test]
fn cli_args_return_no_window_for_ui_host_commands() {
    let mut workspace = Workspace::default();
    workspace.new_document("Untitled");
    let shared = workspace.into_shared();
    for name in UI_HOST_COMMANDS {
        assert_no_window(run_cli_args(&shared, &[name.to_string()]));
        assert_no_window(run_cli_args(&shared, &["run".to_string(), name.to_string(), "{}".to_string()]));
    }
    assert!(run_cli_args(&shared, &["app.status".to_string()]).is_ok());
}

#[test]
fn cli_binary_prints_no_window_for_screenshot_capture() {
    let profile = TestProfile::new();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_dtool-cli"))
            .args(args)
            .env("APPDATA", profile.path())
            .env("LOCALAPPDATA", profile.path())
            .output()
            .expect("dtool-cli")
    };
    for name in UI_HOST_COMMANDS {
        let output = run(&["run", name, "{}"]);
        assert_eq!(output.status.code(), Some(1));
        let reply: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(reply["error"]["code"], "NO_WINDOW");
        assert_eq!(reply["error"]["key"], "error.no_window");
    }
    let output = run(&["run", "app.status"]);
    assert!(output.status.success());
}