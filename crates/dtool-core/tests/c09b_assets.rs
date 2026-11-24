use dtool_core::command::Session;
use dtool_core::commands::assets::{library_path, load_library};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_protocol::Command;
use serde_json::{json, Value};

fn run(s: &mut Session, name: &str, params: Value) -> Result<Value, dtool_protocol::CommandError> {
    s.run(&Command::with_params(name, params))
}

fn doc_with_group() -> (Session, ObjectId, ObjectId, ObjectId) {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let a = s.document.create_object(Object { name: "A".into(), pos: [10.0, 20.0], size: [10.0, 10.0], ..Object::default() });
    let b = s.document.create_object(Object { name: "B".into(), pos: [30.0, 20.0], size: [10.0, 10.0], ..Object::default() });
    let group = run(&mut s, "node.group", json!({ "nodes": [a.0, b.0], "name": "Pair" })).unwrap()["group"].as_u64().unwrap();
    (s, ObjectId(group), a, b)
}

#[test]
fn asset_library_is_app_level_persisted_under_appdata() {
    let profile = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09b_assets_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile).unwrap();
    unsafe { std::env::set_var("APPDATA", &profile) };
    let path = library_path().unwrap();
    assert_eq!(path, profile.join("DTool").join("assets").join("library.json"));
    assert!(!path.exists());

    let (mut first, group, _, _) = doc_with_group();
    run(&mut first, "asset.add_category", json!({ "name": "Shapes" })).unwrap();
    run(&mut first, "asset.add_group", json!({ "category": "Shapes", "name": "Basic" })).unwrap();
    assert!(run(&mut first, "asset.add_group", json!({ "category": "Nope", "name": "x" })).is_err());
    first.selection.ids = vec![group];
    let added = run(&mut first, "asset.add", json!({ "category": "Shapes", "group": "Basic", "name": "Pair asset" })).unwrap();
    let asset_id = added["id"].as_u64().unwrap();
    assert_eq!(added["nodes"], json!(3));
    assert!(path.is_file());
    let stored = load_library(&path);
    assert_eq!(stored.assets.len(), 1);
    assert_eq!(stored.categories[0].groups, vec!["Basic".to_string()]);

    let mut other = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let listed = run(&mut other, "asset.list", json!({})).unwrap();
    assert_eq!(listed["assets"][0]["name"], json!("Pair asset"));
    let placed = run(&mut other, "asset.place", json!({ "asset_id": asset_id, "pos": [100.0, 100.0] })).unwrap();
    let ids: Vec<ObjectId> = placed["ids"].as_array().unwrap().iter().map(|v| ObjectId(v.as_u64().unwrap())).collect();
    assert_eq!(ids.len(), 3);
    let placed_group = ids.iter().copied().find(|id| other.document.get_object(*id).unwrap().parent_id.is_none()).unwrap();
    let children: Vec<&Object> = other.document.objects.iter().filter(|o| o.parent_id == Some(placed_group)).collect();
    assert_eq!(children.len(), 2);
    let min_x = children.iter().map(|o| o.pos[0]).fold(f32::INFINITY, f32::min);
    let min_y = children.iter().map(|o| o.pos[1]).fold(f32::INFINITY, f32::min);
    assert!((min_x - 100.0).abs() < 0.01 && (min_y - 100.0).abs() < 0.01, "{min_x},{min_y}");
    assert_eq!(other.selection.ids, vec![placed_group]);
    run(&mut other, "edit.undo", json!({})).unwrap();
    assert!(other.document.objects.is_empty());
    run(&mut other, "edit.redo", json!({})).unwrap();
    assert_eq!(other.document.objects.len(), 3);

    run(&mut other, "asset.delete", json!({ "asset_id": asset_id })).unwrap();
    assert!(load_library(&path).assets.is_empty());
    assert!(run(&mut other, "asset.place", json!({ "asset_id": asset_id })).is_err());
    assert_eq!(load_library(&path).categories.len(), 1);
    let _ = std::fs::remove_dir_all(&profile);
}