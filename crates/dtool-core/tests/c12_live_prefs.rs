#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::commands::prefs_cmd;
use dtool_core::document::{Document, Units};
use dtool_core::Session;
use serde_json::json;

fn undo_count(session: &mut Session) -> usize {
    let mut count = 0;
    while session.undo.can_undo() {
        session.run_named("edit.undo", json!({})).expect("undo");
        count += 1;
    }
    count
}

#[test]
fn c12_pref_writes_apply_undo_cap_and_autosave_interval_to_the_live_session() {
    let home = test_home::enter("c12_live_prefs");
    assert!(prefs_cmd::install(home.root.join("prefs.json")));
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1));
    session.run_named("pref.performance.set", json!({ "undo_limit": 2 })).expect("pref.performance.set is routed");
    assert_eq!(session.undo.limit(), 2);
    for name in ["a", "b", "c"] {
        session.run_named("object.create", json!({ "name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0] })).unwrap();
    }
    assert_eq!(undo_count(&mut session), 2);

    session.run_named("pref.set", json!({ "recovery_interval_s": 0 })).expect("pref.set");
    assert!(!session.autosave.settings.is_active());
    session.run_named("pref.set", json!({ "recovery_interval_s": 120 })).expect("pref.set");
    assert!(session.autosave.settings.is_active());
    assert_eq!(session.autosave.settings.recovery_interval_s, 120);

    session.run_named("pref.set", json!({ "undo_limit": 0 })).expect("pref.set");
    assert_eq!(session.undo.limit(), 1);
    let queried = session.run_named("pref.query", json!({ "keys": ["undo_limit"] })).expect("query");
    assert!(queried.to_string().contains("undo_limit"), "{queried}");
}
