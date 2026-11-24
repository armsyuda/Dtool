use crate::{Command, CommandError, Document, ObjectId, Session, ViewState};
use crate::workspace::{ContextId, Scope, Workspace};
use std::sync::OnceLock;

pub const PREVIEW_DEFAULT_PX: f64 = 768.0;
pub const PREVIEW_MAX_PX: f64 = 2048.0;
pub const PREVIEW_MAX_BYTES: usize = 8 << 20;
pub const PREVIEW_KIND: &str = "document_preview";
pub const UI_STATE_KIND: &str = "ui_state";
pub const UI_SCREENSHOT_KIND: &str = "ui_screenshot";
pub const REPLY_KINDS: [&str; 3] = [PREVIEW_KIND, UI_STATE_KIND, UI_SCREENSHOT_KIND];

pub fn reply_kind(reply: &serde_json::Value) -> Option<&'static str> {
    let kind = reply.get("kind")?.as_str()?;
    REPLY_KINDS.into_iter().find(|known| *known == kind)
}

pub struct PreviewRequest { pub id: Option<ObjectId>, pub max_px: f64, pub view_size: Option<[f32; 2]> }
pub struct RenderedPreview { pub png: Vec<u8>, pub width: u32, pub height: u32, pub bounds: [f32; 4], pub source: &'static str }
pub type PreviewProvider = fn(&Document, &ViewState, &PreviewRequest) -> Result<RenderedPreview, CommandError>;
static PROVIDER: OnceLock<PreviewProvider> = OnceLock::new();

pub fn register_provider(provider: PreviewProvider) -> bool { PROVIDER.set(provider).is_ok() || PROVIDER.get().is_some() }

pub fn parse(params: &serde_json::Value) -> Result<PreviewRequest, CommandError> {
    if !params.is_null() && !params.is_object() { return Err(CommandError::invalid_params()); }
    let max_px = match params.get("max_px") { None => PREVIEW_DEFAULT_PX, Some(value) => value.as_f64().ok_or_else(CommandError::invalid_params)? };
    if !max_px.is_finite() || max_px <= 0.0 { return Err(CommandError::invalid_params()); }
    if max_px > PREVIEW_MAX_PX { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    let id = match params.get("id") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(ObjectId(value.as_u64().or_else(|| value.as_str().and_then(|v| v.parse().ok())).ok_or_else(CommandError::invalid_params)?)),
    };
    let view_size: Option<[f32; 2]> = params.get("view_size").map(|value| serde_json::from_value(value.clone())).transpose().map_err(|_| CommandError::invalid_params())?;
    if view_size.is_some_and(|size| !size.iter().all(|v| v.is_finite() && *v > 0.0)) { return Err(CommandError::invalid_params()); }
    Ok(PreviewRequest { id, max_px, view_size })
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let a = group[0]; let b = group.get(1).copied().unwrap_or(0); let c = group.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(a >> 2) as usize] as char);
        result.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if group.len() > 1 { ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char } else { '=' });
        result.push(if group.len() > 2 { ALPHABET[(c & 63) as usize] as char } else { '=' });
    }
    result
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == "render.node_preview" { Some(session.has_document && parse(&command.params).is_ok() && PROVIDER.get().is_some()) } else { None }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    if command.name != "render.node_preview" { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let request = parse(&command.params)?;
        let provider = PROVIDER.get().ok_or_else(CommandError::not_implemented)?;
        let image = provider(&session.document, &session.view, &request)?;
        if image.png.len() > PREVIEW_MAX_BYTES || image.width > PREVIEW_MAX_PX as u32 || image.height > PREVIEW_MAX_PX as u32 { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
        Ok(serde_json::json!({ "kind": PREVIEW_KIND, "doc_version": session.generation, "mime_type": "image/png", "png_base64": base64(&image.png), "width": image.width, "height": image.height, "byte_size": image.png.len(), "bounds": image.bounds, "source": image.source, "oversize_policy": "reject" }))
    })())
}

pub fn run_workspace(workspace: &mut Workspace, scope: Scope, command: &Command) -> Option<(Option<ContextId>, crate::CommandResult)> {
    if command.name != "render.node_preview" { return None; }
    let id = scope.context.or_else(|| workspace.active());
    let result = match id {
        None => Err(CommandError::no_document()),
        Some(id) => match workspace.get_mut(id) {
            None => Err(CommandError::context_not_found()),
            Some(context) => run(&mut context.session, command).unwrap_or_else(|| Err(CommandError::not_implemented())),
        },
    };
    Some((id, result))
}
