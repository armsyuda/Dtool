use dtool_io::native::{open, save, IoErrorKind};
use dtool_io::RecentList;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_b03_{}_{}", std::process::id(), name));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_open_live_path() {
    let dir = scratch("live");
    let path = dir.join("live.dtool");
    let doc = dtool_core::Document::new([100.0, 100.0], dtool_core::document::Units::Px, 72.0, 1);
    save(&doc, &path).unwrap();
    let opened = open(&path).unwrap();
    assert_eq!(opened.version, doc.version);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_open_missing_path_does_not_create_context() {
    let dir = scratch("missing");
    let path = dir.join("missing.dtool");
    let result = open(&path);
    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e.kind, IoErrorKind::NotFound);
    }
    assert!(!path.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_recent_list_yes_removes_entry() {
    let missing_path = scratch("yes").join("gone.dtool");
    let mut list = RecentList::new();
    list.push(&missing_path);
    assert!(list.paths().contains(&missing_path));
    list.remove(&missing_path);
    assert!(!list.paths().contains(&missing_path));
    assert!(list.is_empty());
}

#[test]
fn test_recent_list_no_keeps_entry() {
    let missing_path = scratch("no").join("gone.dtool");
    let mut list = RecentList::new();
    list.push(&missing_path);
    let kept = list.clone();
    assert!(kept.paths().contains(&missing_path));
    assert_eq!(kept.paths().first(), Some(&missing_path));
}

#[test]
fn test_recent_list_push_moves_to_front_and_caps() {
    let dir = scratch("cap");
    let mut list = RecentList::new();
    for i in 0..12 {
        list.push(&dir.join(format!("{i}.dtool")));
    }
    list.push(&dir.join("5.dtool"));
    assert_eq!(list.paths().len(), 10);
    assert_eq!(list.paths()[0], dir.join("5.dtool"));
    assert_eq!(list.paths().iter().filter(|path| **path == dir.join("5.dtool")).count(), 1);
}