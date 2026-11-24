use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::json;

const EPS: &str = "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 72 72\n%%EndComments\nnewpath 10 10 moveto 60 10 lineto 60 60 lineto closepath fill\nshowpage\n%%EOF\n";

#[test]
fn doc_open_eps_on_a_live_workspace_opens_a_new_context_and_bad_eps_changes_nothing() {
    dtool_io::import_commands::install();
    let dir = std::env::temp_dir().join(format!("c14_open_eps_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("shape.eps");
    std::fs::write(&good, EPS).unwrap();
    let bad = dir.join("broken.eps");
    std::fs::write(&bad, b"not postscript").unwrap();
    let mut ws = Workspace::default();
    let live = ws.new_document("Live");
    let live_before = serde_json::to_value(&ws.get(live).unwrap().session.document).unwrap();
    let (id, result) = ws.run(Scope::active(), Command::with_params("doc.open", json!({ "path": good.to_string_lossy() })));
    let value = result.expect("doc.open eps");
    assert_eq!(value["format"], json!("eps"));
    let id = id.expect("context");
    assert_ne!(id, live);
    assert_eq!(ws.active(), Some(id));
    let shape = ws.get(id).unwrap().session.document.objects.iter().find(|o| o.curve().is_some()).expect("eps path imported as a curve").clone();
    assert_eq!(shape.pos, [10.0, 12.0]);
    assert_eq!(shape.size, [50.0, 50.0]);
    assert_eq!(shape.curve().unwrap().contour.points.len(), 3);
    assert_eq!(value["report"]["objects"], json!(1));
    let count = ws.contexts().len();
    let active = ws.active();
    let (none, err) = ws.run(Scope::active(), Command::with_params("doc.open", json!({ "path": bad.to_string_lossy() })));
    assert!(none.is_none());
    assert!(err.is_err());
    assert_eq!(ws.contexts().len(), count);
    assert_eq!(ws.active(), active);
    assert_eq!(serde_json::to_value(&ws.get(live).unwrap().session.document).unwrap(), live_before);
}