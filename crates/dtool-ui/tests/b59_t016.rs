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
    let dir = std::env::temp_dir().join(format!("dtool_b59_t016_{}_{}", std::process::id(), test));
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

fn snapshot(ws: &Workspace, id: dtool_core::workspace::ContextId) -> Value {
    serde_json::to_value(&ws.get(id).unwrap().session.document).unwrap()
}
#[test]
fn svg_and_ai_open_and_import_into_new_editable_contexts() {
    let mut ws = workspace();
    let live = ws.new_document("Live");
    let before = snapshot(&ws, live);
    let svg = write_fixture("ok", "art.svg", SVG.as_bytes());
    let ai = write_fixture("ok", "art.ai", &ai_pdf());
    for (command, path, format) in [("import.svg", &svg, "svg"), ("doc.open", &svg, "svg"), ("import.ai", &ai, "ai"), ("doc.open", &ai, "ai")] {
        let count = ws.contexts().len();
        let (id, result) = run(&mut ws, command, path);
        let value = result.unwrap_or_else(|error| panic!("{command} {format}: {error:?}"));
        assert_eq!(value["format"], format, "{command}");
        let id = id.expect("new context");
        assert_eq!(ws.contexts().len(), count + 1);
        assert_eq!(ws.active(), Some(id));
        let objects = &ws.get(id).unwrap().session.document.objects;
        assert!(objects.iter().any(|o| !o.is_group()), "{command} {format} has no editable object");
    }
    assert_eq!(snapshot(&ws, live), before);
}

#[test]
fn failed_svg_and_ai_reads_leave_the_workspace_unchanged() {
    let mut ws = workspace();
    let live = ws.new_document("Live");
    let before = snapshot(&ws, live);
    let bad_svg = write_fixture("bad", "bad.svg", b"<svg");
    let ps_ai = write_fixture("bad", "legacy.ai", b"%!PS-Adobe-3.0\n%%Creator: Adobe Illustrator(R) 8.0\n");
    for (command, path) in [("import.svg", &bad_svg), ("doc.open", &bad_svg), ("import.ai", &ps_ai), ("doc.open", &ps_ai)] {
        let (id, result) = run(&mut ws, command, path);
        assert!(id.is_none(), "{command}");
        assert!(result.is_err(), "{command} {}", path.display());
        assert_eq!(ws.contexts().len(), 1);
        assert_eq!(ws.active(), Some(live));
    }
    let (_, result) = run(&mut ws, "import.ai", &ps_ai);
    let error = result.unwrap_err();
    assert_eq!(error.code, "UNSUPPORTED_PROFILE");
    assert_eq!(error.message_key, "error.ai.no_pdf");
    assert_eq!(snapshot(&ws, live), before);
}