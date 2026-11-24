#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::shell::AppShell;
use serde_json::json;

#[test]
fn c12c_threads_pref_is_removed() {
    let _home = test_home::enter("c12c_prefs_threads");
    assert!(!dtool_strings::has("pref.performance.threads"));
    assert!(!dtool_strings::has("pref.threads"));
    let mut shell = AppShell::new();
    assert!(shell.pref_command("pref.performance.set", &json!({ "threads": 4 })).is_err());
    let query = shell.pref_command("pref.query", &json!({})).expect("pref.query");
    assert!(query.get("threads").is_none(), "{query}");
    assert!(serde_json::to_value(&shell.prefs).unwrap().get("threads").is_none());
}