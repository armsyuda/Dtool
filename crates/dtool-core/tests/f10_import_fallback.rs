use dtool_core::{Command, Scope, Workspace};

fn pdf_bytes() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
    ];
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
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes());
    out
}

#[test]
fn core_fallback_admits_but_import_is_not_available() {
    assert!(dtool_core::import_hook::handler().is_none());
    let dir = std::env::temp_dir().join(format!("dtool_f10_core_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("fallback.pdf");
    std::fs::write(&path, pdf_bytes()).expect("write pdf");
    let mut ws = Workspace::default();
    let (_, admitted) = ws.run(Scope::active(), Command::with_params("import.admit", serde_json::json!({ "path": path.to_string_lossy() })));
    let profile = admitted.expect("core admits the pdf");
    assert_eq!(profile["engine"], "core");
    assert_eq!(profile["pages"], 1);
    let (context, imported) = ws.run(Scope::active(), Command::with_params("import.pdf", serde_json::json!({ "path": path.to_string_lossy() })));
    let error = imported.expect_err("core has no pdf engine");
    assert_eq!(error.code, "NOT_AVAILABLE");
    assert_eq!(error.message_key, "error.import.not_available");
    assert!(context.is_none());
    assert!(ws.contexts().is_empty());
}