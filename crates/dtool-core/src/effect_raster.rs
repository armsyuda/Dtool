use crate::command::Session;
use crate::Document;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const SET: &str = "doc.effect_raster.set";
pub const QUERY: &str = "doc.effect_raster.query";
pub const MIN_DPI: f32 = 36.0;
pub const MAX_DPI: f32 = 2400.0;
pub const PDF_DEFAULT_DPI: f32 = 300.0;

pub fn effect_scale(doc: &Document, out_px_per_unit: f32) -> f32 {
    match doc.effect_raster_dpi {
        Some(dpi) if doc.dpi > 0.0 && out_px_per_unit > 0.0 && out_px_per_unit.is_finite() => (dpi / doc.dpi / out_px_per_unit).clamp(0.01, 1.0),
        _ => 1.0,
    }
}

pub fn pdf_dpi(doc: &Document) -> f32 {
    doc.effect_raster_dpi.unwrap_or(PDF_DEFAULT_DPI)
}

fn parse(params: &Value) -> Result<Option<f32>, CommandError> {
    match params.get("dpi") {
        None => Err(CommandError::invalid_params()),
        Some(Value::Null) => Ok(None),
        Some(value) => {
            let dpi = value.as_f64().ok_or_else(CommandError::invalid_params)? as f32;
            if dpi.is_finite() && (MIN_DPI..=MAX_DPI).contains(&dpi) { Ok(Some(dpi)) } else { Err(CommandError::invalid_params()) }
        }
    }
}

fn reply(session: &Session, changes: Value) -> Value {
    json!({ "doc_version": session.generation, "changes": changes, "dpi": session.document.effect_raster_dpi, "pdf_dpi": pdf_dpi(&session.document) })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        SET => Some(session.has_document && parse(&command.params).is_ok()),
        QUERY => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if name != SET && name != QUERY {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        if name == QUERY {
            return Ok(reply(session, json!([])));
        }
        let dpi = parse(&command.params)?;
        if session.document.effect_raster_dpi == dpi {
            return Ok(reply(session, json!([])));
        }
        let mut document = session.document.clone();
        document.effect_raster_dpi = dpi;
        let selection = session.selection.ids.clone();
        crate::commands::document_change::commit(session, SET, document, selection, json!({ "dpi": dpi }))?;
        Ok(reply(session, json!([{ "name": SET }])))
    })())
}