use crate::{Command, CommandError, CommandResult, Object, ObjectId, Session};
use crate::shape::{CornerRecipe, ShapeKind, ShapeRecipe, TriangleRecipe};
use serde_json::json;

pub const DEFAULT_TRIANGLE_POINTS:[f32;3]=[0.5,1.0,0.5];
pub fn default_triangle()->TriangleRecipe {TriangleRecipe{points:DEFAULT_TRIANGLE_POINTS}}
pub const B19_RECIPE_KEYS:[(&str,ShapeKind);19]=[("star",ShapeKind::Star),("arrow",ShapeKind::Arrow),("callout_ellipse",ShapeKind::CalloutEllipse),("callout_rectangle",ShapeKind::CalloutRectangle),("donut",ShapeKind::Donut),("pie",ShapeKind::Pie),("heart",ShapeKind::Heart),("cog",ShapeKind::Cog),("tear",ShapeKind::Tear),("crescent",ShapeKind::Crescent),("cloud",ShapeKind::Cloud),("segment",ShapeKind::Segment),("cat",ShapeKind::Cat),("square_star",ShapeKind::SquareStar),("double_star",ShapeKind::DoubleStar),("diamond",ShapeKind::Diamond),("trapezoid",ShapeKind::Trapezoid),("polygon",ShapeKind::Polygon),("spiral",ShapeKind::Spiral)];
pub const CORNER_RADIUS_SET: &str = "shape.corner.radius.set";

pub(crate) fn rounded_corner(object: &Object) -> Option<&CornerRecipe> {
    if !matches!(object.kind, crate::ObjectKind::Shape) {
        return None;
    }
    let recipe = object.shape.as_ref().filter(|recipe| recipe.kind == ShapeKind::RoundedRectangle)?;
    recipe.corner.as_ref()
}

pub(crate) fn short_side(object: &Object) -> f32 {
    object.size[0].abs().min(object.size[1].abs())
}

pub(super) enum CornerEdit {
    Uniform(f32),
    Replace(CornerRecipe),
}

impl CornerEdit {
    pub(super) fn for_object(&self, object: &Object) -> Option<CornerRecipe> {
        let current = rounded_corner(object)?;
        match self {
            Self::Uniform(percent) => {
                let mut corner = current.clone();
                let radius = if corner.absolute { short_side(object) * (*percent / 100.0) } else { *percent };
                corner.radii = [radius; 4];
                Some(corner)
            }
            Self::Replace(corner) => Some(corner.clone()),
        }
    }
}

fn radius_plan(session: &Session, command: &Command) -> Result<(Vec<ObjectId>, CornerEdit), CommandError> {
    let percent = command.params["percent"].as_f64()
        .filter(|value| (0.0..=50.0).contains(value)).ok_or_else(CommandError::invalid_params)?;
    let ids = super::stroke::stroke_targets(&command.params, &session.selection.ids)?;
    if !session.has_document || ids.iter().any(|id| !session.document.get_object(*id)
        .is_some_and(|object| !object.locked && rounded_corner(object).is_some()))
    {
        return Err(CommandError::invalid_params());
    }
    Ok((ids, CornerEdit::Uniform(percent as f32)))
}

fn commit_corners(session: &mut Session, name: &str, ids: &[ObjectId], edit: &CornerEdit) -> CommandResult {
    let mut updated = session.document.clone();
    let (changed, kept_override) = super::symbols::update_corners(&mut updated, ids, edit, session.symbol_sync)?;
    if !changed {
        return Ok(json!({"doc_version": session.generation, "changes": []}));
    }
    let mut reply = json!({"ids": ids, "warnings": super::symbols::warnings(kept_override)});
    if let [id] = ids {
        reply["id"] = json!(id.0);
    }
    super::document_change::commit(session, name, updated, session.selection.ids.clone(), reply)
}

pub fn preview_recipe_field(recipe: &ShapeRecipe, key: &str, field: &str, value: f32) -> Option<ShapeRecipe> {
    let mut json = serde_json::to_value(recipe).ok()?;
    *json.get_mut(key)?.get_mut(field)? = value.into();
    let edited: ShapeRecipe = serde_json::from_value(json).ok()?;
    edited.validate().ok()?;
    Some(edited)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == CORNER_RADIUS_SET {
        return Some(radius_plan(session, command).is_ok());
    }
    matches!(command.name.as_str(),"shape.add"|"shape.params.set"|"shape.params.query").then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name == CORNER_RADIUS_SET {
        return Some(radius_plan(session, command).and_then(|(ids, edit)| commit_corners(session, &command.name, &ids, &edit)));
    }
    can_execute(session,command)?;
    Some(execute(session,command))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == CORNER_RADIUS_SET).then(|| {
        let (ids, edit) = radius_plan(session, command)?;
        let mut updated = session.document.clone();
        if super::symbols::update_corners(&mut updated, &ids, &edit, session.symbol_sync)?.0 {
            session.document = updated;
        }
        Ok(())
    })
}

fn execute(session: &mut Session, command: &Command) -> CommandResult {
    let params=&command.params;
    if command.name=="shape.params.query" {
        let id=ObjectId(params.get("id").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)?);
        return serde_json::to_value(crate::shape::query(session,id)?).map_err(|_|CommandError::invalid_params());
    }
    let before=session.document.clone();
    let select_new = command.name == "shape.add" && params["select"].as_bool() == Some(true);
    let id;
    if command.name=="shape.add" {
        let kind: ShapeKind=serde_json::from_value(params.get("kind").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
        let size=serde_json::from_value(params.get("size").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
        let triangle=if kind==ShapeKind::Triangle { Some(match params.get("triangle") {Some(value)=>serde_json::from_value::<TriangleRecipe>(value.clone()).map_err(|_|CommandError::invalid_params())?,None=>default_triangle()}) } else { None };
        let mut fields=params.as_object().cloned().unwrap_or_default();
        for key in ["pos","fill","stroke","select"] {fields.remove(key);}
        fields.insert("triangle".into(),serde_json::to_value(&triangle).map_err(|_|CommandError::invalid_params())?);
        let recipe:ShapeRecipe=serde_json::from_value(serde_json::Value::Object(fields)).map_err(|_|CommandError::invalid_params())?;recipe.validate()?;
        let mut object=Object::default();object.size=size;
        object.pos=serde_json::from_value(params.get("pos").cloned().unwrap_or_else(||serde_json::json!([0.0,0.0]))).map_err(|_|CommandError::invalid_params())?;
        if object.pos.iter().any(|v|!v.is_finite()){return Err(CommandError::invalid_params());}
        if let Some(fill)=params.get("fill"){object.fill=serde_json::from_value(fill.clone()).map_err(|_|CommandError::invalid_params())?;}
        if let Some(stroke)=params.get("stroke"){
            object.stroke=serde_json::from_value(stroke.clone()).map_err(|_|CommandError::invalid_params())?;
        } else if kind == ShapeKind::Spiral {
            object.stroke = super::curve_edit::default_stroke();
        }
        let center = [object.pos[0] + object.size[0] * 0.5, object.pos[1] + object.size[1] * 0.5];
        crate::artboard::edit::assign_new_object(&session.document, &mut object, center);
        object.shape=Some(recipe);id=session.document.create_object(object);
    } else {
        id=ObjectId(params.get("id").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)?);
        let mut recipe=crate::shape::query(session,id)?.clone();
        if let Some(value)=params.get("kind") {
            let kind:ShapeKind=serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?;
            if kind!=recipe.kind{return Err(CommandError::invalid_params());}
        }
        if let Some(value)=params.get("size"){recipe.size=serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?;}
        if let Some(value)=params.get("corner") {
            if recipe.kind!=ShapeKind::RoundedRectangle{return Err(CommandError::invalid_params());}
            recipe.corner=Some(serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?);
        }
        if let Some(value)=params.get("triangle") {
            if recipe.kind!=ShapeKind::Triangle{return Err(CommandError::invalid_params());}
            recipe.triangle=Some(serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?);
        }
        for (key,kind) in B19_RECIPE_KEYS {
            let Some(value)=params.get(key) else {continue};
            if recipe.kind!=kind{return Err(CommandError::invalid_params());}
            let mut whole=serde_json::to_value(&recipe).map_err(|_|CommandError::invalid_params())?;
            match (whole.get_mut(key),value) {
                (Some(serde_json::Value::Object(current)),serde_json::Value::Object(patch))=>{for (field,v) in patch{current.insert(field.clone(),v.clone());}}
                _=>{whole[key]=value.clone();}
            }
            recipe=serde_json::from_value(whole).map_err(|_|CommandError::invalid_params())?;
        }
        recipe.validate()?;
        if let Some(corner) = recipe.corner.as_ref()
            && params.get("corner").is_some()
            && params.as_object().is_some_and(|fields| fields.keys().all(|key| matches!(key.as_str(), "id" | "kind" | "corner")))
        {
            if session.document.get_object(id).is_some_and(|object| object.locked) {
                return Err(CommandError::invalid_params());
            }
            return commit_corners(session, &command.name, &[id], &CornerEdit::Replace(corner.clone()));
        }
        let object=session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        if params.get("size").is_some() {
            object.size = recipe.size;
        }
        object.shape = Some(recipe);
    }
    let after=session.document.clone();
    let (undo, redo) = if select_new {
        let undo = super::document_change::restore(&before, &session.selection.ids);
        let redo = super::document_change::restore(&after, &[id]);
        session.selection.ids = vec![id];
        (undo, redo)
    } else {
        (Command::with_params("core.feature.restore", json!({"document": before})),
         Command::with_params("core.feature.restore", json!({"document": after})))
    };
    session.undo.push(crate::undo::Change { name: command.name.clone(), undo, redo });
    session.generation+=1;session.document.version+=1;
    Ok(serde_json::json!({"id":id.0,"doc_version":session.generation,"changes":[{"name":command.name}]}))
}
