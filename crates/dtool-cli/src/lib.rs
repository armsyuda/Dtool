pub mod harness;

pub use harness::{
    create_in_process_test_fixture,
    create_http_test_fixture,
    create_stdio_test_fixture,
    create_test_fixture,
    create_test_profile,
    HeadlessHarness,
    InProcessHarness,
    InProcessTestFixture,
    HARNESS_KIND,
    HARNESS_ROUTE,
    PROFILE_VARS,
    HttpHarness,
    HttpTestFixture,
    StdioHarness,
    StdioTestFixture,
    TestFixture,
    TestProfile,
};


pub fn install_hosts() -> bool {
    dtool_io::install_import_commands();
    dtool_render::preview::install();
    dtool_text::text_host::install()
}

pub fn run_cli_args(workspace: &dtool_core::workspace::SharedWorkspace, args: &[String]) -> dtool_protocol::CommandResult {
    install_hosts();
    harness::run_cli_args(workspace, args)
}

pub fn create_headless_harness(profile: &TestProfile) -> HeadlessHarness {
    install_hosts();
    harness::create_headless_harness(profile)
}

pub fn create_in_process_harness(profile: &TestProfile) -> InProcessHarness {
    install_hosts();
    harness::create_in_process_harness(profile)
}

pub fn create_stdio_harness(profile: &TestProfile) -> StdioHarness {
    install_hosts();
    harness::create_stdio_harness(profile)
}

pub fn create_http_harness(profile: &TestProfile) -> HttpHarness {
    install_hosts();
    harness::create_http_harness(profile)
}
