use dtool_core::{Scope, Workspace};
use dtool_protocol::{Command, CommandResult};
use serde_json::Value;
use std::path::PathBuf;

fn workspace() -> Workspace {
    dtool_io::install_import_commands();
    Workspace::default()
}

fn fixture_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_b59_t009_{}_{}", std::process::id(), test));
    std::fs::create_dir_all(&dir).expect("temp fixture dir");
    dir
}

fn write_fixture(test: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let path = fixture_dir(test).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn stream(dict: &str, body: &str) -> String {
    format!("<< {} /Length {} >>\nstream\n{}\nendstream", dict, body.len(), body)
}

fn build_pdf(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, body).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes(),
    );
    out
}

fn simple_pdf() -> Vec<u8> {
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_string(),
        stream("", "0 0 1 rg 10 10 100 50 re f"),
    ])
}

fn mapped_pdf() -> Vec<u8> {
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> /Contents 4 0 R >>".to_string(),
        stream("", "0 0 1 rg 10 10 100 50 re f q 20 20 80 30 re W n BT /F1 12 Tf 10 100 Td (Hello) Tj ET 40 0 0 40 50 50 cm /Im1 Do Q"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8", "abc"),
    ])
}

fn lossy_pdf() -> Vec<u8> {
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /ExtGState << /G1 << /BM /Multiply >> >> /Shading << /Sh1 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 1 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >> /Annots [5 0 R] /Contents 4 0 R >>".to_string(),
        stream("", "/G1 gs /Sh1 sh"),
        "<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (note) >>".to_string(),
    ])
}

fn af_tree() -> Vec<u8> {
    let mut out = Vec::new();
    for (tag, children) in [("Document", 1u8), ("Page", 1u8), ("Shape", 0u8)] {
        out.push(tag.len() as u8);
        out.extend_from_slice(tag.as_bytes());
        out.push(0);
        out.push(children);
    }
    out
}

fn af_container() -> Vec<u8> {
    let tree = af_tree();
    let name = b"doc.dat";
    let mut out = Vec::new();
    out.extend_from_slice(&0x496E666Eu32.to_le_bytes());
    out.extend_from_slice(&7u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0u8, 0u8]);
    out.extend_from_slice(b"fnI#");
    out.extend_from_slice(&1u32.to_le_bytes());
    let offset = (out.len() + 1 + name.len() + 8 + 8 + 8 + 4 + 1) as u64;
    out.push(name.len() as u8);
    out.extend_from_slice(name);
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&(tree.len() as u64).to_le_bytes());
    out.extend_from_slice(&(tree.len() as u64).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&tree);
    out
}

fn run(ws: &mut Workspace, name: &str, params: Value) -> CommandResult {
    ws.run(Scope::active(), Command::with_params(name, params)).1
}

fn error_code(result: &CommandResult) -> String {
    match result {
        Err(error) => error.code.clone(),
        Ok(value) => panic!("expected an error, got {value}"),
    }
}

#[test]
fn pdf_admission_supported() {
    let mut ws = workspace();
    let path = write_fixture("supported", "b59_t009_supported.pdf", &simple_pdf());
    let result = run(&mut ws, "import.admit", serde_json::json!({ "path": path.to_string_lossy(), "format": "pdf" }));
    let profile = result.expect("supported pdf is admitted");
    assert_eq!(profile["format"], "pdf");
    assert_eq!(profile["version"], "1.7");
    assert_eq!(profile["pages"], 1);
    assert_eq!(profile["supported"], true);
    assert_eq!(profile["encrypted"], false);
    assert!(ws.contexts().is_empty());
}

#[test]
fn pdf_admission_rejected() {
    let mut ws = workspace();
    let garbage = write_fixture("rejected", "b59_t009_unsupported.pdf", b"this is not a pdf file");
    let result = run(&mut ws, "import.admit", serde_json::json!({ "path": garbage.to_string_lossy(), "format": "pdf" }));
    assert_eq!(error_code(&result), "UNSUPPORTED_PROFILE");
    let truncated = write_fixture("rejected", "b59_t009_truncated.pdf", b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\n");
    let result = run(&mut ws, "import.admit", serde_json::json!({ "path": truncated.to_string_lossy(), "format": "pdf" }));
    assert_eq!(error_code(&result), "UNSUPPORTED_PROFILE");
    let missing = fixture_dir("rejected").join("missing.pdf");
    let result = run(&mut ws, "import.admit", serde_json::json!({ "path": missing.to_string_lossy(), "format": "pdf" }));
    assert!(matches!(result, CommandResult::Err(_)));
    assert!(ws.contexts().is_empty());
}

#[test]
fn import_pdf_uses_io_engine_and_opens_context() {
    let mut ws = workspace();
    let path = write_fixture("engine", "b59_t009_engine.pdf", &simple_pdf());
    let (context, result) = ws.run(Scope::active(), Command::with_params("import.pdf", serde_json::json!({ "path": path.to_string_lossy() })));
    let value = result.expect("io engine imports the pdf");
    let id = context.expect("import opens a context");
    assert_eq!(value["context"], id.0);
    assert_eq!(ws.contexts().len(), 1);
    let profile = run(&mut ws, "import.admit", serde_json::json!({ "path": path.to_string_lossy() })).expect("admitted");
    assert_eq!(profile["engine"], "io");
}
#[test]
fn pdf_mapping_rect_clip_image_text() {
    let mut ws = workspace();
    let path = write_fixture("mapped", "b59_t009_mapped.pdf", &mapped_pdf());
    let result = run(&mut ws, "import.pdf", serde_json::json!({ "path": path.to_string_lossy() }));
    assert!(matches!(result, CommandResult::Ok(_)));
}

#[test]
fn pdf_losses_reported_not_silent() {
    let mut ws = workspace();
    let path = write_fixture("lossy", "b59_t009_lossy.pdf", &lossy_pdf());
    let result = run(&mut ws, "import.pdf", serde_json::json!({ "path": path.to_string_lossy() }));
    let value = result.expect("lossy pdf imports");
    assert!(!value["report"]["losses"].as_array().map(Vec::is_empty).unwrap_or(true));
    let kinds: Vec<&str> = value["report"]["losses"].as_array().unwrap().iter().filter_map(|loss| loss["kind"].as_str()).collect();
    for kind in ["Blend", "Gradient", "Annotation"] {
        assert!(kinds.contains(&kind), "{kind} loss not reported: {kinds:?}");
    }
    assert_eq!(value["report"]["objects"], 1);
}

#[test]
fn af_inspect_report_no_open() {
    let mut ws = workspace();
    let bytes = af_container();
    let path = write_fixture("af", "b59_t009_sample.af", &bytes);
    let result = run(&mut ws, "import.af.inspect", serde_json::json!({ "path": path.to_string_lossy() }));
    let report = result.expect("af container is inspected");
    assert_eq!(report["version"], "7");
    assert_eq!(report["entries"], serde_json::json!(["doc.dat"]));
    assert!(report.get("verdict_go").and_then(Value::as_bool).is_some());
    assert_eq!(std::fs::read(&path).expect("fixture still readable"), bytes);
    assert!(ws.contexts().is_empty());
    let broken = write_fixture("af", "b59_t009_broken.af", b"not an af file at all");
    let result = run(&mut ws, "import.af.inspect", serde_json::json!({ "path": broken.to_string_lossy() }));
    assert_eq!(error_code(&result), "INVALID_FORMAT");
}

#[test]
fn pdf_fidelity_fixture() {
    let mut ws = workspace();
    let path = write_fixture("fidelity", "b59_t009_fidelity.pdf", &mapped_pdf());
    let result = run(&mut ws, "import.pdf", serde_json::json!({ "path": path.to_string_lossy() }));
    assert!(matches!(result, CommandResult::Ok(_)));
}