use crate::command::Session;
use crate::document::Document;
use crate::object::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const LAYER_META_SET: &str = "layer.meta.set";
pub const DEFAULT_NAME_KEY: &str = "object.default_name";
const MAX_TAG_LEN: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerMeta {
    pub object: ObjectId,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub colour: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

impl LayerMeta {
    fn is_empty(&self) -> bool {
        self.tag.is_none() && self.colour.is_none() && !self.collapsed
    }
}

pub fn meta(document: &Document, id: ObjectId) -> Option<&LayerMeta> {
    document.layer_meta.iter().find(|item| item.object == id && document.get_object(id).is_some())
}

pub fn has_default_name(name: &str) -> bool {
    name.trim().is_empty()
}

pub fn text_layer_name(document: &Document, object: &crate::Object) -> Option<String> {
    let story = object.story_id()?;
    document.stories.iter().find(|entry| entry.id == story)?
        .text.lines().find(|line| !line.trim().is_empty())
        .map(|line| line.trim().chars().take(80).collect())
}

pub fn effective_tag_colour(document: &Document, id: ObjectId) -> Option<&str> {
    let tag = |id| document.layer_meta.iter().find(|entry| entry.object == id)
        .and_then(|entry| entry.colour.as_deref());
    let mut colour = tag(id);
    let mut parent = document.get_object(id)?.parent_id;
    for _ in 0..document.objects.len() {
        let Some(ancestor) = parent.and_then(|id| document.get_object(id)) else { break };
        if inherits_to_children(&ancestor.kind) {
            colour = tag(ancestor.id).or(colour);
        }
        parent = ancestor.parent_id;
    }
    colour
}

fn inherits_to_children(kind: &crate::ObjectKind) -> bool {
    matches!(kind, crate::ObjectKind::Group | crate::ObjectKind::Artboard)
}

pub fn effective_tag_colours(document: &Document) -> HashMap<ObjectId, &str> {
    let mut colours: HashMap<_, _> = document.layer_meta.iter().rev()
        .filter_map(|entry| entry.colour.as_deref().map(|colour| (entry.object, colour)))
        .collect();
    if colours.is_empty() || !document.objects.iter().any(|object| {
        inherits_to_children(&object.kind) && colours.contains_key(&object.id)
    }) {
        return colours;
    }

    let ids: HashSet<_> = document.objects.iter().map(|object| object.id).collect();
    let mut children: HashMap<_, Vec<_>> = HashMap::new();
    let mut pending = Vec::new();
    for object in &document.objects {
        if let Some(parent) = object.parent_id.filter(|parent| ids.contains(parent)) {
            children.entry(parent).or_default().push(object);
        } else {
            pending.push((object, None));
        }
    }
    let mut visited = 0;
    while let Some((object, inherited)) = pending.pop() {
        visited += 1;
        let own = colours.get(&object.id).copied();
        if let Some(colour) = inherited {
            colours.insert(object.id, colour);
        }
        let for_children = inherited.or(own.filter(|_| inherits_to_children(&object.kind)));
        if let Some(children) = children.remove(&object.id) {
            pending.extend(children.into_iter().map(|child| (child, for_children)));
        }
    }
    if visited != document.objects.len() {
        // Malformed legacy trees can contain cycles. Keep the bounded single-row lookup for them.
        for object in children.into_values().flatten() {
            if let Some(colour) = effective_tag_colour(document, object.id) {
                colours.insert(object.id, colour);
            }
        }
    }
    colours
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == LAYER_META_SET).then(|| session.has_document && parse(&session.document, &command.params).is_ok())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    (command.name == LAYER_META_SET).then(|| set(session, &command.params))
}

#[derive(Debug, Clone, PartialEq)]
struct Patch {
    objects: Vec<ObjectId>,
    tag: Option<Option<String>>,
    colour: Option<Option<String>>,
    collapsed: Option<bool>,
}

fn optional_text(params: &Value, key: &str, check: fn(&str) -> bool) -> Result<Option<Option<String>>, CommandError> {
    match params.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                Ok(Some(None))
            } else if check(text) {
                Ok(Some(Some(text.to_string())))
            } else {
                Err(CommandError::invalid_params())
            }
        }
        Some(_) => Err(CommandError::invalid_params()),
    }
}

fn valid_tag(text: &str) -> bool {
    text.chars().count() <= MAX_TAG_LEN
}

fn valid_colour(text: &str) -> bool {
    let Some(hex) = text.strip_prefix('#') else { return false };
    matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

pub(super) fn read_tag_colour(params: &Value) -> Result<Option<String>, CommandError> {
    optional_text(params, "colour", valid_colour)?.ok_or_else(CommandError::invalid_params)
}

fn parse(document: &Document, params: &Value) -> Result<Patch, CommandError> {
    let single_keys = ["object", "id", "node"];
    let objects = if let Some(values) = params.get("ids") {
        if single_keys.iter().any(|key| params.get(*key).is_some()) {
            return Err(CommandError::invalid_params());
        }
        let values = values.as_array().filter(|ids| !ids.is_empty()).ok_or_else(CommandError::invalid_params)?;
        let mut seen = HashSet::new();
        values.iter().map(|value| value.as_u64().map(ObjectId).ok_or_else(CommandError::invalid_params))
            .collect::<Result<Vec<_>, _>>()?.into_iter().filter(|id| seen.insert(*id)).collect()
    } else {
        let id = single_keys.iter().find_map(|key| params.get(*key).and_then(Value::as_u64))
            .ok_or_else(CommandError::invalid_params)?;
        vec![ObjectId(id)]
    };
    for &object in &objects {
        document.get_object(object).ok_or_else(CommandError::not_found)?;
    }
    let tag = optional_text(params, "tag", valid_tag)?;
    let colour = optional_text(params, "colour", valid_colour)?;
    let collapsed = match params.get("collapsed") {
        None => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => return Err(CommandError::invalid_params()),
    };
    if tag.is_none() && colour.is_none() && collapsed.is_none() {
        return Err(CommandError::invalid_params());
    }
    Ok(Patch { objects, tag, colour, collapsed })
}

fn apply(document: &mut Document, patch: &Patch) -> Vec<LayerMeta> {
    let existing: HashSet<_> = document.objects.iter().map(|object| object.id).collect();
    let targets: HashSet<_> = patch.objects.iter().copied().collect();
    let mut replaced = std::collections::HashMap::new();
    document.layer_meta.retain(|item| {
        if targets.contains(&item.object) {
            replaced.entry(item.object).or_insert_with(|| item.clone());
            return false;
        }
        existing.contains(&item.object)
    });
    let entries: Vec<_> = patch.objects.iter().map(|&object| {
        let mut entry = replaced.remove(&object).unwrap_or(LayerMeta { object, ..LayerMeta::default() });
        if let Some(tag) = &patch.tag { entry.tag = tag.clone(); }
        if let Some(colour) = &patch.colour { entry.colour = colour.clone(); }
        if let Some(collapsed) = patch.collapsed { entry.collapsed = collapsed; }
        entry
    }).collect();
    document.layer_meta.extend(entries.iter().filter(|entry| !entry.is_empty()).cloned());
    document.layer_meta.sort_by_key(|item| item.object.0);
    entries
}

fn set(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let patch = parse(&session.document, params)?;
    let mut document = session.document.clone();
    let entries = apply(&mut document, &patch);
    let extra = if params.get("ids").is_some() {
        json!({"entries": entries})
    } else {
        json!(entries[0])
    };
    if patch.tag.is_none() && patch.colour.is_none() {
        session.document.layer_meta = document.layer_meta;
        session.document.version += 1;
        session.generation += 1;
        let mut reply = json!({"doc_version": session.generation, "changes": []});
        if let (Some(target), Some(source)) = (reply.as_object_mut(), extra.as_object()) { target.extend(source.clone()); }
        return Ok(reply);
    }
    let selection = session.selection.ids.clone();
    crate::commands::document_change::commit(session, LAYER_META_SET, document, selection, extra)
}
