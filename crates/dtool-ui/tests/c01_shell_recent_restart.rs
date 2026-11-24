#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::menu_tree::{self, recent::{self, RecentOutcome}};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn row(shell: &AppShell, index: usize) -> Value {
    let mut tree = menu_tree::queried_menu_tree(None, "pixel", shell.workspace.recent());
    let id = recent::item_id(index);
    fn find(tree: &mut Value, id: &str) -> Option<Value> {
        match tree {
            Value::Array(items) => items.iter_mut().find_map(|item| find(item, id)),
            Value::Object(map) => {
                if map.get("id").and_then(Value::as_str) == Some(id) { return Some(Value::Object(map.clone())); }
                map.get_mut("children").and_then(|children| find(children, id))
            }
            _ => None,
        }
    }
    find(&mut tree, &id).unwrap_or(Value::Null)
}

#[test]
fn c01_recent_files_survive_an_app_restart_and_reopen() {
    let home = test_home::enter("c01_shell_recent_restart");
    let docs = home.root.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let (a, b) = (docs.join("a.dtool"), docs.join("b.dtool"));
    let ctx = egui::Context::default();
    {
        let mut shell = AppShell::new();
        assert!(shell.workspace.recent().is_empty());
        for path in [&a, &b] {
            let doc = shell.workspace.new_document("recent");
            shell.workspace.activate(doc).unwrap();
            shell.workspace.run(Scope::active(), Command::with_params("doc.save_as", json!({ "path": path }))).1.unwrap();
            frame(&mut shell, &ctx);
        }
        assert_eq!(shell.workspace.recent()[0], b);
    }
    let mut shell = AppShell::new();
    frame(&mut shell, &ctx);
    assert_eq!(shell.workspace.recent().to_vec(), vec![b.clone(), a.clone()], "restart restores newest first");
    let first = row(&shell, 0);
    assert_eq!(first["path"], json!(b.to_string_lossy()));
    assert_eq!(first["missing"], json!(false));
    let tabs = shell.workspace.contexts().len();
    assert_eq!(shell.recent_files.handle(&mut shell.workspace, &recent::item_id(1)).unwrap(), RecentOutcome::Opened);
    frame(&mut shell, &ctx);
    assert_eq!(shell.workspace.contexts().len(), tabs + 1, "recent item opens a tab");

    assert_eq!(shell.workspace.recent().to_vec(), vec![a.clone(), b.clone()], "reopened file moves to the top");
    std::fs::remove_file(&b).unwrap();
    assert_eq!(row(&shell, 0)["missing"], json!(false));
    assert_eq!(row(&shell, 1)["missing"], json!(true), "a gone file is marked");
    assert_eq!(shell.recent_files.handle(&mut shell.workspace, &recent::item_id(1)).unwrap(), RecentOutcome::Missing(b.clone()));
    shell.recent_files.resolve_missing(&mut shell.workspace, true);
    frame(&mut shell, &ctx);
    drop(shell);
    let mut shell = AppShell::new();
    assert_eq!(shell.workspace.recent().to_vec(), vec![a.clone()], "removal survives the restart");
    assert_eq!(shell.recent_files.handle(&mut shell.workspace, recent::CLEAR_ID).unwrap(), RecentOutcome::Cleared);
    frame(&mut shell, &ctx);
    drop(shell);
    let shell = AppShell::new();
    assert!(shell.workspace.recent().is_empty(), "clear survives the restart");
}
