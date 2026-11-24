use dtool_cli::harness::{HeadlessHarness, InProcessHarness, InProcessTestFixture, TestFixture, TestProfile, HARNESS_KIND, HARNESS_ROUTE, PROFILE_VARS};
use dtool_core::autosave::recovery_dir;
use dtool_core::prefs::Prefs;
use dtool_protocol::Command;

#[test]
fn harness_runs_inside_its_temp_profile_and_is_labelled_headless_in_process() {
    let profile = TestProfile::new();
    assert!(profile.is_applied());
    assert!(profile.path().starts_with(std::env::temp_dir()));
    for name in PROFILE_VARS {
        assert_eq!(std::env::var_os(name).as_deref(), Some(profile.path().as_os_str()));
    }
    assert!(Prefs::default_path().starts_with(profile.path()));
    assert!(recovery_dir().starts_with(profile.path()));

    let mut headless = HeadlessHarness::new(&profile);
    assert_eq!(headless.profile_path(), Some(profile.path()));
    assert_eq!(headless.kind(), "headless");
    assert_eq!(headless.route(), "in_process");
    assert!(headless.app_status().is_ok());

    let mut in_process = InProcessHarness::new(&profile);
    assert_eq!(in_process.kind(), HARNESS_KIND);
    assert_eq!(in_process.route(), HARNESS_ROUTE);
    assert_ne!(in_process.route(), "stdio");
    assert_ne!(in_process.route(), "http");
    assert!(in_process.run_command(Command::new("app.status")).is_ok());
    drop(in_process);
    drop(headless);
    let dir = profile.path().to_path_buf();
    drop(profile);
    for name in PROFILE_VARS {
        assert_ne!(std::env::var_os(name).as_deref(), Some(dir.as_os_str()));
    }

    let fixture = TestFixture::new();
    assert_eq!(fixture.kind(), "headless");
    assert!(Prefs::default_path().starts_with(fixture.profile_path()));
    let dir = fixture.profile_path().to_path_buf();
    drop(fixture);
    assert_ne!(std::env::var_os("APPDATA").as_deref(), Some(dir.as_os_str()));

    let mut fixture = InProcessTestFixture::new();
    assert_eq!(fixture.route(), "in_process");
    assert!(recovery_dir().starts_with(fixture.profile_path()));
    assert!(fixture.app_status().is_ok());
}