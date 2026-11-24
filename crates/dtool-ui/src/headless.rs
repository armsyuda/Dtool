use dtool_core::workspace::{run_shared, SharedWorkspace, Workspace};
use dtool_core::Scope;
use dtool_protocol::{Command, CommandError, CommandResult};

pub const HEADLESS_KIND: &str = "headless";
pub const HEADLESS_ROUTE: &str = "in_process";
pub const UI_HOST_COMMANDS: [&str; 2] = ["ui.state.query", "ui.screenshot.capture"];

pub fn is_ui_host_command(name: &str) -> bool {
    UI_HOST_COMMANDS.contains(&name)
}

pub fn run_headless(workspace: &SharedWorkspace, command: Command) -> CommandResult {
    crate::panels::session_route::install();
    if is_ui_host_command(&command.name) {
        return Err(CommandError::no_window());
    }
    run_shared(workspace, Scope::active(), command).1
}

pub fn run_on_workspace(workspace: &mut Workspace, command: Command) -> CommandResult {
    crate::panels::session_route::install();
    if is_ui_host_command(&command.name) {
        return Err(CommandError::no_window());
    }
    workspace.run(Scope::active(), command).1
}

pub struct HeadlessScreenshotHelper {
    workspace: SharedWorkspace,
}

impl HeadlessScreenshotHelper {
    pub fn new(workspace: SharedWorkspace) -> Self {
        Self { workspace }
    }

    pub fn kind(&self) -> &'static str {
        HEADLESS_KIND
    }

    pub fn capture(&mut self) -> CommandResult {
        self.run_command(Command::with_params("ui.screenshot.capture", serde_json::json!({})))
    }

    pub fn run_command(&mut self, command: Command) -> CommandResult {
        run_headless(&self.workspace, command)
    }
}

pub struct HeadlessUiHarness {
    workspace: SharedWorkspace,
}

impl HeadlessUiHarness {
    pub fn new(workspace: SharedWorkspace) -> Self {
        Self { workspace }
    }

    pub fn kind(&self) -> &'static str {
        HEADLESS_KIND
    }

    pub fn route(&self) -> &'static str {
        HEADLESS_ROUTE
    }

    pub fn run_command(&mut self, command: Command) -> CommandResult {
        run_headless(&self.workspace, command)
    }

    pub fn capture_screenshot(&mut self) -> CommandResult {
        self.run_command(Command::with_params("ui.screenshot.capture", serde_json::json!({})))
    }

    pub fn query_state(&mut self) -> CommandResult {
        self.run_command(Command::with_params("ui.state.query", serde_json::json!({})))
    }

    pub fn list_commands(&mut self) -> CommandResult {
        self.run_command(Command::with_params("ui.commands.list", serde_json::json!({})))
    }
}

pub struct HeadlessUiTestFixture {
    harness: HeadlessUiHarness,
}

impl HeadlessUiTestFixture {
    pub fn new(workspace: SharedWorkspace) -> Self {
        Self { harness: HeadlessUiHarness::new(workspace) }
    }

    pub fn kind(&self) -> &'static str {
        self.harness.kind()
    }

    pub fn route(&self) -> &'static str {
        self.harness.route()
    }

    pub fn run_command(&mut self, command: Command) -> CommandResult {
        self.harness.run_command(command)
    }

    pub fn capture_screenshot(&mut self) -> CommandResult {
        self.harness.capture_screenshot()
    }

    pub fn query_state(&mut self) -> CommandResult {
        self.harness.query_state()
    }

    pub fn list_commands(&mut self) -> CommandResult {
        self.harness.list_commands()
    }
}

pub fn create_headless_ui_harness(workspace: SharedWorkspace) -> HeadlessUiHarness {
    HeadlessUiHarness::new(workspace)
}

pub fn create_headless_ui_test_fixture(workspace: SharedWorkspace) -> HeadlessUiTestFixture {
    HeadlessUiTestFixture::new(workspace)
}