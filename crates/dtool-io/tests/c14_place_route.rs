use std::path::PathBuf;

use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use dtool_core::ContextId;
use dtool_io::import_commands::handle;
use serde_json::json;

const EPS: &str = "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 72 72\n%%EndComments\nnewpath 10 10 moveto 60 10 lineto 60 60 lineto closepath fill\nshowpage\n%%EOF\n";
const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><rect x=\"5\" y=\"5\" width=\"40\" height=\"30\" fill=\"red\"/><circle cx=\"80\" cy=\"40\" r=\"10\"/></svg>";

fn eps(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("c14_place_route_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shape.eps");
    std::fs::write(&path, EPS).unwrap();
    path
}

fn svg() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("c14_place_route_svg_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("place.svg");
    std::fs::write(&path, SVG).unwrap();
    path
}

fn place(path: &std::path::Path) -> Command {
    Command::with_params(dtool_core::import_hook::IMPORT_PLACE, json!({ "path": path.to_string_lossy() }))
}

fn live() -> (Workspace, ContextId) {
    dtool_io::import_commands::install();
    let mut ws = Workspace::default();
    let id = ws.new_document("Live");
    (ws, id)
}

fn count(ws: &Workspace, id: ContextId) -> usize {
    ws.get(id).unwrap().session.document.objects.len()
}

#[test]
fn import_place_is_an_import_hook_command() {
    assert!(dtool_core::import_hook::is_import("import.place"));
    assert_eq!(dtool_core::import_hook::IMPORT_PLACE, dtool_io::place_current::IMPORT_PLACE);
}

#[test]
fn import_place_handler_places_into_the_live_document_with_one_undo() {
    let path = eps("handler");
    let mut ws = Workspace::default();
    let live = ws.new_document("Live");
    let steps = ws.get(live).unwrap().session.undo.applied_steps();
    let (ctx, result) = handle(&mut ws, &place(&path)).expect("import.place handled");
    let value = result.expect("place");
    assert_eq!(ctx, Some(live));
    assert_eq!(value["changes"], json!([{ "name": "import.place" }]));
    assert_eq!(ws.contexts().len(), 1);
    let session = &ws.get(live).unwrap().session;
    assert_eq!(session.document.objects.len(), 1);
    assert_eq!(session.undo.applied_steps(), steps + 1);
    let (_, missing) = handle(&mut ws, &Command::with_params("import.place", json!({}))).unwrap();
    assert!(missing.is_err());
    assert_eq!(ws.get(live).unwrap().session.document.objects.len(), 1);
}

#[test]
fn import_place_runs_through_workspace_run_on_a_live_document() {
    dtool_io::import_commands::install();
    let path = eps("run");
    let mut ws = Workspace::default();
    let live = ws.new_document("Live");
    let (ctx, result) = ws.run(Scope::active(), place(&path));
    result.expect("import.place via Workspace::run");
    assert_eq!(ctx, Some(live));
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(ws.get(live).unwrap().session.document.objects.len(), 1);
    ws.run(Scope::active(), Command::new("edit.undo")).1.expect("undo");
    assert!(ws.get(live).unwrap().session.document.objects.is_empty());
}

#[test]
fn c14_import_place_handler_places_into_active_document_with_one_undo() {
    let (mut ws, id) = live();
    let tabs = ws.contexts().len();
    let (target, result) = dtool_io::import_commands::handle(&mut ws, &Command::with_params("import.place", json!({ "path": svg() }))).expect("import.place routed by dtool-io handler");
    let reply = result.expect("placed");
    assert!(reply["ids"].as_array().is_some_and(|ids| !ids.is_empty()), "{reply}");
    assert!(count(&ws, id) >= 2);
    assert_eq!(ws.contexts().len(), tabs, "place, not open: no new tab");
    let (_, undo) = ws.run(Scope::of(id), Command::with_params("edit.undo", json!({})));
    undo.unwrap();
    assert_eq!(count(&ws, id), 0);
    let (_, missing) = dtool_io::import_commands::handle(&mut ws, &Command::with_params("import.place", json!({}))).unwrap();
    assert_eq!(missing.unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn c14_import_place_through_workspace_run() {
    let (mut ws, id) = live();
    let (target, result) = ws.run(Scope::of(id), Command::with_params("import.place", json!({ "path": svg() })));
    result.expect("import.place via Workspace::run");
    assert_eq!(target, Some(id));
    assert!(count(&ws, id) >= 2);
}