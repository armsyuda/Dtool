use dtool_core::document::{Document, Units};
use dtool_core::snap_prefs::{set_thread_path, SnapPrefs, SnapPrefsService};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, Session};
use serde_json::json;
use std::path::PathBuf;

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dtool_f3j_snap_{}_{}", tag, std::process::id())).join("snap_prefs.json")
}

#[test]
fn thread_path_redirects_sessions_and_writes() {
    let path = temp_path("thread");
    set_thread_path(Some(path.clone()));
    assert_eq!(SnapPrefs::active_path(), path);
    let mut s = Session::new(Document::new([100.0, 100.0], Units::Px, 72.0, 1));
    assert_eq!(s.snap_prefs.path(), path.as_path());
    let before = s.snap_prefs.query().enabled;
    s.run_named("snap.toggle", json!({})).unwrap();
    assert_eq!(SnapPrefs::load(path.clone()).enabled, !before);
    let reloaded = SnapPrefsService::new();
    assert_eq!(reloaded.query().enabled, !before);
    set_thread_path(None);
    assert_eq!(SnapPrefs::active_path(), SnapPrefs::default_path());
}

#[test]
fn workspace_contexts_use_thread_path() {
    let path = temp_path("workspace");
    set_thread_path(Some(path.clone()));
    let mut workspace = Workspace::default();
    let first = workspace.new_document("a");
    let second = workspace.new_document("b");
    let (_, result) = workspace.run(Scope::of(first), Command::with_params("snap.prefs.set", json!({"screen_tolerance": 23})));
    result.unwrap();
    assert_eq!(SnapPrefs::load(path.clone()).screen_tolerance, 23);
    assert_eq!(workspace.get(second).unwrap().session.snap_prefs.query().screen_tolerance, 23);
    assert_eq!(workspace.get(second).unwrap().session.snap_prefs.path(), path.as_path());
    set_thread_path(None);
}

#[test]
fn override_is_per_thread_and_with_path_is_explicit() {
    set_thread_path(Some(temp_path("other")));
    let default = std::thread::spawn(SnapPrefs::active_path).join().unwrap();
    assert_eq!(default, SnapPrefs::default_path());
    set_thread_path(None);
    let explicit = temp_path("explicit");
    let mut service = SnapPrefsService::with_path(explicit.clone());
    assert_eq!(service.path(), explicit.as_path());
    service.set(serde_json::from_value(json!({"enabled": false})).unwrap()).unwrap();
    assert!(!SnapPrefs::load(explicit).enabled);
}
