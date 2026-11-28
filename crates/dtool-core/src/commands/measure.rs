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

fn master_at(doc: &Document, [px, py]: [f64; 2]) -> Option<ObjectId> {
    doc.master_pages.roots.iter().copied().find_map(|id| {
        let [x, y, w, h] = crate::geometry::object_world_bounds(doc.get_object(id)?).map(f64::from);
        (px >= x && px <= x + w && py >= y && py <= y + h).then(|| doc.master_pages.spread_id(id))
    })
}

fn measurement_basis(doc: &Document, mut points: impl Iterator<Item = [f64; 2]>) -> (crate::units::ReadoutUnit, [f32; 2]) {
    let master = points.next().and_then(|point| master_at(doc, point));
    master.filter(|&id| points.all(|point| master_at(doc, point) == Some(id)))
        .and_then(|id| doc.master_pages.page_setup(id))
        .and_then(|setup| setup.drawing_scale.map(|scale| (setup.measurement_unit, scale)))
        .unwrap_or((doc.measurement_unit, doc.drawing_scale))
}

pub fn reading(doc: &Document, start: [f64; 2], end: [f64; 2]) -> Option<Reading> {
    let (unit, scale) = measurement_basis(doc, [start, end].into_iter());
    let (units, factor) = unit.from_document(doc.units, doc.dpi);
    let [drawing, real] = scale.map(f64::from);
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let unscaled_length = dx.hypot(dy) * factor;
    let length = unscaled_length * real / drawing;
    (unscaled_length.is_finite() && unscaled_length > 0.0 && length.is_finite() && length > 0.0)
        .then_some(Reading { length, unscaled_length, angle_deg: dy.atan2(dx).to_degrees(), units })
}

pub(crate) fn refresh_saved_dimensions(doc: &mut Document) {
    let readings: Vec<_> = doc.objects.iter().enumerate().filter_map(|(index, object)| {
        let ObjectKind::Dimension(dimension) = &object.kind else { return None };
        let [x, y] = object.pos.map(f64::from);
        let point = |[dx, dy]: [f64; 2]| [x + dx, y + dy];
        reading(doc, point(dimension.start), point(dimension.end)).map(|value| (index, value))
    }).collect();
    for (index, value) in readings {
        if let ObjectKind::Dimension(dimension) = &mut doc.objects[index].kind {
            dimension.length = value.length;
            if dimension.units != value.units {
                dimension.units = value.units.to_owned();
            }
        }
    }
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
    let mut object = Object {
        name: dtool_strings::text("object.dimension_name").to_owned(),
        pos: [x0 as f32, y0 as f32],
        size: [(start[0] - end[0]).abs() as f32, (start[1] - end[1]).abs() as f32],
        kind: ObjectKind::Dimension(dimension),
        ..Object::default()
    };
    let doc = &session.document;
    let start = start.map(|v| v as f32);
    let end = end.map(|v| v as f32);
    let midpoint = [start[0] * 0.5 + end[0] * 0.5, start[1] * 0.5 + end[1] * 0.5];
    use crate::artboard::edit::{assign_new_object, at_point, page_at_point};
    let board_at = |point| at_point(doc, point).map(|board| board.id);
    let board = board_at(midpoint);
    let page = page_at_point(doc, midpoint);
    if board_at(start) == board && board_at(end) == board
        && page_at_point(doc, start) == page && page_at_point(doc, end) == page
    {
        assign_new_object(doc, &mut object, midpoint);
    }
    Ok(object)
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
