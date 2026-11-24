#[path = "support/test_home.rs"]
mod test_home;

use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};

fn run(ws: &mut Workspace, name: &str, params: Value) -> Value {
    let (_, result) = ws.run(Scope::active(), Command::with_params(name, params));
    result.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn listed(ws: &mut Workspace) -> Vec<String> {
    run(ws, "doc.recent.list", json!({}))["recent"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

fn persistent() -> Workspace {
    let mut ws = Workspace::default();
    ws.enable_recent_persistence(Workspace::recent_default_path());
    ws
}

#[test]
fn c01_recent_list_survives_restart_and_follows_remove_and_clear() {
    let home = test_home::enter("c01_recent_persist");
    let store = Workspace::recent_default_path();
    assert!(store.starts_with(&home.root), "{store:?} must be under the test home");
    assert_eq!(store.file_name().unwrap(), "recent.json");
    assert_eq!(store.parent(), dtool_core::prefs::Prefs::default_path().parent(), "same folder as prefs.json");

    let plain = Workspace::default();
    drop(plain);
    assert!(!store.exists(), "persistence is opt-in");

    let docs = home.root.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let (a, b) = (docs.join("a.dtool"), docs.join("b.dtool"));
    let mut ws = persistent();
    assert_eq!(ws.recent_store(), Some(store.as_path()));
    for path in [&a, &b] {
        ws.new_document("n");
        run(&mut ws, "doc.save_as", json!({ "path": path }));
        run(&mut ws, "doc.open", json!({ "path": path }));
    }
    let first = listed(&mut ws);
    assert_eq!(first.first().map(String::as_str), Some(b.display().to_string().as_str()), "{first:?}");
    assert!(first.contains(&a.display().to_string()));
    assert!(store.exists());
    drop(ws);

    let mut again = persistent();
    assert_eq!(listed(&mut again), first, "list restored after restart");
    let removed = run(&mut again, "doc.recent.remove", json!({ "path": a }));
    assert_eq!(removed["removed"], true);
    drop(again);

    let mut third = persistent();
    assert_eq!(listed(&mut third), vec![b.display().to_string()]);
    run(&mut third, "doc.recent.clear", json!({}));
    drop(third);

    let mut fourth = persistent();
    assert!(listed(&mut fourth).is_empty());

    std::fs::write(&store, "not json").unwrap();
    let mut broken = persistent();
    assert!(listed(&mut broken).is_empty(), "a broken store starts empty");
}