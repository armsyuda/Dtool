use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::commands::curve_edit::{localise, parse_contours, world_contours};
use crate::curve::{Contour, Curve};
use crate::object::{Object, ObjectId, ObjectKind};
use crate::outline::object_outline;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const CONVERT: &str = "vector.convert_to_curves";
pub const SHAPE_CONVERT: &str = "shape.convert_to_curves";

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        CONVERT | SHAPE_CONVERT => Some(
            plan(session, command.name.as_str(), &command.params)
                .and_then(|plan| plan_edit(&plan, &command.params)).is_ok(),
        ),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        CONVERT | SHAPE_CONVERT => Some(convert(session, command.name.as_str(), &command.params)),
        _ => None,
    }
}

fn targets(session: &Session, params: &Value) -> Vec<ObjectId> {
    if let Some(id) = params.get("id").and_then(Value::as_u64) {
        return vec![ObjectId(id)];
    }
    match params.get("ids").or_else(|| params.get("nodes")).and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(Value::as_u64).map(ObjectId).collect(),
        None => session.selection.ids.clone(),
    }
}

pub fn preview_curve(object: &Object) -> Option<Curve> {
    if !matches!(object.kind, ObjectKind::Shape) {
        return None;
    }
    object_outline(object).map(|path| Curve::from_kurbo(&path).with_source(object.id.0))
}

pub fn preview_contours(object: &Object) -> Option<Vec<Contour>> {
    let curve = preview_curve(object)?;
    let mut converted = object.clone();
    converted.kind = ObjectKind::Curve(curve);
    converted.shape = None;
    Some(world_contours(&converted))
}

type Placed = ([f32; 2], [f32; 2], Vec<Contour>);

enum Conversion {
    Shape(Curve),
    Compound,
    Text(crate::text_lines::TextOutline),
}

fn plan_edit(plan: &[(ObjectId, Conversion)], params: &Value) -> Result<Option<Placed>, CommandError> {
    match params.get("contours") {
        None | Some(Value::Null) => Ok(None),
        Some(list) => {
            if !matches!(plan, [(_, Conversion::Shape(_))]) {
                return Err(CommandError::invalid_params());
            }
            localise(&parse_contours(list)?).map(Some)
        }
    }
}

fn plan(session: &Session, name: &str, params: &Value) -> Result<Vec<(ObjectId, Conversion)>, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let ids = targets(session, params);
    if ids.is_empty() {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(id) { continue; }
        let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
        match object.kind {
            ObjectKind::Curve(_) => {}
            ObjectKind::Shape => {
                if session.document.is_object_locked(object) {
                    return Err(CommandError::new("LOCKED"));
                }
                let path = object_outline(object).ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?;
                out.push((id, Conversion::Shape(Curve::from_kurbo(&path).with_source(id.0))));
            }
            ObjectKind::Compound(_) if name == CONVERT => {
                if session.document.is_object_locked(object) {
                    return Err(CommandError::new("LOCKED"));
                }
                out.push((id, Conversion::Compound));
            }
            ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_) if name == CONVERT => {
                if session.document.is_object_locked(object) {
                    return Err(CommandError::new("LOCKED"));
                }
                let outline = crate::text_lines::text_outline_curve(&session.document, id)?;
                out.push((id, Conversion::Text(outline)));
            }
            _ => return Err(CommandError::new("UNSUPPORTED_SHAPE")),
        }
    }
    if out.is_empty() {
        return Err(CommandError::new("NOTHING_TO_CONVERT"));
    }
    Ok(out)
}

fn convert(session: &mut Session, name: &str, params: &Value) -> CommandResult {
    let plan = plan(session, name, params)?;
    let mut edit = plan_edit(&plan, params)?;
    let before = snapshot(session);
    let ids: Vec<u64> = plan.iter().map(|(id, _)| id.0).collect();
    let edited = edit.is_some();
    let converted_text = plan.iter().any(|(_, conversion)| matches!(conversion, Conversion::Text(_)));
    let compound_tolerance = crate::boolean::compound_conversion_tolerance(&session.document);
    for (id, conversion) in plan {
        match conversion {
            Conversion::Shape(mut curve) => {
                if let Some(object) = session.document.find_object_mut(id.0) {
                    if let Some((pos, size, local)) = edit.take() {
                        let mut all = local.into_iter();
                        curve.contour = all.next().unwrap_or_default();
                        curve.holes = all.collect();
                        object.pos = pos;
                        object.size = size;
                    }
                    object.kind = ObjectKind::Curve(curve);
                    object.shape = None;
                    object.contour = None;
                    object.qr_payload = None;
                    object.qr_content = None;
                }
            }
            Conversion::Compound => {
                let object = session.document.find_object_mut(id.0).expect("planned compound");
                let ObjectKind::Compound(spec) = &object.kind else { unreachable!("planned compound") };
                match crate::boolean::expanded_compound_curve(object, spec, compound_tolerance, None) {
                    Ok(curve) => object.kind = ObjectKind::Curve(curve),
                    Err(error) => {
                        crate::commands::arrange::restore(session, &before);
                        return Err(error);
                    }
                }
            }
            Conversion::Text(outline) => {
                if let Err(error) = crate::text_cmd::apply_text_outline(&mut session.document, id, outline) {
                    crate::commands::arrange::restore(session, &before);
                    return Err(error);
                }
            }
        }
    }
    session.document.merge_bindings.retain(|binding| {
        !matches!(binding, crate::data_merge::MergeBinding::Qr { object_id, .. } if ids.contains(&object_id.0))
    });
    if converted_text {
        crate::frame::prune_chains(&mut session.document);
    }
    commit_snapshot(session, name, before, json!({ "converted": ids, "id": ids[0], "edited": edited }))
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::document::{Document, Units};
    use crate::object::Object;

    #[test]
    fn rect_converts_in_place_and_undo_restores_shape() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [10.0, 20.0], size: [100.0, 50.0], ..Object::default() });
        let text = session.document.create_object(Object { kind: ObjectKind::Text(String::new()), size: [10.0, 10.0], ..Object::default() });
        assert!(session.run(&Command::with_params(CONVERT, json!({ "id": text.0 }))).is_err());
        let reply = session.run(&Command::with_params(SHAPE_CONVERT, json!({ "id": id.0 }))).unwrap();
        assert_eq!(reply["converted"], json!([id.0]));
        let object = session.document.get_object(id).unwrap();
        let curve = object.curve().unwrap();
        assert!(curve.contour.closed);
        assert_eq!(curve.source_object, Some(id.0));
        assert_eq!(object.pos, [10.0, 20.0]);
        assert!(session.run(&Command::with_params(CONVERT, json!({ "id": id.0 }))).is_err());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(matches!(session.document.get_object(id).unwrap().kind, ObjectKind::Shape));
    }
}
