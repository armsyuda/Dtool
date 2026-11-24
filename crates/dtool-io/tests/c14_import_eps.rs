use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::json;

#[test]
fn import_eps_opens_a_context_and_missing_file_changes_nothing() {
    dtool_io::import_commands::install();
    let dir = std::env::temp_dir().join(format!("c14_eps_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiny.eps");
    std::fs::write(&path, "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 72 72\n%%EndComments\nnewpath 10 10 moveto 60 10 lineto 60 60 lineto closepath fill\nshowpage\n%%EOF\n").unwrap();
    let mut ws = Workspace::default();
    let (id, result) = ws.run(Scope::active(), Command::with_params("import.eps", json!({"path": path.to_string_lossy()})));
    let value = result.unwrap();
    assert_eq!(value["format"], json!("eps"));
    let id = id.unwrap();
    assert!(ws.get(id).is_some());
    let count = ws.contexts().len();
    let (none, err) = ws.run(Scope::active(), Command::with_params("import.eps", json!({"path": dir.join("missing.eps").to_string_lossy()})));
    assert!(none.is_none());
    assert!(err.is_err());
    assert_eq!(ws.contexts().len(), count);
    assert!(dtool_protocol::CommandCatalog::default().get("import.eps").is_some_and(|spec| spec.implemented));
}