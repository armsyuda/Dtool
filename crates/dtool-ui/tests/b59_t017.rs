use dtool_core::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};
use std::path::PathBuf;

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300" viewBox="0 0 400 300"><rect id="box" x="60" y="40" width="100" height="80" fill="#ff0000"/><circle id="dot" cx="300" cy="200" r="20" fill="#0000ff"/></svg>"##;

fn workspace() -> Workspace {
    dtool_io::install_import_commands();
    Workspace::default()
}

fn write_fixture(test: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_b59_t017_{}_{}", std::process::id(), test));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn ai_pdf() -> Vec<u8> {
    let body = "1 0 0 rg 60 60 100 80 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", body.len(), body),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes());
    out
}

fn run(ws: &mut Workspace, name: &str, path: &PathBuf) -> (Option<dtool_core::workspace::ContextId>, Result<Value, dtool_protocol::CommandError>) {
    ws.run(Scope::active(), Command::with_params(name, json!({ "path": path.to_string_lossy() })))
}

fn manifest(objects: Vec<(&str, [f32; 4])>, colours: Vec<&str>) -> dtool_io::FidelityManifest {
    dtool_io::FidelityManifest {
        objects: objects.into_iter().map(|(name, rect)| (name.to_string(), rect)).collect(),
        texts: vec![],
        colours: colours.into_iter().map(str::to_string).collect(),
    }
}

#[test]
fn svg_opened_through_the_workspace_matches_its_fidelity_manifest() {
    let mut ws = workspace();
    let path = write_fixture("svg", "art.svg", SVG.as_bytes());
    let (id, result) = run(&mut ws, "doc.open", &path);
    result.expect("svg opens");
    let doc = &ws.get(id.unwrap()).unwrap().session.document;
    let expected = manifest(vec![("box", [60.0, 40.0, 100.0, 80.0]), ("dot", [280.0, 180.0, 40.0, 40.0])], vec!["#ff0000", "#0000ff"]);
    let warnings = dtool_io::fidelity_check(doc, &expected, 0.5);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn ai_opened_through_the_workspace_matches_its_fidelity_manifest() {
    let mut ws = workspace();
    let path = write_fixture("ai", "art.ai", &ai_pdf());
    let (id, result) = run(&mut ws, "doc.open", &path);
    let value = result.expect("ai opens");
    assert_eq!(value["report"]["objects"], 1);
    let doc = &ws.get(id.unwrap()).unwrap().session.document;
    assert_eq!(doc.pages[0].size, [400.0, 300.0]);
    let expected = manifest(vec![("rect_60_160", [60.0, 160.0, 100.0, 80.0])], vec!["#ff0000"]);
    let warnings = dtool_io::fidelity_check(doc, &expected, 0.01);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn fidelity_mismatch_after_workspace_open_is_not_a_pass() {
    let mut ws = workspace();
    let path = write_fixture("miss", "art.svg", SVG.as_bytes());
    let (id, result) = run(&mut ws, "doc.open", &path);
    result.expect("svg opens");
    let doc = &ws.get(id.unwrap()).unwrap().session.document;
    let expected = manifest(vec![("box", [0.0, 0.0, 1.0, 1.0])], vec!["#00ff00"]);
    assert_eq!(dtool_io::fidelity_check(doc, &expected, 0.5).len(), 2);
}