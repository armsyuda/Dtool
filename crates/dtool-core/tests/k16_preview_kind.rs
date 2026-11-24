use dtool_core::commands::preview::{register_provider, PreviewRequest, RenderedPreview, PREVIEW_KIND};
use dtool_core::{Command, CommandError, Document, Scope, ViewState, Workspace};
use serde_json::json;

fn fake_provider(_document: &Document, _view: &ViewState, request: &PreviewRequest) -> Result<RenderedPreview, CommandError> {
    let side = request.max_px.min(4.0) as u32;
    Ok(RenderedPreview { png: vec![0x89, b'P', b'N', b'G'], width: side, height: side, bounds: [0.0, 0.0, 4.0, 4.0], source: "document" })
}

#[test]
fn document_preview_reports_its_kind_and_wire_field_names() {
    assert!(register_provider(fake_provider));
    let mut workspace = Workspace::default();
    let id = workspace.new_document("k16-preview");
    let (target, result) = workspace.run(Scope::of(id), Command::with_params("render.node_preview", json!({ "max_px": 4.0 })));
    assert_eq!(target, Some(id));
    let payload = result.unwrap();
    assert_eq!(PREVIEW_KIND, "document_preview");
    assert_eq!(payload["kind"], "document_preview");
    assert_eq!(payload["mime_type"], "image/png");
    assert_eq!(payload["png_base64"], "iVBORw==");
    assert_eq!(payload["byte_size"], 4);
    assert!(payload.get("doc_version").is_some_and(|value| value.is_u64()));
    assert!(payload.get("mime").is_none());
    assert!(payload.get("data").is_none());
}

#[test]
fn document_preview_without_document_is_not_a_preview() {
    assert!(register_provider(fake_provider));
    let mut workspace = Workspace::default();
    let (_, result) = workspace.run(Scope::active(), Command::with_params("render.node_preview", json!({})));
    assert_eq!(result.unwrap_err().code, "NO_DOCUMENT");
}