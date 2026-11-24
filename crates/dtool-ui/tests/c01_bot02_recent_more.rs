use dtool_core::{SharedServices, Workspace};
use dtool_ui::menu_tree::recent::{self, RecentFiles, RecentOutcome, INLINE_ROWS, MENU_ID, MORE_ID};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

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

fn item_ids(rows: &Value) -> Vec<usize> {
    rows.as_array().unwrap().iter().filter_map(|row| row["id"].as_str().and_then(recent::item_index)).collect()
}

#[test]
fn more_lists_the_full_recent_list_up_to_the_cap() {
    let mut ws = Workspace::new(Arc::new(SharedServices {}));
    ws.set_recent_count(20);
    for index in 0..25 {
        ws.push_recent(&PathBuf::from(format!("doc_{:02}.dtool", index)));
    }
    assert_eq!(ws.recent().len(), 20);
    let tree = dtool_ui::menu_tree::queried_menu_tree(None, "pixel", ws.recent());
    let inline = item_ids(&find(&tree, MENU_ID).unwrap()["children"]);
    assert_eq!(inline, (0..INLINE_ROWS).collect::<Vec<_>>());
    let more = find(&tree, MORE_ID).unwrap();
    assert_eq!(more["enabled"], true);
    assert_eq!(more["planned"], false);
    assert_eq!(item_ids(&more["children"]), (0..20).collect::<Vec<_>>());
    assert_eq!(more["children"][19]["label"], "20 doc_05.dtool");
    assert_eq!(more["children"][0]["label"], "1 doc_24.dtool");
    let mut store = RecentFiles::default();
    let id = more["children"][15]["id"].as_str().unwrap().to_string();
    assert_eq!(store.handle(&mut ws, &id).unwrap(), RecentOutcome::Missing(ws.recent()[15].clone()));
}

#[test]
fn more_is_disabled_and_empty_without_recent_files() {
    let tree = dtool_ui::menu_tree::queried_menu_tree(None, "pixel", &[]);
    let more = find(&tree, MORE_ID).unwrap();
    assert_eq!(more["enabled"], false);
    assert!(more["children"].as_array().unwrap().is_empty());
    let short = vec![PathBuf::from("a.dtool"), PathBuf::from("b.dtool")];
    let tree = dtool_ui::menu_tree::queried_menu_tree(None, "pixel", &short);
    assert_eq!(item_ids(&find(&tree, MORE_ID).unwrap()["children"]), vec![0, 1]);
    assert_eq!(item_ids(&find(&tree, MENU_ID).unwrap()["children"]), vec![0, 1]);
}

#[test]
fn more_submenu_renders() {
    let list: Vec<PathBuf> = (0..12).map(|index| PathBuf::from(format!("r{}.dtool", index))).collect();
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let _ = dtool_ui::menu_tree::show_queried_menu_bar_recent(ui, None, "pixel", &[], &list);
    });
    output.textures_delta.clear();
}