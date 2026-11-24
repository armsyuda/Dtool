use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use dtool_core::command::Session;
use dtool_core::workspace::{run_shared, Scope, SharedWorkspace, Workspace};
use dtool_protocol::{Command, CommandResult};

pub const HARNESS_KIND: &str = "headless";
pub const HARNESS_ROUTE: &str = "in_process";
pub const PROFILE_VARS: [&str; 2] = ["APPDATA", "LOCALAPPDATA"];
pub const UI_HOST_COMMANDS: [&str; 2] = ["ui.state.query", "ui.screenshot.capture"];

pub fn is_ui_host_command(name: &str) -> bool {
    UI_HOST_COMMANDS.contains(&name)
}

static PROFILE_LOCK: Mutex<()> = Mutex::new(());

pub struct TestProfile {
    dir: PathBuf,
    previous: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl TestProfile {
    pub fn new() -> Self {
        let guard = PROFILE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dtool-test-{}-{}", std::process::id(), id));
        std::fs::create_dir_all(&dir).expect("failed to create test profile dir");
        let previous = PROFILE_VARS.iter().map(|name| (*name, std::env::var_os(name))).collect();
        for name in PROFILE_VARS {
            unsafe { std::env::set_var(name, &dir) };
        }
        Self { dir, previous, _guard: guard }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn is_applied(&self) -> bool {
        PROFILE_VARS.iter().all(|name| std::env::var_os(name).is_some_and(|value| Path::new(&value) == self.dir.as_path()))
    }
}

impl Drop for TestProfile {
    fn drop(&mut self) {
        for (name, value) in self.previous.drain(..) {
            match value {
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct HeadlessHarness {
    workspace: Option<Workspace>,
    shared_workspace: Option<SharedWorkspace>,
    profile_dir: Option<PathBuf>,
}

impl HeadlessHarness {
    pub fn new(profile: &TestProfile) -> Self {
        assert!(profile.is_applied(), "test profile is not the active app-data folder");
        let mut workspace = Workspace::new(Arc::new(dtool_core::SharedServices::default()));
        workspace.new_document("Untitled");
        Self { workspace: Some(workspace), shared_workspace: None, profile_dir: Some(profile.path().to_path_buf()) }
    }

    pub fn kind(&self) -> &'static str {
        HARNESS_KIND
    }

    pub fn route(&self) -> &'static str {
        HARNESS_ROUTE
    }

    pub fn profile_path(&self) -> Option<&Path> {
        self.profile_dir.as_deref()
    }

    pub fn run_command(&mut self, command: Command) -> CommandResult {
        if is_ui_host_command(&command.name) {
            return Err(dtool_protocol::CommandError::no_window());
        }
        match &mut self.workspace {
            Some(workspace) => workspace.run(Scope::active(), command).1,
            None => run_shared(self.shared_workspace.as_ref().expect("shared workspace"), Scope::active(), command).1,
        }
    }

    pub fn app_status(&mut self) -> CommandResult {
        self.run_command(Command::new("app.status"))
    }

    pub fn session(&self) -> Option<&Session> {
        self.workspace.as_ref().and_then(Workspace::session)
    }

    pub fn workspace(&self) -> &Workspace {
        self.workspace.as_ref().expect("borrowed workspace accessor is owned-mode only; use with_workspace/shared in shared mode")
    }
}

pub struct InProcessHarness {
    harness: HeadlessHarness,
}

pub type StdioHarness = InProcessHarness;
pub type HttpHarness = InProcessHarness;

impl InProcessHarness {
    pub fn new(profile: &TestProfile) -> Self {
        Self {
            harness: HeadlessHarness::new(profile),
        }
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

    pub fn app_status(&mut self) -> CommandResult {
        self.harness.app_status()
    }

    pub fn from_shared(workspace: SharedWorkspace) -> Self { Self { harness: HeadlessHarness::from_shared(workspace) } }
    pub fn from_shared_in(workspace: SharedWorkspace, profile: &TestProfile) -> Self { Self { harness: HeadlessHarness::from_shared_in(workspace, profile) } }
    pub fn profile_path(&self) -> Option<&Path> { self.harness.profile_path() }
    pub fn shared(&mut self) -> SharedWorkspace { self.harness.shared() }
}

pub struct TestFixture {
    harness: HeadlessHarness,
    profile: TestProfile,
}

impl TestFixture {
    pub fn new() -> Self {
        let profile = TestProfile::new();
        let harness = HeadlessHarness::new(&profile);
        Self { harness, profile }
    }

    pub fn kind(&self) -> &'static str {
        self.harness.kind()
    }

    pub fn run_command(&mut self, command: Command) -> CommandResult {
        self.harness.run_command(command)
    }

    pub fn app_status(&mut self) -> CommandResult {
        self.harness.app_status()
    }

    pub fn profile_path(&self) -> &Path {
        self.profile.path()
    }

    pub fn session(&self) -> Option<&Session> {
        self.harness.session()
    }

    pub fn workspace(&self) -> &Workspace {
        self.harness.workspace()
    }
}

pub struct InProcessTestFixture {
    harness: InProcessHarness,
    profile: TestProfile,
}

pub type StdioTestFixture = InProcessTestFixture;
pub type HttpTestFixture = InProcessTestFixture;

impl InProcessTestFixture {
    pub fn new() -> Self {
        let profile = TestProfile::new();
        let harness = InProcessHarness::new(&profile);
        Self { harness, profile }
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

    pub fn app_status(&mut self) -> CommandResult {
        self.harness.app_status()
    }

    pub fn profile_path(&self) -> &Path {
        self.profile.path()
    }
}

pub fn create_test_profile() -> TestProfile {
    TestProfile::new()
}

pub fn create_headless_harness(profile: &TestProfile) -> HeadlessHarness {
    HeadlessHarness::new(profile)
}

pub fn create_in_process_harness(profile: &TestProfile) -> InProcessHarness {
    InProcessHarness::new(profile)
}

pub fn create_stdio_harness(profile: &TestProfile) -> InProcessHarness {
    InProcessHarness::new(profile)
}

pub fn create_http_harness(profile: &TestProfile) -> InProcessHarness {
    InProcessHarness::new(profile)
}

pub fn create_test_fixture() -> TestFixture {
    TestFixture::new()
}

pub fn create_in_process_test_fixture() -> InProcessTestFixture {
    InProcessTestFixture::new()
}

pub fn create_stdio_test_fixture() -> InProcessTestFixture {
    InProcessTestFixture::new()
}

pub fn create_http_test_fixture() -> InProcessTestFixture {
    InProcessTestFixture::new()
}

impl HeadlessHarness {
    pub fn from_shared(workspace: SharedWorkspace) -> Self { Self { workspace: None, shared_workspace: Some(workspace), profile_dir: None } }
    pub fn from_shared_in(workspace: SharedWorkspace, profile: &TestProfile) -> Self {
        assert!(profile.is_applied(), "test profile is not the active app-data folder");
        Self { workspace: None, shared_workspace: Some(workspace), profile_dir: Some(profile.path().to_path_buf()) }
    }
    pub fn shared(&mut self) -> SharedWorkspace {
        if self.shared_workspace.is_none() { self.shared_workspace = Some(self.workspace.take().expect("owned workspace").into_shared()); }
        Arc::clone(self.shared_workspace.as_ref().expect("shared workspace"))
    }
    pub fn with_workspace<T>(&self, read: impl FnOnce(&Workspace) -> T) -> Result<T, dtool_protocol::CommandError> {
        match &self.workspace {
            Some(workspace) => Ok(read(workspace)),
            None => self.shared_workspace.as_ref().expect("shared workspace").lock().map(|workspace| read(&workspace)).map_err(|_| dtool_protocol::CommandError::new("INTERNAL_ERROR")),
        }
    }
    pub fn with_workspace_mut<T>(&mut self, edit: impl FnOnce(&mut Workspace) -> T) -> Result<T, dtool_protocol::CommandError> {
        match &mut self.workspace {
            Some(workspace) => Ok(edit(workspace)),
            None => self.shared_workspace.as_ref().expect("shared workspace").lock().map(|mut workspace| edit(&mut workspace)).map_err(|_| dtool_protocol::CommandError::new("INTERNAL_ERROR")),
        }
    }
}
pub fn run_cli_args(workspace: &SharedWorkspace, args: &[String]) -> CommandResult {
    let first = args.first().ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    if first == "catalog" {
        return workspace.lock().map_err(|_| dtool_protocol::CommandError::new("INTERNAL_ERROR"))?
            .session().map(|session| serde_json::json!({ "commands": session.catalog.names() })).ok_or_else(dtool_protocol::CommandError::no_document);
    }
    let offset = usize::from(first == "run");
    let name = args.get(offset).filter(|name| !name.is_empty()).ok_or_else(dtool_protocol::CommandError::invalid_params)?.clone();
    let params = args.get(offset + 1).map(|value| serde_json::from_str(value)).transpose().map_err(|_| dtool_protocol::CommandError::invalid_params())?.unwrap_or(serde_json::Value::Null);
    if is_ui_host_command(&name) {
        return Err(dtool_protocol::CommandError::no_window());
    }
    run_shared(workspace, Scope::active(), Command { name, params }).1
}
