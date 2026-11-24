use dtool_core::document::{Document, Units};
use dtool_core::snap_prefs::{SnapPrefs, SnapPrefsSetArgs};
use dtool_core::{Command, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1))
}

fn add(s: &mut Session, name: &str) -> u64 {
    s.run(&Command::with_params("guides.add", json!({"kind": "Horizontal", "position": 10.0, "name": name}))).unwrap()["id"].as_u64().unwrap()
}

#[test]
fn guides_find_by_id_and_name() {
    let mut s = session();
    add(&mut s, "a");
    add(&mut s, "Title");
    add(&mut s, "b");
    let version = s.document.version;
    let undo = s.undo.can_undo();
    let r = s.run(&Command::with_params("guides.find", json!({"id": 2}))).unwrap();
    assert_eq!(r["count"], json!(1));
    assert_eq!(r["guides"][0]["id"], json!(2));
    let r = s.run(&Command::with_params("guides.find", json!({"name": " title "}))).unwrap();
    assert_eq!(r["guides"][0]["id"], json!(2));
    let r = s.run(&Command::with_params("guides.find", json!({"name": "it"}))).unwrap();
    assert_eq!(r["count"], json!(1));
    assert_eq!(s.run(&Command::with_params("guides.find", json!({"name": "zz"}))).unwrap()["count"], json!(0));
    assert_eq!(s.run(&Command::with_params("guides.find", json!({}))).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(s.document.version, version);
    assert_eq!(s.undo.can_undo(), undo);
    assert!(dtool_protocol::CommandCatalog::default().get("guides.find").is_some_and(|spec| spec.implemented && !spec.mutating));
}

#[test]
fn guide_ids_are_never_reused() {
    let mut s = session();
    for name in ["1", "2", "3"] { add(&mut s, name); }
    s.run(&Command::with_params("guides.delete", json!({"id": 3}))).unwrap();
    assert_eq!(add(&mut s, "x"), 4);
    s.run(&Command::with_params("guides.delete", json!({"id": 4}))).unwrap();
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(s.guides.get(dtool_core::guides::GuideId(4)).is_some());
    assert_eq!(add(&mut s, "y"), 5);
    let ids: Vec<u64> = s.guides.list().iter().map(|g| g.id.0).collect();
    assert_eq!(ids, vec![1, 2, 4, 5]);
    let old: dtool_core::guides::Guides = serde_json::from_value(json!({"items": [{"id": 7, "kind": "Vertical", "position": 1.0}], "show": true, "show_columns": true, "locked": false, "colour": null})).unwrap();
    assert_eq!(old.next_id().0, 8);
}

#[test]
fn snap_prefs_partial_file_and_range_checks() {
    let prefs: SnapPrefs = serde_json::from_value(json!({"enabled": false})).unwrap();
    assert!(!prefs.enabled);
    assert_eq!(prefs.screen_tolerance, SnapPrefs::default().screen_tolerance);
    let args = |v: serde_json::Value| serde_json::from_value::<SnapPrefsSetArgs>(v).unwrap();
    assert!(!args(json!({"screen_tolerance": 0})).is_valid());
    assert!(!args(json!({"max_candidates": 0})).is_valid());
    assert!(!args(json!({"candidates_mode": "nope"})).is_valid());
    assert!(args(json!({"screen_tolerance": 23, "max_candidates": 6})).is_valid());
    let mut s = session();
    let before = s.run(&Command::new("snap.prefs.query")).ok();
    assert_eq!(s.run(&Command::with_params("snap.prefs.set", json!({"screen_tolerance": 0}))).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(s.run(&Command::new("snap.prefs.query")).ok(), before);
}