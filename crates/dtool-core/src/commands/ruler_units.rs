use crate::command::Session;
use crate::document::{PageId, Units};
use crate::guides::{GuideId, GuideKind, PercentAxis};
use dtool_protocol::{Command, CommandError};
use serde_json::Value;

pub use crate::units::suffix;

pub fn display_scale(session: &Session) -> f32 {
    session.document.rulers.display_scale(session.document.units, session.document.dpi)
}

pub fn normalize(session: &Session, command: &Command) -> Result<Command, CommandError> {
    let mut command = command.clone();
    let fields: &[&str] = match command.name.as_str() {
        "guides.add" | "guides.update" => &["position"],
        "transform.set" => &["x", "y", "w", "h"],
        _ => return Ok(command),
    };
    for field in fields {
        if let Some(Value::String(input)) = command.params.get(*field) {
            let text = input.trim();
            if *field == "position" && let Some(percent) = text.strip_suffix('%') {
                let percent = percent.trim().parse::<f64>().map_err(|_| CommandError::invalid_params())?;
                command.params[*field] = serde_json::json!(guide_percent_position(session, &command, percent)? as f32);
                continue;
            }
            let rulers = session.document.rulers;
            let value = if text.chars().any(|ch| ch.is_ascii_alphabetic()) { text.to_string() }
                else if rulers.meters { format!("{text}m") }
                else { format!("{text}{}", suffix(rulers.effective_units(session.document.units))) };
            let pixels = crate::selection::parse_length(&Value::String(value), session.document.dpi)?;
            let stored = pixels * crate::rulers::units_per_px(session.document.units, session.document.dpi);
            command.params[*field] = serde_json::json!(stored);
        }
    }
    Ok(command)
}

fn guide_percent_position(session: &Session, command: &Command, percent: f64) -> Result<f64, CommandError> {
    let (kind, page) = if command.name == "guides.add" {
        let kind = serde_json::from_value::<GuideKind>(command.params["kind"].clone()).map_err(|_| CommandError::invalid_params())?;
        let page = serde_json::from_value::<Option<PageId>>(command.params["page_id"].clone()).map_err(|_| CommandError::invalid_params())?;
        (kind, page)
    } else {
        let id = command.params["id"].as_u64().ok_or_else(CommandError::invalid_params)?;
        let guide = session.guides.get(GuideId(id)).ok_or_else(CommandError::not_found)?;
        let kind = serde_json::from_value::<Option<GuideKind>>(command.params["kind"].clone()).map_err(|_| CommandError::invalid_params())?;
        (kind.unwrap_or(guide.kind), guide.page_id)
    };
    if page.is_some_and(|id| crate::guides::page_bounds(&session.document, id).is_none()) {
        return Err(CommandError::not_found());
    }
    PercentAxis::new(&session.document, page, kind).and_then(|axis| axis.from_percent(percent)).ok_or_else(CommandError::invalid_params)
}

pub fn readout(session: &Session, mut value: Value) -> Value {
    let scale = display_scale(session);
    value["display_units"] = session.document.rulers.unit_value(Some(session.document.units));
    for (raw, display) in [("pos", "display_pos"), ("size", "display_size")] {
        if let Some(values) = value.get(raw).and_then(Value::as_array) {
            value[display] = Value::Array(values.iter().map(|value| serde_json::json!(value.as_f64().unwrap_or(0.0) * f64::from(scale))).collect());
        }
    }
    value
}
