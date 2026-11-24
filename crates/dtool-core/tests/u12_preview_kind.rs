use dtool_core::commands::preview::{register_provider, reply_kind, PreviewRequest, RenderedPreview, PREVIEW_KIND, REPLY_KINDS, UI_SCREENSHOT_KIND, UI_STATE_KIND};
use dtool_core::{Command, CommandError, Document, Scope, ViewState, Workspace};
use serde_json::json;

fn fake_provider(_document: &Document, _view: &ViewState, request: &PreviewRequest) -> Result<RenderedPreview, CommandError> {
    let side = request.max_px.min(8.0) as u32;
    Ok(RenderedPreview { png: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a], width: side, height: side, bounds: [0.0, 0.0, 8.0, 8.0], source: "document" })
}

#[test]
fn u12_kinds_are_distinct_and_preview_is_document_preview() {
    assert_eq!(REPLY_KINDS, ["document_preview", "ui_state", "ui_screenshot"]);
    assert_ne!(PREVIEW_KIND, UI_STATE_KIND);
    assert_ne!(PREVIEW_KIND, UI_SCREENSHOT_KIND);
    assert_eq!(reply_kind(&json!({ "kind": "ui_screenshot" })), Some(UI_SCREENSHOT_KIND));
    assert_eq!(reply_kind(&json!({ "kind": "screenshot" })), None);
    assert_eq!(reply_kind(&json!({ "png_base64": "" })), None);
}

#[test]
fn u12_preview_reply_carries_kind_on_workspace_route() {
    assert!(register_provider(fake_provider));
    let mut workspace = Workspace::default();
    let id = workspace.new_document("u12-preview");
    let (target, result) = workspace.run(Scope::of(id), Command::with_params("render.node_preview", json!({ "max_px": 8.0 })));
    assert_eq!(target, Some(id));
    let routed = result.unwrap();
    assert_eq!(reply_kind(&routed), Some(PREVIEW_KIND));
    assert_ne!(routed["kind"], UI_STATE_KIND);
    assert_ne!(routed["kind"], UI_SCREENSHOT_KIND);
    assert_eq!(routed["mime_type"], "image/png");
    assert!(routed["png_base64"].is_string());
    assert!(routed.get("doc_version").is_some_and(serde_json::Value::is_u64));
    let (_, oversize) = workspace.run(Scope::of(id), Command::with_params("render.node_preview", json!({ "max_px": 4096.0 })));
    assert_eq!(oversize.unwrap_err().code, "REQUEST_TOO_LARGE");
}
