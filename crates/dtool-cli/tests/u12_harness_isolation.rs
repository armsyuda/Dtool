use dtool_cli::harness::{HeadlessHarness, InProcessHarness, TestFixture, TestProfile};
use dtool_core::prefs::{Prefs, CMD_SET};
use dtool_core::Workspace;
use dtool_protocol::Command;
use serde_json::json;

#[test]
fn u12_profiles_do_not_share_app_data() {
    let real = std::env::var_os("APPDATA").map(std::path::PathBuf::from).map(|dir| dir.join("DTool").join("prefs.json"));
    let real_before = real.as_ref().and_then(|path| std::fs::read(path).ok());

    let first = TestFixture::new();
    let first_dir = first.profile_path().to_path_buf();
    let first_prefs = Prefs::default_path();
    assert!(first_prefs.starts_with(&first_dir));
    let mut prefs = Prefs::load(&first_prefs);
    prefs.run(CMD_SET, &json!({ "recent_count": 3 }), &first_prefs).unwrap();
    assert_eq!(Prefs::load(&first_prefs).recent_count, 3);
    drop(first);

    let second = TestFixture::new();
    assert_ne!(second.profile_path(), first_dir.as_path());
    let second_prefs = Prefs::default_path();
    assert!(second_prefs.starts_with(second.profile_path()));
    assert!(!second_prefs.exists());
    assert_eq!(Prefs::load(&second_prefs).recent_count, Prefs::default().recent_count);
    drop(second);

    if let Some(path) = real.as_ref() {
        assert_eq!(std::fs::read(path).ok(), real_before);
    }
}

#[test]
fn u12_shared_harness_binds_the_profile() {
    let profile = TestProfile::new();
    let mut workspace = Workspace::default();
    workspace.new_document("u12-shared");
    let shared = workspace.into_shared();
    let mut headless = HeadlessHarness::from_shared_in(std::sync::Arc::clone(&shared), &profile);
    assert_eq!(headless.profile_path(), Some(profile.path()));
    assert_eq!(headless.kind(), "headless");
    assert!(headless.run_command(Command::new("app.status")).is_ok());
    let mut in_process = InProcessHarness::from_shared_in(shared, &profile);
    assert_eq!(in_process.profile_path(), Some(profile.path()));
    assert_eq!(in_process.route(), "in_process");
    assert!(in_process.app_status().is_ok());
    assert!(HeadlessHarness::from_shared(in_process.shared()).profile_path().is_none());
}
