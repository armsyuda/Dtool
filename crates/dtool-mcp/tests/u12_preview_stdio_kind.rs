#[path = "../../dtool-cli/src/harness.rs"]
#[allow(dead_code)]
mod profile_fixture;

use dtool_core::commands::preview::{register_provider, PreviewRequest, RenderedPreview, PREVIEW_KIND};
use dtool_core::{CommandError, Document, ViewState, Workspace};
use dtool_mcp::{serve_stdio, Config, Server};
use profile_fixture::TestProfile;
use serde_json::{json, Value};
use std::sync::Arc;

fn fake_provider(_document: &Document, _view: &ViewState, request: &PreviewRequest) -> Result<RenderedPreview, CommandError> {
    let side = request.max_px.min(8.0) as u32;
    Ok(RenderedPreview { png: vec![0x89, b'P', b'N', b'G'], width: side, height: side, bounds: [0.0, 0.0, 8.0, 8.0], source: "document" })
}

fn find_preview(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) if map.contains_key("png_base64") => Some(value),
        Value::Object(map) => map.values().find_map(find_preview),
        Value::Array(items) => items.iter().find_map(find_preview),
        _ => None,
    }
}

#[test]
fn u12_stdio_preview_passes_document_preview_kind_through() {
    assert!(register_provider(fake_provider));
    let profile = TestProfile::new();
    let mut workspace = Workspace::default();
    workspace.new_document("u12-stdio-preview");
    let shared = workspace.into_shared();
    let mut server = Server::with_workspace(Config { allow_dirs: vec![profile.path().to_path_buf()], ..Config::default() }, Arc::clone(&shared));
    server.gui_connection_handle().set_from_gui(true);
    let request = json!({ "jsonrpc": "2.0", "id": 7, "method": "dtool.preview", "params": { "max_px": 8.0 } });
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    let mut output = Vec::new();
    serve_stdio(&mut server, std::io::Cursor::new(bytes), &mut output);
    let reply: Value = serde_json::from_slice(&output).unwrap();
    let preview = find_preview(&reply).unwrap_or_else(|| panic!("{reply}"));
    assert_eq!(preview["kind"], PREVIEW_KIND, "{reply}");
    assert_eq!(preview["mime_type"], "image/png");
    assert!(preview.get("doc_version").is_some_and(Value::is_u64));
}
