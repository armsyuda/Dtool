use dtool_core::{Command, CommandResult, Scope, Workspace};
use serde_json::Value;
use std::path::PathBuf;

fn fixture_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_f10_io_{}_{}", std::process::id(), test));
    std::fs::create_dir_all(&dir).expect("temp fixture dir");
    dir
}

fn write_fixture(test: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let path = fixture_dir(test).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn stream(dict: &str, body: &str) -> String {
    format!(
        "<< {} /Length {} >>\nstream\n{}\nendstream",
        dict,
        body.len(),
        body
    )
}

fn build_pdf(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, body).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref
        )
        .as_bytes(),
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

fn lossy_pdf(with_body: bool) -> Vec<u8> {
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /ExtGState << /G1 << /BM /UnrecognizedBlend >> >> /Shading << /Sh1 << /ShadingType 3 /ColorSpace /DeviceRGB /Coords [0 0 1 3 0 2] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >> /Annots [5 0 R] /Contents 4 0 R >>".to_string(),
        stream("", if with_body { "0 0 1 rg 10 10 100 50 re f /G1 gs /Sh1 sh" } else { "/G1 gs /Sh1 sh" }),
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

fn workspace() -> Workspace {
    dtool_io::install_import_commands();
    Workspace::default()
}

fn run(
    ws: &mut Workspace,
    name: &str,
    params: Value,
) -> (Option<dtool_core::ContextId>, CommandResult) {
    ws.run(Scope::active(), Command::with_params(name, params))
}

#[test]
fn f10_io_admit_pdf_and_reject() {
    let mut ws = workspace();
    let path = write_fixture("admit", "simple.pdf", &simple_pdf());
    let (_, result) = run(
        &mut ws,
        "import.admit",
        serde_json::json!({ "path": path.to_string_lossy(), "format": "pdf" }),
    );
    let profile = result.expect("pdf admitted by lopdf");
    assert_eq!(profile["engine"], "io");
    assert_eq!(profile["pages"], 1);
    assert_eq!(profile["supported"], true);
    let bad = write_fixture("admit", "bad.pdf", b"nope");
    let (_, result) = run(
        &mut ws,
        "import.admit",
        serde_json::json!({ "path": bad.to_string_lossy() }),
    );
    assert_eq!(
        result.expect_err("garbage rejected").code,
        "UNSUPPORTED_PROFILE"
    );
    let missing = fixture_dir("admit").join("missing.pdf");
    let (_, result) = run(
        &mut ws,
        "import.admit",
        serde_json::json!({ "path": missing.to_string_lossy() }),
    );
    assert_eq!(result.expect_err("missing file").code, "IO.NOT_FOUND");
    assert!(ws.contexts().is_empty());
}

#[test]
fn f10_io_import_pdf_opens_context_with_report() {
    let mut ws = workspace();
    let path = write_fixture("mapped", "mapped.pdf", &mapped_pdf());
    let (context, result) = run(
        &mut ws,
        "import.pdf",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let value = result.expect("pdf imported");
    let id = context.expect("import opens a context");
    assert_eq!(ws.active(), Some(id));
    assert_eq!(value["context"], id.0);
    assert_eq!(value["title"], "mapped");
    assert!(value["report"]["objects"].as_u64().unwrap_or(0) >= 1);
    assert!(value["report"]["texts"].as_u64().unwrap_or(0) >= 1);
    assert!(value["report"]["images"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(ws.contexts().len(), 1);
}

#[test]
fn f10_io_import_pdf_reports_losses() {
    let mut ws = workspace();
    let path = write_fixture("lossy", "lossy.pdf", &lossy_pdf(true));
    let original = std::fs::read(&path).unwrap();
    let (context, result) = run(
        &mut ws,
        "import.pdf",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let value = result.expect("supported body imported with explicit losses");
    let document = &ws
        .get(context.expect("real body opens context"))
        .unwrap()
        .session
        .document;
    assert_eq!(
        document
            .objects
            .iter()
            .filter(|obj| !obj.is_group())
            .count(),
        2
    );
    assert_eq!(value["report"]["objects"], 2);
    assert_eq!(value["report"]["acceptable"], false);
    let kinds: Vec<String> = value["report"]["losses"]
        .as_array()
        .expect("losses listed")
        .iter()
        .filter_map(|loss| loss["kind"].as_str().map(str::to_string))
        .collect();
    assert!(kinds.contains(&"Blend".to_string()));
    assert!(kinds.contains(&"Gradient".to_string()));
    assert!(kinds.contains(&"Annotation".to_string()));
    let losses = value["report"]["losses"].as_array().unwrap();
    for kind in ["Blend", "Gradient"] {
        assert!(
            losses
                .iter()
                .any(|loss| loss["kind"] == kind && loss["required"] == true)
        );
    }
    assert!(
        losses
            .iter()
            .any(|loss| loss["kind"] == "Annotation" && loss["required"] == false)
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn f10_io_import_pdf_lossy_only_body_opens_with_losses() {
    let mut ws = workspace();
    let target = ws.new_document("Existing");
    let before = serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap();
    let bytes = lossy_pdf(false);
    let path = write_fixture("lossy_only", "lossy_only.pdf", &bytes);
    let (context, result) = run(
        &mut ws,
        "import.pdf",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let value = result.expect("lossy-only body opens with losses");
    let id = context.expect("new context");
    assert_ne!(id, target);
    assert_eq!(ws.contexts().len(), 2);
    assert_eq!(value["report"]["objects"], 1);
    let kinds: Vec<&str> = value["report"]["losses"]
        .as_array()
        .expect("losses listed")
        .iter()
        .filter_map(|loss| loss["kind"].as_str())
        .collect();
    for kind in ["Blend", "Gradient", "Annotation"] {
        assert!(kinds.contains(&kind), "{kind} missing: {kinds:?}");
    }
    assert_eq!(
        serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap(),
        before
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn f10_io_import_pdf_bad_options() {
    let mut ws = workspace();
    let path = write_fixture("options", "simple.pdf", &simple_pdf());
    let (_, result) = run(
        &mut ws,
        "import.pdf",
        serde_json::json!({ "path": path.to_string_lossy(), "options": { "pages": [2, 3] } }),
    );
    assert!(result.is_err());
    let (context, result) = run(
        &mut ws,
        "import.pdf",
        serde_json::json!({ "path": path.to_string_lossy(), "options": { "pages": [1, 1], "dpi": 72 } }),
    );
    assert!(result.is_ok());
    assert!(context.is_some());
}

#[test]
fn f10_io_af_inspect_reads_tree_without_opening() {
    let mut ws = workspace();
    let bytes = af_container();
    let path = write_fixture("af", "sample.af", &bytes);
    let (context, result) = run(
        &mut ws,
        "import.af.inspect",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let report = result.expect("af inspected");
    assert!(context.is_none());
    assert_eq!(report["engine"], "io");
    assert_eq!(report["version"], "7");
    assert_eq!(report["node_counts"]["Document"], 1);
    assert_eq!(report["node_counts"]["Shape"], 1);
    assert_eq!(report["feature_status"]["Page"], "supported");
    assert_eq!(report["verdict_go"], true);
    assert_eq!(std::fs::read(&path).expect("fixture"), bytes);
    assert!(ws.contexts().is_empty());
}


fn simple_svg() -> Vec<u8> {
    br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="10" y="20" width="30" height="40" fill="red"/></svg>"#.to_vec()
}

#[test]
fn f10_io_doc_open_pdf_uses_import_pdf() {
    let mut ws = workspace();
    let path = write_fixture("open_mapped", "mapped.pdf", &mapped_pdf());
    let (context, result) = run(
        &mut ws,
        "doc.open",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let value = result.expect("doc.open routes to import.pdf");
    let id = context.expect("open opens a context");
    assert_eq!(ws.active(), Some(id));
    assert_eq!(value["context"], id.0);
    assert_eq!(value["format"], "pdf");
    assert!(value["report"]["objects"].as_u64().unwrap_or(0) >= 1);
    assert!(value["report"]["texts"].as_u64().unwrap_or(0) >= 1);
    assert!(value["report"]["images"].as_u64().unwrap_or(0) >= 1);
    let document = &ws.get(id).unwrap().session.document;
    assert!(!document.objects.is_empty());
    assert_eq!(ws.contexts().len(), 1);
}

#[test]
fn f10_io_doc_open_failed_pdf_leaves_existing_document() {
    let mut ws = workspace();
    let target = ws.new_document("Existing");
    let before = serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap();
    let generation = ws.get(target).unwrap().session.generation;
    let missing = fixture_dir("open_missing").join("missing.pdf");
    let (context, result) = run(
        &mut ws,
        "doc.open",
        serde_json::json!({ "path": missing.to_string_lossy() }),
    );
    assert_eq!(result.expect_err("missing pdf").code, "IO.NOT_FOUND");
    assert!(context.is_none());
    let bad = write_fixture("open_missing", "bad.pdf", b"nope");
    let (context, result) = run(
        &mut ws,
        "doc.open",
        serde_json::json!({ "path": bad.to_string_lossy() }),
    );
    assert!(result.is_err());
    assert!(context.is_none());
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(ws.active(), Some(target));
    assert_eq!(
        serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap(),
        before
    );
    assert_eq!(ws.get(target).unwrap().session.generation, generation);
}

#[test]
fn f10_io_doc_open_svg_and_pdf_compatible_ai_use_importers() {
    let mut ws = workspace();
    let svg = write_fixture("open_svg", "shape.svg", &simple_svg());
    let (context, result) = run(
        &mut ws,
        "doc.open",
        serde_json::json!({ "path": svg.to_string_lossy() }),
    );
    let value = result.expect("doc.open routes to import.svg");
    let id = context.expect("svg open opens a context");
    assert_eq!(value["format"], "svg");
    assert!(value["report"]["objects"].as_u64().unwrap_or(0) >= 1);
    assert!(!ws.get(id).unwrap().session.document.objects.is_empty());

    let ai = write_fixture("open_ai", "compat.ai", &simple_pdf());
    let (context, result) = run(
        &mut ws,
        "doc.open",
        serde_json::json!({ "path": ai.to_string_lossy() }),
    );
    let value = result.expect("doc.open of pdf-compatible AI routes to import.ai");
    let id = context.expect("ai open opens a context");
    assert_eq!(value["format"], "ai");
    assert!(value["report"]["objects"].as_u64().unwrap_or(0) >= 1);
    assert!(!ws.get(id).unwrap().session.document.objects.is_empty());
}

#[test]
fn f10_io_doc_open_eps_opens_through_import_eps() {
    let mut ws = workspace();
    let target = ws.new_document("Existing");
    let before = serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap();
    let path = write_fixture("open_eps", "line.eps", b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 72 72\n%%EndComments\nnewpath 10 10 moveto 60 10 lineto 60 60 lineto closepath fill\nshowpage\n%%EOF\n");
    let (context, result) = run(
        &mut ws,
        "doc.open",
        serde_json::json!({ "path": path.to_string_lossy() }),
    );
    let value = result.expect("eps opens");
    assert_eq!(value["format"], "eps");
    let id = context.expect("new context");
    assert_ne!(id, target);
    assert_eq!(ws.contexts().len(), 2);
    assert!(!ws.get(id).unwrap().session.document.objects.is_empty());
    assert_eq!(
        serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap(),
        before
    );
}
