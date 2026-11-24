use crate::command::Session;
use crate::object::{Object, ObjectId};
use crate::paint::{Paint, PaintOrder};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::Deserialize;
use serde_json::json;

pub const PAINT_ORDER_SET: &str = "paint.order.set";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    order: PaintOrder,
    ids: Option<Vec<u64>>,
    object_id: Option<u64>,
}

fn targets(session: &Session, command: &Command) -> Result<(PaintOrder, Vec<ObjectId>), CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let args: Args = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    let mut ids = args.ids.map(|ids| ids.into_iter().map(ObjectId).collect::<Vec<_>>())
        .or_else(|| args.object_id.map(|id| vec![ObjectId(id)]))
        .unwrap_or_else(|| session.selection.ids.clone());
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    if ids.is_empty() || ids.iter().any(|id| !session.document.objects.iter().any(|object| object.id == *id && !object.locked)) {
        return Err(CommandError::invalid_params());
    }
    Ok((args.order, ids))
}

pub const PAINT_SWAP: &str = "paint.swap";
pub const PAINT_DEFAULT: &str = "paint.default";
pub const PAINT_NONE: &str = "paint.none";

fn well_command(name: &str) -> bool {
    matches!(name, PAINT_SWAP | PAINT_DEFAULT | PAINT_NONE)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WellRole {
    Fill,
    Stroke,
    TextFill,
    TextStroke,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WellArgs {
    ids: Option<Vec<u64>>,
    object_id: Option<u64>,
    role: Option<WellRole>,
}

fn well_targets(session: &Session, command: &Command) -> Result<(Vec<ObjectId>, Option<WellRole>), CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let args: WellArgs = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
    let mut ids = args.ids.map(|ids| ids.into_iter().map(ObjectId).collect::<Vec<_>>())
        .or_else(|| args.object_id.map(|id| vec![ObjectId(id)]))
        .unwrap_or_else(|| session.selection.ids.clone());
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    if ids.is_empty() || ids.iter().any(|id| !session.document.objects.iter().any(|object| object.id == *id && !object.locked)) {
        return Err(CommandError::invalid_params());
    }
    match (command.name.as_str(), args.role) {
        (PAINT_NONE, None) => return Err(CommandError::invalid_params()),
        (PAINT_SWAP | PAINT_DEFAULT, Some(_)) => return Err(CommandError::invalid_params()),
        _ => {}
    }
    Ok((ids, args.role))
}

#[derive(Default)]
struct WellChange {
    objects: Vec<Object>,
    stories: Vec<crate::document::Story>,
}

fn change_wells(session: &mut Session, command: &Command) -> Result<(Vec<ObjectId>, WellChange), CommandError> {
    let (ids, role) = well_targets(session, command)?;
    let defaults = super::defaults::current();
    let default_stroke = defaults.stroke_for_document(&session.document);
    let mut old = WellChange::default();
    if matches!(role, Some(WellRole::TextFill | WellRole::TextStroke)) {
        let mut stories: Vec<crate::document::StoryId> = ids.iter().filter_map(|id| session.document.get_object(*id).and_then(|object| object.story_id())).collect();
        if stories.len() != ids.len() {
            return Err(CommandError::invalid_params());
        }
        stories.sort_by_key(|story| story.0);
        stories.dedup();
        if stories.len() != 1 {
            return Err(CommandError::invalid_params());
        }
        for story in &mut session.document.stories {
            if !stories.contains(&story.id) || old.stories.iter().any(|item| item.id == story.id) {
                continue;
            }
            let before = story.clone();
            for range in story.ranges.iter_mut() {
                if role == Some(WellRole::TextFill) {
                    crate::paint::set_range_fill(range, None);
                } else if let Some(map) = range.as_object_mut() {
                    if map.get("outline").is_some_and(|value| !value.is_null()) {
                        map.remove("outline");
                    }
                }
            }
            if *story != before {
                old.stories.push(before);
            }
        }
        return Ok((ids, old));
    }
    for object in &mut session.document.objects {
        if !ids.contains(&object.id) {
            continue;
        }
        let before = object.clone();
        match (command.name.as_str(), role) {
            (PAINT_SWAP, _) => std::mem::swap(&mut object.fill, &mut object.stroke.paint),
            (PAINT_DEFAULT, _) => {
                object.fill = defaults.fill.clone();
                object.stroke = default_stroke.clone();
            }
            (_, Some(WellRole::Fill)) => object.fill = Paint::None,
            (_, Some(WellRole::Stroke)) => object.stroke.paint = Paint::None,
            _ => {}
        }
        if object.fill != before.fill || object.stroke != before.stroke {
            old.objects.push(before);
        }
    }
    Ok((ids, old))
}

fn run_wells(session: &mut Session, command: &Command) -> CommandResult {
    let (ids, old) = change_wells(session, command)?;
    let id_values: Vec<u64> = ids.iter().map(|id| id.0).collect();
    if !old.objects.is_empty() || !old.stories.is_empty() {
        let mut redo = command.clone();
        redo.params["ids"] = json!(id_values);
        if let Some(map) = redo.params.as_object_mut() {
            map.remove("object_id");
        }
        let undo = if old.stories.is_empty() {
            Command::with_params("object.restore", json!({"objects": old.objects}))
        } else {
            Command::with_params("text.story.restore", json!({"story": old.stories[0]}))
        };

        session.undo.push(crate::undo::Change { name: command.name.clone(), undo, redo });
        session.generation += 1;
        session.document.version += 1;
    }
    Ok(json!({"doc_version": session.generation, "changes": [{"name": command.name, "ids": id_values}]}))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if well_command(&command.name) {
        return Some(well_targets(session, command).is_ok());
    }
    (command.name == PAINT_ORDER_SET).then(|| targets(session, command).is_ok())
}

fn change(session: &mut Session, command: &Command) -> Result<Vec<Object>, CommandError> {
    let (order, ids) = targets(session, command)?;
    let mut old = Vec::new();
    for object in &mut session.document.objects {
        if ids.contains(&object.id) && (object.paint_order != order || !object.appearance_stack.is_empty()) {
            old.push(object.clone());
            object.paint_order = order;
            object.appearance_stack.clear();
        }
    }
    Ok(old)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if well_command(&command.name) {
        return Some(change_wells(session, command).map(|_| ()));
    }
    (command.name == PAINT_ORDER_SET).then(|| change(session, command).map(|_| ()))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if well_command(&command.name) {
        return Some(run_wells(session, command));
    }
    if command.name != PAINT_ORDER_SET { return None; }
    Some((|| {
        let old = change(session, command)?;
        let ids: Vec<u64> = old.iter().map(|object| object.id.0).collect();
        if !old.is_empty() {
            let undo = Command::with_params("object.restore", json!({"objects": old}));
            let redo = Command::with_params(PAINT_ORDER_SET, json!({"ids": ids, "order": command.params["order"]}));
            session.undo.push(crate::undo::Change { name: PAINT_ORDER_SET.into(), undo, redo });
            session.generation += 1;
            session.document.version += 1;
        }
        Ok(json!({"doc_version": session.generation, "changes": [{"name": PAINT_ORDER_SET, "ids": ids}]}))
    })())
}
