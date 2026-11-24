use dtool_core::workspace::{SharedWorkspace, Workspace};
use dtool_protocol::{Command, CommandResult};
use dtool_ui::headless::{
    create_headless_ui_harness, create_headless_ui_test_fixture, is_ui_host_command, run_headless, HeadlessScreenshotHelper, HEADLESS_KIND, HEADLESS_ROUTE,
    UI_HOST_COMMANDS,
};
use serde_json::json;

fn shared_with_document() -> SharedWorkspace {
    let mut workspace = Workspace::default();
    workspace.new_document("Untitled");
    workspace.into_shared()
}

fn generation(workspace: &SharedWorkspace) -> Option<u64> {
    workspace.lock().expect("workspace").session().map(|session| session.generation)
}

fn assert_no_window(result: CommandResult) {
    let error = result.expect_err("headless must not answer a UI host command");
    assert_eq!(error.code, "NO_WINDOW");
    assert_eq!(error.message_key, "error.no_window");
}

#[test]
fn ui_host_commands_are_exactly_state_and_screenshot() {
    assert_eq!(UI_HOST_COMMANDS, ["ui.state.query", "ui.screenshot.capture"]);
    assert!(is_ui_host_command("ui.state.query"));
    assert!(is_ui_host_command("ui.screenshot.capture"));
    assert!(!is_ui_host_command("dtool.preview"));
    assert!(!is_ui_host_command("ui.commands.list"));
    assert!(!is_ui_host_command("app.status"));
}

#[test]
fn headless_harness_returns_no_window_for_state_and_capture() {
    let shared = shared_with_document();
    let before = generation(&shared);
    assert!(before.is_some());
    let mut harness = create_headless_ui_harness(shared.clone());
    assert_eq!(harness.kind(), HEADLESS_KIND);
    assert_eq!(harness.route(), HEADLESS_ROUTE);
    assert_no_window(harness.query_state());
    assert_no_window(harness.capture_screenshot());
    assert_no_window(harness.run_command(Command::with_params("ui.screenshot.capture", json!({"viewport": "root", "max_px": 256}))));
    assert_no_window(harness.run_command(Command::with_params("ui.state.query", json!({"context": 1}))));
    assert_eq!(generation(&shared), before);
}

#[test]
fn headless_fixture_and_screenshot_helper_never_produce_ui_replies() {
    let shared = shared_with_document();
    let mut fixture = create_headless_ui_test_fixture(shared.clone());
    assert_eq!(fixture.kind(), "headless");
    assert_eq!(fixture.route(), "in_process");
    assert_no_window(fixture.query_state());
    assert_no_window(fixture.capture_screenshot());
    let mut helper = HeadlessScreenshotHelper::new(shared.clone());
    assert_eq!(helper.kind(), "headless");
    assert_no_window(helper.capture());
    assert_no_window(run_headless(&shared, Command::new("ui.screenshot.capture")));
    assert_no_window(run_headless(&shared, Command::new("ui.state.query")));
}

#[test]
fn headless_still_runs_document_commands_on_the_shared_workspace() {
    let shared = shared_with_document();
    let mut harness = create_headless_ui_harness(shared.clone());
    assert!(harness.run_command(Command::with_params("app.about", json!({}))).is_ok());
    assert!(run_headless(&shared, Command::new("app.status")).is_ok());
    let reply = harness.run_command(Command::new("app.status")).expect("app.status");
    assert_ne!(reply.get("kind").and_then(|kind| kind.as_str()), Some("ui_state"));
    assert_ne!(reply.get("kind").and_then(|kind| kind.as_str()), Some("ui_screenshot"));
}

#[test]
fn headless_no_window_without_a_document() {
    let shared = Workspace::default().into_shared();
    let mut harness = create_headless_ui_harness(shared);
    assert_no_window(harness.capture_screenshot());
    assert_no_window(harness.query_state());
}

#[test]
fn no_window_message_key_is_in_the_string_table() {
    assert!(dtool_strings::has("error.no_window"));
}