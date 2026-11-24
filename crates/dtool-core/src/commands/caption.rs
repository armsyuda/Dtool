use dtool_protocol::{Command, CommandError, CommandResult};
use crate::document::Document;
use crate::command::Session;
use crate::object::{Object, ObjectId, ObjectKind};

const DEFAULT_GAP: f32 = 4.0;
const DEFAULT_HEIGHT: f32 = 20.0;

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    if !matches!(cmd.name.as_str(), "caption.add" | "caption.remove") {
        return None;
    }
    let owner = cmd.params.get("owner").and_then(|v| v.as_u64()).map(ObjectId);
    Some(match (cmd.name.as_str(), owner) {
        ("caption.add", Some(owner)) => session.document.get_object(owner).is_some() && find_caption(&session.document, owner).is_none(),
        ("caption.remove", Some(owner)) => find_caption(&session.document, owner).is_some(),
        _ => false,
    })
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    if !matches!(cmd.name.as_str(), "caption.add" | "caption.remove") {
        return None;
    }
    Some((|| {
        let owner = cmd.params.get("owner").and_then(|v| v.as_u64()).map(ObjectId).ok_or_else(CommandError::invalid_params)?;
        let undo = match cmd.name.as_str() {
            "caption.add" => {
                let settings: serde_json::Value = cmd.params.get("settings").cloned().unwrap_or_default();
                let caption = add_caption(&mut session.document, owner, settings)?;
                Command::with_params("caption.remove", serde_json::json!({ "owner": owner.0, "text_node": caption.text_node.0 }))
            }
            _ => {
                let removed = remove_caption(&mut session.document, owner)?;
                let text = match &removed.kind { ObjectKind::Text(text) => text.clone(), _ => String::new() };
                let (side, gap) = layout(&removed).unwrap_or_else(|| ("below".to_string(), DEFAULT_GAP));
                Command::with_params("caption.add", serde_json::json!({ "owner": owner.0, "settings": { "text": text, "height": removed.size[1], "side": side, "gap": gap } }))
            }
        };
        session.undo.push(crate::undo::Change { name: cmd.name.clone(), redo: cmd.clone(), undo });
        session.generation += 1;
        session.document.version += 1;
        let text_node = find_caption(&session.document, owner).map(|id| id.0);
        Ok(serde_json::json!({ "doc_version": session.generation, "owner": owner.0, "text_node": text_node }))
    })())
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    let owner_id = cmd.params.get("owner").and_then(|v| v.as_u64()).map(ObjectId);
    match cmd.name.as_str() {
        "caption.add" => Some(match owner_id {
            Some(owner) => {
                let settings: serde_json::Value = cmd.params.get("settings").cloned().unwrap_or_default();
                add_caption(&mut session.document, owner, settings).map(|_| ())
            }
            None => Err(CommandError::invalid_params()),
        }),
        "caption.remove" => Some(match owner_id {
            Some(owner) => remove_caption(&mut session.document, owner).map(|_| ()),
            None => Err(CommandError::invalid_params()),
        }),
        _ => None,
    }
}

fn find_caption(doc: &Document, owner: ObjectId) -> Option<ObjectId> {
    doc.objects
        .iter()
        .find(|o| o.parent_id == Some(owner) && matches!(o.kind, ObjectKind::Text(_)))
        .map(|o| o.id)
}

fn is_table(doc: &Document, id: ObjectId) -> bool {
    doc.get_object(id).is_some_and(|object| matches!(object.kind, ObjectKind::Table(_)))
}

fn caption_text(doc: &Document, source: &Object, settings: &serde_json::Value) -> Result<String, CommandError> {
    if let Some(text) = settings.get("text").and_then(|v| v.as_str()) {
        return Ok(text.to_string());
    }
    if settings.get("from_name").and_then(|v| v.as_bool()).unwrap_or(false) {
        let crate::paint::Paint::Bitmap(bitmap) = &source.fill else { return Err(CommandError::invalid_params()) };
        let name = if source.name.trim().is_empty() { bitmap.resource_id.as_str() } else { source.name.as_str() };
        let file = name.rsplit(['/', '\\']).next().filter(|s| !s.trim().is_empty()).ok_or_else(CommandError::invalid_params)?;
        return Ok(file.to_string());
    }
    let table = matches!(source.kind, ObjectKind::Table(_));
    let format = settings.get("format").and_then(|v| v.as_str()).unwrap_or(if table { "Table {counter}" } else { "Figure {counter}" });
    let counter = 1 + doc.objects.iter().filter(|o| matches!(o.kind, ObjectKind::Text(_)) && o.parent_id.is_some_and(|p| doc.get_object(p).is_some() && is_table(doc, p) == table)).count();
    Ok(format.replace("{counter}", &counter.to_string()))
}

fn add_caption(doc: &mut Document, owner: ObjectId, settings: serde_json::Value) -> Result<CaptionResult, CommandError> {
    if find_caption(doc, owner).is_some() {
        return Err(CommandError::new("CONFLICT"));
    }
    let source = doc.get_object(owner).ok_or_else(CommandError::not_found)?.clone();
    let gap = settings.get("gap").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(DEFAULT_GAP);
    let height = settings.get("height").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(DEFAULT_HEIGHT);
    if !gap.is_finite() || !height.is_finite() || height <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let text = caption_text(doc, &source, &settings)?;
    let default_side = if matches!(source.kind, ObjectKind::Table(_)) { "above" } else { "below" };
    let side = settings.get("side").and_then(|v| v.as_str()).unwrap_or(default_side);
    if !SIDES.contains(&side) {
        return Err(CommandError::invalid_params());
    }
    let (pos, size) = placement(&source, side, gap, [crate::geometry::object_world_bounds(&source)[2].max(1.0), height]);
    let caption = Object { name: layout_name(side, gap), pos, size, kind: ObjectKind::Text(text), parent_id: Some(owner), ..Object::default() };
    let text_node = doc.create_object(caption);
    Ok(CaptionResult { text_node })
}

fn remove_caption(doc: &mut Document, owner: ObjectId) -> Result<Object, CommandError> {
    let id = find_caption(doc, owner).ok_or_else(CommandError::not_found)?;
    let removed = doc.get_object(id).ok_or_else(CommandError::not_found)?.clone();
    doc.delete_object(id)?;
    Ok(removed)
}

const SIDES: [&str; 4] = ["below", "above", "left", "right"];
const CAPTION_NAME: &str = "Caption";

fn placement(owner: &Object, side: &str, gap: f32, size: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let [x, y, w, h] = crate::geometry::object_world_bounds(owner);
    let size = if matches!(side, "below" | "above") { [w.max(1.0), size[1]] } else { size };
    let pos = match side {
        "above" => [x, y - size[1] - gap],
        "left" => [x - size[0] - gap, y],
        "right" => [x + w + gap, y],
        _ => [x, y + h + gap],
    };
    (pos, size)
}

pub fn layout_name(side: &str, gap: f32) -> String {
    format!("{CAPTION_NAME} {side} {gap}")
}

fn layout(object: &Object) -> Option<(String, f32)> {
    let mut parts = object.name.split_whitespace();
    if parts.next()? != CAPTION_NAME || !matches!(object.kind, ObjectKind::Text(_)) {
        return None;
    }
    let side = parts.next().filter(|side| SIDES.contains(side))?.to_string();
    let gap = parts.next()?.parse::<f32>().ok().filter(|gap| gap.is_finite())?;
    parts.next().is_none().then_some((side, gap))
}

pub fn follow(session: &mut Session) -> bool {
    let doc = &mut session.document;
    let moves: Vec<(ObjectId, [f32; 2], [f32; 2])> = doc
        .objects
        .iter()
        .filter_map(|caption| {
            let (side, gap) = layout(caption)?;
            let owner = doc.get_object(caption.parent_id?)?;
            let (pos, size) = placement(owner, &side, gap, caption.size);
            (pos != caption.pos || size != caption.size).then_some((caption.id, pos, size))
        })
        .collect();
    for (id, pos, size) in &moves {
        if let Some(caption) = doc.find_object_mut(id.0) {
            caption.pos = *pos;
            caption.size = *size;
        }
    }
    !moves.is_empty()
}

struct CaptionResult {
    text_node: ObjectId,
}
