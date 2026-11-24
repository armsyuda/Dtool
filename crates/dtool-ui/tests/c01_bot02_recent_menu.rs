use dtool_core::{SharedServices, Workspace};
use dtool_ui::menu_tree::recent::{self, RecentFiles, RecentOutcome, CLEAR_ID, MENU_ID};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn workspace() -> Workspace {
    Workspace::new(Arc::new(SharedServices {}))
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("c01_bot02_recent").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn find<'a>(tree: &'a Value, id: &str) -> Option<&'a Value> {
    match tree {
        Value::Array(items) => items.iter().find_map(|item| find(item, id)),
        Value::Object(map) => {
            if map.get("id").and_then(Value::as_str) == Some(id) {
                return Some(tree);
            }
            map.get("children").and_then(|children| find(children, id))
        }
        _ => None,
    }
}

#[test]
fn recent_list_round_trips_through_preferences_file() {
    let dir = scratch("round_trip");
    let file = dir.join(recent::FILE_NAME);
    let list = vec![PathBuf::from("b.dtool"), PathBuf::from("a.dtool")];
    recent::save(&file, &list).unwrap();
    assert_eq!(recent::load(&file), list);
    assert!(recent::load(&dir.join("absent.json")).is_empty());
    std::fs::write(dir.join("broken.json"), "not json").unwrap();
    assert!(recent::load(&dir.join("broken.json")).is_empty());
    assert_eq!(recent::default_path().file_name().and_then(|n| n.to_str()), Some(recent::FILE_NAME));
    assert_eq!(recent::default_path().parent(), dtool_core::prefs::Prefs::default_path().parent());
}

#[test]
fn sync_persists_and_restore_keeps_order() {
    let dir = scratch("sync");
    let file = dir.join(recent::FILE_NAME);
    let mut ws = workspace();
    let mut store = RecentFiles::at(file.clone());
    store.restore_into(&mut ws);
    assert!(ws.recent().is_empty());
    assert!(!store.sync(&ws));
    ws.push_recent(Path::new("one.dtool"));
    ws.push_recent(Path::new("two.dtool"));
    assert!(store.sync(&ws));
    assert!(!store.sync(&ws));
    assert_eq!(recent::load(&file), vec![PathBuf::from("two.dtool"), PathBuf::from("one.dtool")]);
    let mut fresh = workspace();
    let mut reloaded = RecentFiles::at(file.clone());
    reloaded.restore_into(&mut fresh);
    assert_eq!(fresh.recent(), ws.recent());
    assert_eq!(reloaded.handle(&mut fresh, CLEAR_ID).unwrap(), RecentOutcome::Cleared);
    assert!(fresh.recent().is_empty());
    assert!(reloaded.sync(&fresh));
    assert!(recent::load(&file).is_empty());
}

#[test]
fn open_recent_submenu_lists_entries_before_clear() {
    let list = vec![PathBuf::from("dir").join("first.dtool"), PathBuf::from("second.dtool")];
    let tree = dtool_ui::menu_tree::queried_menu_tree(None, "pixel", &list);
    let menu = find(&tree, MENU_ID).expect("open recent row");
    let rows = menu["children"].as_array().unwrap();
    assert_eq!(rows[0]["id"], recent::item_id(0));
    assert_eq!(rows[0]["label"], "1 first.dtool");
    assert_eq!(rows[0]["enabled"], true);
    assert_eq!(rows[0]["planned"], false);
    assert_eq!(rows[1]["id"], recent::item_id(1));
    assert_eq!(rows[1]["label"], "2 second.dtool");
    let clear = find(&tree, CLEAR_ID).unwrap();
    assert_eq!(clear["enabled"], true);
    assert_eq!(recent::item_index(&recent::item_id(7)), Some(7));
    assert_eq!(recent::item_index(CLEAR_ID), None);
    let empty = dtool_ui::menu_tree::queried_menu_tree(None, "pixel", &[]);
    let rows = find(&empty, MENU_ID).unwrap()["children"].as_array().unwrap().clone();
    assert!(rows.iter().all(|row| row["id"].as_str().and_then(recent::item_index).is_none()));
    assert_eq!(find(&empty, CLEAR_ID).unwrap()["enabled"], false);
    let mut again = tree.clone();
    recent::inject_rows(&mut again, &list[..1]);
    let count = find(&again, MENU_ID).unwrap()["children"].as_array().unwrap().iter().filter(|row| row["id"].as_str().and_then(recent::item_index).is_some()).count();
    assert_eq!(count, 1);
}

#[test]
fn missing_file_prompts_and_answer_decides_removal() {
    let dir = scratch("missing");
    let gone = dir.join("gone.dtool");
    let mut ws = workspace();
    ws.push_recent(&gone);
    let mut store = RecentFiles::default();
    assert_eq!(store.handle(&mut ws, &recent::item_id(0)).unwrap(), RecentOutcome::Missing(gone.clone()));
    assert_eq!(store.missing.as_ref().unwrap().path, gone.to_string_lossy());
    assert_eq!(ws.recent().len(), 1);
    assert!(ws.active().is_none());
    store.resolve_missing(&mut ws, false);
    assert!(store.missing.is_none());
    assert_eq!(ws.recent().len(), 1);
    store.open(&mut ws, 0).unwrap();
    store.resolve_missing(&mut ws, true);
    assert!(ws.recent().is_empty());
    assert_eq!(store.open(&mut ws, 3).unwrap(), RecentOutcome::Ignored);
    assert_eq!(store.handle(&mut ws, "file.open").unwrap(), RecentOutcome::Ignored);
}

#[test]
fn missing_prompt_renders_until_answered() {
    let mut ws = workspace();
    ws.push_recent(Path::new("nowhere_c01_bot02.dtool"));
    let mut store = RecentFiles::default();
    store.open(&mut ws, 0).unwrap();
    let ctx = egui::Context::default();
    let mut answer = Some(true);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| answer = store.show(ui, &mut ws));
    output.textures_delta.clear();
    assert_eq!(answer, None);
    assert!(store.missing.is_some());
    let mut idle = RecentFiles::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| answer = idle.show(ui, &mut ws));
    output.textures_delta.clear();
    assert_eq!(answer, None);
}