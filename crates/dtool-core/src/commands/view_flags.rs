use crate::command::Session;
use crate::view_state::{CanvasFlags, ViewMode};
use dtool_protocol::{Command, CommandError};
use serde::Deserialize;

pub const VIEW_FLAGS_SET: &str = "view.flags.set";
pub const VIEW_FLAGS_QUERY: &str = "view.flags.query";
pub const VIEW_ZOOM_TO_FIT: &str = "view.zoom.to_fit";
pub const VIEW_XRAY: &str = "view.xray";

fn xray_param(params: &serde_json::Value) -> Result<Option<bool>, CommandError> {
    match params.get("on") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(CommandError::invalid_params()),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewFlagsSetArgs {
    #[serde(default)]
    pub force_pixel_alignment: Option<bool>,
    #[serde(default)]
    pub clip_to_canvas: Option<bool>,
    #[serde(default)]
    pub preview_mode: Option<bool>,
    #[serde(default)]
    pub view_mode: Option<ViewMode>,
    #[serde(default)]
    pub show_bleed: Option<bool>,
    #[serde(default)]
    pub show_margins: Option<bool>,
    #[serde(default)]
    pub show_page_edges_in_preview: Option<bool>,
    #[serde(default)]
    pub show_pixel_selection: Option<bool>,
    #[serde(default)]
    pub highlight_missing_fonts: Option<bool>,
    #[serde(default)]
    pub highlight_missing_chars: Option<bool>,
    #[serde(default)]
    pub show_invisibles: Option<bool>,
}

pub fn parse(params: &serde_json::Value) -> Result<ViewFlagsSetArgs, CommandError> {
    match params {
        serde_json::Value::Null => Ok(ViewFlagsSetArgs::default()),
        serde_json::Value::Object(_) => serde_json::from_value(params.clone()).map_err(|_| CommandError::invalid_params()),
        _ => Err(CommandError::invalid_params()),
    }
}

fn reply(session: &Session, name: &str) -> serde_json::Value {
    serde_json::json!({
        "doc_version": session.generation,
        "changes": [{ "name": name }],
        "flags": session.view.canvas_flags
    })
}

pub fn apply_flags(flags: &mut CanvasFlags, args: &ViewFlagsSetArgs) {
    if let Some(value) = args.force_pixel_alignment { flags.force_pixel_alignment = value; }
    if let Some(value) = args.clip_to_canvas { flags.clip_to_canvas = value; }
    if let Some(value) = args.preview_mode { flags.preview_mode = value; }
    if let Some(value) = args.view_mode { flags.view_mode = value; }
    if let Some(value) = args.show_bleed { flags.show_bleed = value; }
    if let Some(value) = args.show_margins { flags.show_margins = value; }
    if let Some(value) = args.show_page_edges_in_preview { flags.show_page_edges_in_preview = value; }
    if let Some(value) = args.show_pixel_selection { flags.show_pixel_selection = value; }
    if let Some(value) = args.highlight_missing_fonts { flags.highlight_missing_fonts = value; }
    if let Some(value) = args.highlight_missing_chars { flags.highlight_missing_chars = value; }
    if let Some(value) = args.show_invisibles { flags.show_invisibles = value; }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        VIEW_FLAGS_SET => Some(session.has_document && parse(&command.params).is_ok()),
        VIEW_FLAGS_QUERY => Some(session.has_document),
        VIEW_XRAY => Some(session.has_document && xray_param(&command.params).is_ok()),
        VIEW_ZOOM_TO_FIT => Some(session.has_document && crate::view::parse_view_size(&command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    match command.name.as_str() {
        VIEW_FLAGS_SET => Some((|| {
            if !session.has_document { return Err(CommandError::no_document()); }
            let args = parse(&command.params)?;
            apply_flags(&mut session.view.canvas_flags, &args);
            if let Some(mode) = args.view_mode {
                session.view.xray = matches!(mode, ViewMode::Outline | ViewMode::Wireframe);
            }
            Ok(reply(session, VIEW_FLAGS_SET))
        })()),
        VIEW_FLAGS_QUERY => Some(if session.has_document { Ok(reply(session, VIEW_FLAGS_QUERY)) } else { Err(CommandError::no_document()) }),
        VIEW_XRAY => Some((|| {
            if !session.has_document { return Err(CommandError::no_document()); }
            let mode = &mut session.view.canvas_flags.view_mode;
            let outline = matches!(*mode, ViewMode::Outline | ViewMode::Wireframe);
            let on = xray_param(&command.params)?.unwrap_or(!outline);
            if on && !outline {
                *mode = ViewMode::Outline;
            } else if !on && outline {
                *mode = ViewMode::Normal;
            }
            session.view.xray = on;
            Ok(serde_json::json!({ "doc_version": session.generation, "changes": [{ "name": VIEW_XRAY }], "xray": on }))
        })()),
        VIEW_ZOOM_TO_FIT => Some((|| {
            if !session.has_document { return Err(CommandError::no_document()); }
            let view_size = crate::view::parse_view_size(&command.params)?;
            session.view_zoom_fit(view_size)
        })()),
        _ => None,
    }
}
