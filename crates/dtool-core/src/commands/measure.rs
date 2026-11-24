use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::document::Document;
use crate::object::{Dimension, Object, ObjectId, ObjectKind};
use crate::selection::Selection;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub mod area;
pub use crate::units::suffix as unit_name;

pub const ADD: &str = "measure.add";
pub const READ: &str = "measure.query";

#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub length: f64,
    pub unscaled_length: f64,
    pub angle_deg: f64,
    pub units: &'static str,
}

pub fn readout_decimals(unit: &str, squared: bool) -> usize {
    match (unit, squared) {
        ("m" | "ft", true) => 6,
        ("m" | "ft", false) => 4,
        _ => 2,
    }
}

pub fn reading(doc: &Document, start: [f64; 2], end: [f64; 2]) -> Option<Reading> {
    let (units, factor) = doc.measurement_unit.from_document(doc.units, doc.dpi);
    let [drawing, real] = doc.drawing_scale.map(f64::from);
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let unscaled_length = dx.hypot(dy) * factor;
    let length = unscaled_length * real / drawing;
    (unscaled_length.is_finite() && unscaled_length > 0.0 && length.is_finite() && length > 0.0)
        .then_some(Reading { length, unscaled_length, angle_deg: dy.atan2(dx).to_degrees(), units })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        ADD => Some(plan(session, &command.params).is_ok()),
        READ => Some(read(session, &command.params).is_ok()),
        area::QUERY => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        ADD => Some(add(session, &command.params)),
        READ => Some(read(session, &command.params)),
        area::QUERY => Some(area::query(session, &command.params)),
        _ => None,
    }
}

fn point(value: Option<&Value>) -> Result<[f64; 2], CommandError> {
    let items = value.and_then(Value::as_array).filter(|items| items.len() == 2).ok_or_else(CommandError::invalid_params)?;
    let x = items[0].as_f64().filter(|v| v.is_finite()).ok_or_else(CommandError::invalid_params)?;
    let y = items[1].as_f64().filter(|v| v.is_finite()).ok_or_else(CommandError::invalid_params)?;
    Ok([x, y])
}

fn read(session: &Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let start = point(params.get("start"))?;
    let end = point(params.get("end"))?;
    let value = reading(&session.document, start, end).ok_or_else(CommandError::invalid_params)?;
    Ok(json!({ "length": value.length, "unscaled_length": value.unscaled_length,
        "angle_deg": value.angle_deg, "units": value.units }))
}

fn plan(session: &Session, params: &Value) -> Result<Object, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let start = point(params.get("start"))?;
    let end = point(params.get("end"))?;
    let offset = match params.get("offset") {
        None | Some(Value::Null) => 0.0,
        Some(value) => value.as_f64().filter(|v| v.is_finite()).ok_or_else(CommandError::invalid_params)?,
    };
    let value = reading(&session.document, start, end).ok_or_else(CommandError::invalid_params)?;
    let x0 = start[0].min(end[0]);
    let y0 = start[1].min(end[1]);
    let dimension = Dimension {
        start: [start[0] - x0, start[1] - y0],
        end: [end[0] - x0, end[1] - y0],
        offset,
        length: value.length,
        units: value.units.to_string(),
    };
    Ok(Object {
        name: dtool_strings::text("object.dimension_name").to_owned(),
        pos: [x0 as f32, y0 as f32],
        size: [(start[0] - end[0]).abs() as f32, (start[1] - end[1]).abs() as f32],
        kind: ObjectKind::Dimension(dimension),
        ..Object::default()
    })
}

fn add(session: &mut Session, params: &Value) -> CommandResult {
    let mut object = plan(session, params)?;
    let maximum = session.document.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
    let id = session.document.next_object_id.max(maximum.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    object.id = ObjectId(id);
    let (length, units) = match &object.kind {
        ObjectKind::Dimension(d) => (d.length, d.units.clone()),
        _ => return Err(CommandError::invalid_params()),
    };
    let before = snapshot(session);
    session.document.create_object(object);
    session.selection = Selection::from_ids(vec![ObjectId(id)]);
    commit_snapshot(session, ADD, before, json!({ "id": id, "length": length, "units": units }))
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::document::Units;

    #[test]
    fn add_measures_in_document_units_and_undo_removes() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let reply = session.run(&Command::with_params(ADD, json!({ "start": [10.0, 10.0], "end": [40.0, 50.0] }))).unwrap();
        assert_eq!(reply["length"], json!(50.0));
        assert_eq!(reply["units"], json!("px"));
        let id = ObjectId(reply["id"].as_u64().unwrap());
        let object = session.document.get_object(id).unwrap();
        assert_eq!(object.pos, [10.0, 10.0]);
        assert_eq!(object.size, [30.0, 40.0]);
        assert_eq!(session.selection.ids, vec![id]);
        assert!(session.run(&Command::with_params(ADD, json!({ "start": [1.0, 1.0], "end": [1.0, 1.0] }))).is_err());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.get_object(id).is_none());
    }
}
