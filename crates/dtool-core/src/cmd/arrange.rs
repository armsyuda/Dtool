use crate::blend::BlendMode;
use crate::command::Session;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::selection::SelectOp;
use crate::undo::Change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const NODE_GROUP: &str = "node.group";
pub const NODE_FOLDER_CREATE: &str = "node.folder.create";
pub const NODE_FOLDER_PROMOTE: &str = "node.folder.promote";
pub const NODE_UNGROUP: &str = "node.ungroup";
pub const NODE_ARRANGE: &str = "node.arrange";
pub const NODE_LOCK_STATE: &str = "node.lock_state";
pub const NODE_UNLOCK_ALL: &str = "node.unlock_all";
pub const NODE_UPDATE: &str = "node.update";
pub const NODE_SHOW_ALL: &str = "node.show_all";
pub const NODE_HIDE_OTHERS: &str = "node.hide_others";
pub const NODE_SHOW_OTHERS: &str = "node.show_others";
pub const NODE_DELETE: &str = "node.delete";
pub const NODE_SNAPSHOT_RESTORE: &str = "node.snapshot.restore";
pub const OBJECT_DELETE: &str = "object.delete";
pub const EDIT_DELETE: &str = "edit.delete";
pub const NODE_TREE_QUERY: &str = "node.tree.query";
pub const NODE_REPARENT: &str = "node.reparent";
pub const NODE_SELECT_INSIDE: &str = "node.select_inside";
pub const LOCKED_NEEDS_CONFIRM: &str = "LOCKED_NEEDS_CONFIRM";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrangeStep {
    ToFront,
    Forward,
    Backward,
    ToBack,
}

impl ArrangeStep {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "to_front" | "front" | "ToFront" => Some(ArrangeStep::ToFront),
            "forward" | "forward_one" | "Forward" => Some(ArrangeStep::Forward),
            "backward" | "back_one" | "Backward" => Some(ArrangeStep::Backward),
            "to_back" | "back" | "ToBack" => Some(ArrangeStep::ToBack),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockState {
    Locked,
    Unlocked,
}

impl LockState {
    pub fn parse(value: &Value) -> Option<Self> {
        if let Some(flag) = value.as_bool() {
            return Some(if flag { LockState::Locked } else { LockState::Unlocked });
        }
        match value.as_str()? {
            "locked" | "lock" | "Locked" => Some(LockState::Locked),
            "unlocked" | "unlock" | "Unlocked" => Some(LockState::Unlocked),
            _ => None,
        }
    }

    pub fn is_locked(self) -> bool {
        self == LockState::Locked
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSnapshot {
    pub objects: Vec<Object>,
    pub next_object_id: u64,
    pub selection: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_key: Option<ObjectId>,
    #[serde(default)]
    pub frame_chains: Option<Vec<crate::frame::FrameChain>>,
    #[serde(default)]
    pub pixel_layers: Option<crate::pixel::PixelLedger>,
    #[serde(default)]
    pub placed_images: Option<Vec<crate::resources::PlacedImage>>,
    #[serde(default)]
    pub layer_masks: Option<Vec<crate::raster_source::LayerMask>>,
    #[serde(default)]
    pub stories: Option<Vec<crate::document::Story>>,
    #[serde(default)]
    story_marks: Option<crate::longdoc::marks::StoryMarks>,
    #[serde(default)]
    notes_endnote_frame: Option<u64>,
    #[serde(default)]
    pub toc_records: Option<Vec<crate::longdoc::toc::TocRecord>>,
    #[serde(default)]
    pub index_stories: Option<Vec<u64>>,
    #[serde(default)]
    pub resources: Option<Vec<crate::resources::ImageResource>>,
    #[serde(default)]
    pub next_page_id: Option<u64>,
    #[serde(default)]
    pub next_story_id: Option<u64>,
    #[serde(default)]
    pub next_resource_id: Option<u64>,
    #[serde(default)]
    pub swatch_bindings: Option<Vec<crate::colour::swatch::SwatchBinding>>,
    #[serde(default)]
    pub text_links: Option<Vec<crate::text_link::TextLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbols: Option<crate::commands::symbols::Symbols>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_bindings: Option<Vec<crate::data_merge::MergeBinding>>,
}

pub fn is_arrange_command(name: &str) -> bool {
    matches!(
        name,
        NODE_GROUP | NODE_FOLDER_CREATE | NODE_FOLDER_PROMOTE | NODE_UNGROUP | NODE_ARRANGE | NODE_LOCK_STATE | NODE_UNLOCK_ALL | NODE_SHOW_ALL | NODE_DELETE | OBJECT_DELETE | EDIT_DELETE
            | NODE_REPARENT | NODE_SELECT_INSIDE | NODE_HIDE_OTHERS | NODE_SHOW_OTHERS
    )
}

fn is_state_update(command: &Command) -> bool {
    if command.name != NODE_UPDATE {
        return false;
    }
    let fields = match command.params.get("set").and_then(Value::as_object) {
        Some(fields) => fields,
        None => return false,
    };
    !fields.is_empty() && fields.keys().all(|key| matches!(key.as_str(), "visible" | "locked"))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name == NODE_SNAPSHOT_RESTORE || command.name == NODE_TREE_QUERY {
        return Some(session.has_document);
    }
    if command.name == NODE_FOLDER_CREATE
        || (command.name == NODE_GROUP && command.params.get("empty").and_then(Value::as_bool).unwrap_or(false))
    {
        return Some(session.has_document);
    }
    if !is_arrange_command(&command.name) && !is_state_update(command) {
        return None;
    }
    let has_objects = !session.document.objects.is_empty();
    let ready = match command.name.as_str() {
        NODE_FOLDER_PROMOTE => promotable_groups(session, command).is_ok(),
        NODE_UNLOCK_ALL => session.document.objects.iter().any(|obj| obj.locked),
        NODE_SHOW_ALL => session.document.objects.iter().any(|obj| !obj.pickable),
        NODE_HIDE_OTHERS | NODE_SHOW_OTHERS => session.has_document && other_visibility_targets(session, command)
            .is_ok_and(|ids| !ids.is_empty()),
        EDIT_DELETE => !session.selection.ids.is_empty(),
        NODE_SELECT_INSIDE => has_objects || !session.selection.ids.is_empty(),
        _ => has_objects,
    };
    Some(ready)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name == NODE_TREE_QUERY {
        return Some(tree_query(session, command));
    }
    if is_state_update(command) {
        return Some(node_update(session, command));
    }
    let result = match command.name.as_str() {
        NODE_GROUP => group(session, command),
        NODE_FOLDER_CREATE => create_folder(session, command),
        NODE_FOLDER_PROMOTE => promote_groups(session, command),
        NODE_UNGROUP => ungroup(session, command),
        NODE_ARRANGE => arrange(session, command),
        NODE_LOCK_STATE => lock_state(session, command),
        NODE_UNLOCK_ALL => unlock_all(session),
        NODE_SHOW_ALL => show_all(session),
        NODE_HIDE_OTHERS | NODE_SHOW_OTHERS => set_other_visibility(session, command),
        NODE_DELETE | EDIT_DELETE => delete(session, command, true),
        OBJECT_DELETE => delete(session, command, false),
        NODE_REPARENT => reparent(session, command),
        NODE_SELECT_INSIDE => select_inside(session, command),
        _ => return None,
    };
    Some(result)
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != NODE_SNAPSHOT_RESTORE {
        return None;
    }
    let snapshot: NodeSnapshot = match serde_json::from_value(command.params.clone()) {
        Ok(snapshot) => snapshot,
        Err(_) => return Some(Err(CommandError::invalid_params())),
    };
    restore(session, &snapshot);
    session.document.version += 1;
    Some(Ok(()))
}

pub fn snapshot(session: &Session) -> NodeSnapshot {
    NodeSnapshot {
        objects: session.document.objects.clone(),
        next_object_id: session.document.next_object_id,
        selection: session.selection.ids.iter().map(|id| id.0).collect(),
        selection_key: session.selection.explicit_key(),
        frame_chains: Some(session.document.frame_chains.clone()),
        pixel_layers: Some(session.document.pixel_layers.clone()),
        placed_images: Some(session.document.placed_images.clone()),
        layer_masks: Some(session.document.layer_masks.clone()),
        stories: Some(session.document.stories.clone()),
        story_marks: Some(crate::longdoc::marks::StoryMarks::snapshot(&session.document)),
        notes_endnote_frame: session.document.notes.endnote_frame,
        toc_records: Some(session.document.toc.tocs.clone()),
        index_stories: Some(session.document.index.indexes.clone()),
        resources: Some(session.document.resources.clone()),
        next_page_id: Some(session.document.next_page_id),
        next_story_id: Some(session.document.next_story_id),
        next_resource_id: Some(session.document.next_resource_id),
        swatch_bindings: Some(session.document.swatch_bindings.clone()),
        text_links: Some(session.document.text_links.clone()),
        symbols: (!session.document.symbols.is_empty()).then(|| session.document.symbols.clone()),
        merge_bindings: Some(session.document.merge_bindings.clone()),
    }
}

pub fn restore(session: &mut Session, snapshot: &NodeSnapshot) {
    let next_object_id = session.document.next_object_id;
    let next_page_id = session.document.next_page_id;
    let next_story_id = session.document.next_story_id;
    let next_resource_id = session.document.next_resource_id;
    session.document.objects = snapshot.objects.clone();
    if let Some(chains) = &snapshot.frame_chains {
        session.document.frame_chains = chains.clone();
    }
    if let Some(layers) = &snapshot.pixel_layers { session.document.pixel_layers = layers.clone(); }
    if let Some(images) = &snapshot.placed_images { session.document.placed_images = images.clone(); }
    if let Some(masks) = &snapshot.layer_masks { session.document.layer_masks = masks.clone(); }
    if let Some(stories) = &snapshot.stories { session.document.stories = stories.clone(); }
    if let Some(marks) = &snapshot.story_marks {
        marks.clone().restore(&mut session.document);
        session.document.notes.endnote_frame = snapshot.notes_endnote_frame;
    }
    if let Some(records) = &snapshot.toc_records { session.document.toc.tocs.clone_from(records); }
    if let Some(stories) = &snapshot.index_stories { session.document.index.indexes.clone_from(stories); }
    if let Some(resources) = &snapshot.resources { session.document.resources = resources.clone(); }
    if let Some(bindings) = &snapshot.swatch_bindings { session.document.swatch_bindings = bindings.clone(); }
    if let Some(links) = &snapshot.text_links { session.document.text_links = links.clone(); }
    if let Some(symbols) = &snapshot.symbols { session.document.symbols = symbols.clone(); }
    if let Some(bindings) = &snapshot.merge_bindings { session.document.merge_bindings = bindings.clone(); }
    session.document.next_object_id = next_object_id.max(snapshot.next_object_id);
    if let Some(value) = snapshot.next_page_id {
        session.document.next_page_id = next_page_id.max(value);
    }
    if let Some(value) = snapshot.next_story_id {
        session.document.next_story_id = next_story_id.max(value);
    }
    if let Some(value) = snapshot.next_resource_id {
        session.document.next_resource_id = next_resource_id.max(value);
    }
    session.document.raise_id_high_water();
    let selection = snapshot
        .selection
        .iter()
        .map(|id| ObjectId(*id))
        .filter(|id| session.document.get_object(*id).is_some())
        .collect();
    session.selection.set(selection, SelectOp::Replace);
    if let Some(key) = snapshot.selection_key {
        session.selection.set_key(key);
    }
}

fn restore_command(snapshot: &NodeSnapshot) -> Command {
    Command::with_params(NODE_SNAPSHOT_RESTORE, serde_json::to_value(snapshot).unwrap_or_default())
}

pub fn commit_snapshot(session: &mut Session, name: &str, before: NodeSnapshot, extra: Value) -> CommandResult {
    commit(session, name, before, extra)
}

pub(crate) fn commit(session: &mut Session, name: &str, before: NodeSnapshot, mut extra: Value) -> CommandResult {
    use crate::clipboard::{EDIT_CUT, EDIT_DUPLICATE, EDIT_PASTE_INSIDE, EDIT_POWER_DUPLICATE};
    use crate::commands::duplicate::EDIT_DUPLICATE_DRAG;

    if before.symbols.is_some() && matches!(name,
        NODE_ARRANGE | NODE_REPARENT | NODE_DELETE | OBJECT_DELETE | EDIT_DELETE
        | NODE_GROUP | NODE_FOLDER_CREATE | NODE_UNGROUP | EDIT_CUT | EDIT_PASTE_INSIDE
        | EDIT_DUPLICATE | EDIT_DUPLICATE_DRAG | EDIT_POWER_DUPLICATE | crate::clipboard::move_duplicate::MOVE_DUPLICATE
    ) {
        let kept = crate::commands::symbols::structure::sync(&mut session.document, &before.objects, session.symbol_sync);
        session.selection.ids.retain(|id| session.document.get_object(*id).is_some());
        if kept {
            extra["warnings"] = json!(crate::commands::symbols::warnings(true));
        }
    }
    if name == crate::clipboard::EDIT_CUT && session.document.objects.iter().any(|object| {
        object.path_text_data().is_some_and(|text| {
            session.document.get_object(text.path).is_none()
                && before.objects.iter().any(|old| old.id == text.path)
        })
    }) {
        restore(session, &before);
        return Err(CommandError::new("CLIPBOARD_PATH_IN_USE"));
    }
    let after = snapshot(session);
    session.undo.push(Change {
        name: name.to_string(),
        redo: restore_command(&after),
        undo: restore_command(&before),
    });
    session.generation += 1;
    session.document.version += 1;
    let mut reply = json!({
        "doc_version": session.generation,
        "changes": [{ "name": name }]
    });
    if let (Some(target), Some(source)) = (reply.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    Ok(reply)
}

fn parse_id(value: &Value) -> Option<ObjectId> {
    if let Some(id) = value.as_u64() {
        return Some(ObjectId(id));
    }
    let raw = value.as_str()?;
    let raw = raw.strip_prefix("node:").unwrap_or(raw);
    raw.parse::<u64>().ok().map(ObjectId)
}

pub(crate) fn listed_ids(command: &Command) -> Result<Option<Vec<ObjectId>>, CommandError> {
    for key in ["nodes", "target", "ids"] {
        if let Some(value) = command.params.get(key) {
            let list = value.as_array().ok_or_else(CommandError::invalid_params)?;
            let mut ids = Vec::with_capacity(list.len());
            for item in list {
                ids.push(parse_id(item).ok_or_else(CommandError::invalid_params)?);
            }
            if !ids.is_empty() {
                return Ok(Some(ids));
            }
        }
    }
    for key in ["node", "id"] {
        if let Some(value) = command.params.get(key) {
            if let Some(id) = parse_id(value) {
                return Ok(Some(vec![id]));
            }
        }
    }
    Ok(None)
}

pub(crate) fn target_ids(session: &Session, command: &Command) -> Result<Vec<ObjectId>, CommandError> {
    let ids = match listed_ids(command)? {
        Some(ids) => ids,
        None => session.selection.ids.clone(),
    };
    if ids.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let mut unique: Vec<ObjectId> = Vec::with_capacity(ids.len());
    for id in ids {
        if session.document.get_object(id).is_none() {
            return Err(CommandError::not_found());
        }
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    Ok(unique)
}

pub(crate) fn descendants(session: &Session, roots: &[ObjectId]) -> Vec<ObjectId> {
    descendant_ids(&session.document, roots)
}

pub(crate) fn descendant_ids(doc: &crate::Document, roots: &[ObjectId]) -> Vec<ObjectId> {
    let mut found: Vec<ObjectId> = roots.to_vec();
    let mut index = 0;
    while index < found.len() {
        let parent = found[index];
        for obj in doc.objects() {
            if obj.parent_id == Some(parent) && !found.contains(&obj.id) {
                found.push(obj.id);
            }
        }
        index += 1;
    }
    found
}

fn object_mut(session: &mut Session, id: ObjectId) -> Option<&mut Object> {
    session.document.objects.iter_mut().find(|obj| obj.id == id)
}

pub(crate) fn bounds(session: &Session, ids: &[ObjectId]) -> ([f32; 2], [f32; 2]) {
    let mut min = [f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN];
    for id in ids {
        if let Some(obj) = session.document.get_object(*id) {
            let x0 = obj.pos[0].min(obj.pos[0] + obj.size[0]);
            let y0 = obj.pos[1].min(obj.pos[1] + obj.size[1]);
            let x1 = obj.pos[0].max(obj.pos[0] + obj.size[0]);
            let y1 = obj.pos[1].max(obj.pos[1] + obj.size[1]);
            min = [min[0].min(x0), min[1].min(y0)];
            max = [max[0].max(x1), max[1].max(y1)];
        }
    }
    if min[0] > max[0] || min[1] > max[1] {
        return ([0.0, 0.0], [0.0, 0.0]);
    }
    (min, [max[0] - min[0], max[1] - min[1]])
}

fn create_folder(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::invalid_params());
    }
    let name = command.params.get("name").map(|value| {
        value.as_str().map(str::trim).filter(|name| !name.is_empty()).ok_or_else(CommandError::invalid_params)
    }).transpose()?.unwrap_or("Layer Folder");
    let anchor = session.document.objects.iter().enumerate().rfind(|(_, object)| {
        session.selection.ids.contains(&object.id)
    });
    let (at, parent_id, page_id) = match anchor {
        Some((index, board)) if crate::artboard::is_artboard(board) => {
            (index + 1, Some(board.id), board.page_id)
        }
        Some((index, object)) => (index + 1, object.parent_id, object.page_id),
        None => (session.document.objects.len(), None, None),
    };
    let before = snapshot(session);
    let folder = session.document.create_object(Object {
        name: name.to_owned(),
        kind: ObjectKind::Group,
        layer_folder: true,
        parent_id,
        page_id,
        blend_mode: BlendMode::PassThrough,
        ..Object::default()
    });
    session.document.objects[at..].rotate_right(1);
    session.selection.ids = vec![folder];
    commit(session, NODE_FOLDER_CREATE, before, json!({ "folder": folder.0 }))
}

fn promotable_groups(session: &Session, command: &Command) -> Result<Vec<ObjectId>, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let ids = target_ids(session, command)?;
    let doc = &session.document;
    if ids.iter().any(|id| {
        let Some(group) = doc.get_object(*id) else { return true };
        !group.is_group() || group.layer_folder || group.locked || group.tone_blend.is_some()
            || crate::artboard::is_artboard(group)
            || doc.master_pages.assignments.iter().any(|assignment| assignment.group == Some(*id))
            || doc.master_pages.source_of(*id).is_some()
            || doc.symbols.instances.iter().any(|instance| instance.root == *id)
            || crate::commands::symbols::object_link(doc, *id).is_some()
    }) {
        return Err(CommandError::invalid_params());
    }
    Ok(ids)
}

fn promote_groups(session: &mut Session, command: &Command) -> CommandResult {
    let ids = promotable_groups(session, command)?;
    let before = snapshot(session);
    for group in &mut session.document.objects {
        if ids.contains(&group.id) {
            group.layer_folder = true;
        }
    }
    commit(session, NODE_FOLDER_PROMOTE, before, json!({ "nodes": ids }))
}

fn group(session: &mut Session, command: &Command) -> CommandResult {
    let empty = command.params.get("empty").and_then(Value::as_bool).unwrap_or(false);
    let ids = if empty {
        listed_ids(command)?.unwrap_or_default()
    } else {
        let ids = target_ids(session, command)?;
        if ids.len() < 2 {
            return Err(CommandError::invalid_params());
        }
        ids
    };
    let name = command.params.get("name").and_then(Value::as_str)
        .filter(|name| !name.is_empty()).unwrap_or("Group");
    create_group(session, NODE_GROUP, &ids, name, None)
}

pub(crate) fn create_group(
    session: &mut Session,
    command: &str,
    ids: &[ObjectId],
    name: &str,
    tone_blend: Option<crate::tone_blend::ToneBlend>,
) -> CommandResult {
    let board = ids.first().and_then(|id| crate::artboard::owner(&session.document, *id));
    let board_id = board.map(|board| board.id);
    if ids.iter().any(|id| session.document.get_object(*id).is_some_and(crate::artboard::is_artboard)
        || crate::artboard::owner(&session.document, *id).map(|board| board.id) != board_id)
    {
        return Err(CommandError::invalid_params());
    }
    let page_id = board.and_then(|board| board.page_id).or_else(|| {
        tone_blend.and_then(|_| ids.first().and_then(|id| session.document.get_object(*id)).and_then(|object| object.page_id))
    });
    let before = snapshot(session);
    let parent = ids.first().and_then(|id| session.document.get_object(*id)).and_then(|obj| obj.parent_id);
    let (pos, size) = bounds(session, ids);
    let top = session
        .document
        .objects
        .iter()
        .rposition(|obj| ids.contains(&obj.id))
        .unwrap_or(session.document.objects.len());
    let group = Object {
        name: name.to_owned(),
        tone_blend,
        pos,
        size,
        kind: ObjectKind::Group,
        parent_id: parent,
        page_id,
        blend_mode: BlendMode::PassThrough,
        ..Object::default()
    };
    let group_id = session.document.create_object(group);
    if let Some(created) = session.document.objects.pop() {
        let at = (top + 1).min(session.document.objects.len());
        session.document.objects.insert(at, created);
    }
    for id in ids {
        if let Some(obj) = object_mut(session, *id) {
            obj.parent_id = Some(group_id);
        }
    }
    session.selection.ids = vec![group_id];
    commit(session, command, before, json!({ "group": group_id.0 }))
}

fn ungroup_one(session: &mut Session, group_id: ObjectId) -> Vec<ObjectId> {
    let parent = session.document.get_object(group_id).and_then(|obj| obj.parent_id);
    let mut released = Vec::new();
    for obj in session.document.objects.iter_mut() {
        if obj.parent_id == Some(group_id) {
            obj.parent_id = parent;
            released.push(obj.id);
        }
    }
    session.document.objects.retain(|obj| obj.id != group_id);
    session.document.layer_masks.retain(|mask| mask.object != group_id);
    released
}

fn is_group(session: &Session, id: ObjectId) -> bool {
    matches!(session.document.get_object(id).map(|obj| &obj.kind), Some(ObjectKind::Group))
}

fn ungroup(session: &mut Session, command: &Command) -> CommandResult {
    let all = command.params.get("all").and_then(Value::as_bool).unwrap_or(false);
    let groups: Vec<ObjectId> = if all {
        session
            .document
            .objects
            .iter()
            .filter(|obj| obj.is_group() && !obj.is_layer_folder())
            .map(|obj| obj.id)
            .collect()
    } else {
        let ids = target_ids(session, command)?;
        if ids.iter().any(|id| !is_group(session, *id)) {
            return Err(CommandError::invalid_params());
        }
        ids
    };
    if groups.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let before = snapshot(session);
    let mut released = Vec::new();
    for group_id in &groups {
        for id in ungroup_one(session, *group_id) {
            if !groups.contains(&id) && !released.contains(&id) {
                released.push(id);
            }
        }
    }
    released.retain(|id| session.document.get_object(*id).is_some());
    session.selection.ids = released.clone();
    let nodes: Vec<u64> = released.iter().map(|id| id.0).collect();
    commit(session, NODE_UNGROUP, before, json!({ "nodes": nodes }))
}

fn arrange(session: &mut Session, command: &Command) -> CommandResult {
    let step = command
        .params
        .get("step")
        .or_else(|| command.params.get("order"))
        .and_then(Value::as_str)
        .and_then(ArrangeStep::parse)
        .ok_or_else(CommandError::invalid_params)?;
    let ids = target_ids(session, command)?;
    let before = snapshot(session);
    let mut parents: Vec<Option<ObjectId>> = Vec::new();
    for id in &ids {
        if let Some(parent) = session.document.get_object(*id).map(|obj| obj.parent_id) {
            if !parents.contains(&parent) {
                parents.push(parent);
            }
        }
    }
    for parent in parents {
        arrange_siblings(&mut session.document.objects, parent, &ids, step);
    }
    commit(session, NODE_ARRANGE, before, json!({}))
}

fn arrange_siblings(objects: &mut Vec<Object>, parent: Option<ObjectId>, ids: &[ObjectId], step: ArrangeStep) {
    let slots: Vec<usize> = objects.iter().enumerate().filter(|(_, obj)| obj.parent_id == parent).map(|(index, _)| index).collect();
    let mut order: Vec<Object> = slots.iter().map(|index| objects[*index].clone()).collect();
    match step {
        ArrangeStep::ToFront => {
            let (moved, rest): (Vec<Object>, Vec<Object>) = order.drain(..).partition(|obj| ids.contains(&obj.id));
            order = rest;
            order.extend(moved);
        }
        ArrangeStep::ToBack => {
            let (moved, rest): (Vec<Object>, Vec<Object>) = order.drain(..).partition(|obj| ids.contains(&obj.id));
            order = moved;
            order.extend(rest);
        }
        ArrangeStep::Forward => {
            let mut index = order.len();
            while index > 1 {
                index -= 1;
                if ids.contains(&order[index - 1].id) && !ids.contains(&order[index].id) {
                    order.swap(index - 1, index);
                }
            }
        }
        ArrangeStep::Backward => {
            for index in 1..order.len() {
                if ids.contains(&order[index].id) && !ids.contains(&order[index - 1].id) {
                    order.swap(index - 1, index);
                }
            }
        }
    }
    for (slot, obj) in slots.into_iter().zip(order) {
        objects[slot] = obj;
    }
}

fn lock_state(session: &mut Session, command: &Command) -> CommandResult {
    let state = command
        .params
        .get("state")
        .or_else(|| command.params.get("locked"))
        .and_then(LockState::parse)
        .ok_or_else(CommandError::invalid_params)?;
    let ids = target_ids(session, command)?;
    let before = snapshot(session);
    for id in descendants(session, &ids) {
        if let Some(obj) = object_mut(session, id) {
            obj.locked = state.is_locked();
        }
    }
    let nodes: Vec<u64> = ids.iter().map(|id| id.0).collect();
    commit(session, NODE_LOCK_STATE, before, json!({ "nodes": nodes, "locked": state.is_locked() }))
}

fn unlock_all(session: &mut Session) -> CommandResult {
    if !session.document.objects.iter().any(|obj| obj.locked) {
        return Err(CommandError::invalid_params());
    }
    let before = snapshot(session);
    for obj in session.document.objects.iter_mut() {
        obj.locked = false;
    }
    commit(session, NODE_UNLOCK_ALL, before, json!({}))
}

fn node_update(session: &mut Session, command: &Command) -> CommandResult {
    let set = command.params.get("set").cloned().unwrap_or_default();
    let visible = match set.get("visible") {
        Some(value) => Some(value.as_bool().ok_or_else(CommandError::invalid_params)?),
        None => None,
    };
    let locked = match set.get("locked") {
        Some(value) => Some(LockState::parse(value).ok_or_else(CommandError::invalid_params)?),
        None => None,
    };
    let ids = target_ids(session, command)?;
    let before = snapshot(session);
    if let Some(flag) = visible {
        for id in &ids {
            if let Some(obj) = object_mut(session, *id) {
                obj.pickable = flag;
            }
        }
    }
    if let Some(state) = locked {
        for id in descendants(session, &ids) {
            if let Some(obj) = object_mut(session, id) {
                obj.locked = state.is_locked();
            }
        }
    }
    if visible == Some(false) {
        session.selection.ids.retain(|id| !ids.contains(id));
    }
    let nodes: Vec<u64> = ids.iter().map(|id| id.0).collect();
    commit(session, NODE_UPDATE, before, json!({ "nodes": nodes }))
}

fn show_all(session: &mut Session) -> CommandResult {
    if !session.document.objects.iter().any(|obj| !obj.pickable) {
        return Err(CommandError::invalid_params());
    }
    let before = snapshot(session);
    for obj in session.document.objects.iter_mut() {
        obj.pickable = true;
    }
    commit(session, NODE_SHOW_ALL, before, json!({}))
}

fn other_visibility_targets(session: &Session, command: &Command) -> Result<HashSet<ObjectId>, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let keep = target_ids(session, command)?;
    let doc = &session.document;
    let (_, in_scope) = tree_objects(doc, &layer_panel_tree_command(session, true).params)?;
    let scope: HashSet<_> = in_scope.iter().map(|object| object.id).collect();
    if keep.iter().any(|id| !scope.contains(id)) {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }

    let parents: HashMap<_, _> = doc.objects.iter().map(|object| (object.id, object.parent_id)).collect();
    let mut children: HashMap<ObjectId, Vec<&Object>> = HashMap::new();
    for object in &doc.objects {
        if let Some(parent) = object.parent_id {
            children.entry(parent).or_default().push(object);
        }
    }

    let mut protected = HashSet::new();
    let mut subtrees = keep.clone();
    for id in keep {
        let mut ancestor = parents.get(&id).copied().flatten();
        while let Some(parent) = ancestor.filter(|parent| protected.insert(*parent)) {
            if let Some(branch) = children.get(&parent) {
                subtrees.extend(branch.iter().filter(|child| child.clip_role == crate::object::ClipRole::Mask)
                    .map(|child| child.id));
            }
            ancestor = parents.get(&parent).copied().flatten();
        }
    }
    let mut visited = HashSet::new();
    while let Some(id) = subtrees.pop() {
        if visited.insert(id) {
            protected.insert(id);
            if let Some(branch) = children.get(&id) {
                subtrees.extend(branch.iter().map(|child| child.id));
            }
        }
    }

    let show = command.name == NODE_SHOW_OTHERS;
    Ok(in_scope.into_iter().filter(|object| {
        !protected.contains(&object.id) && object.pickable != show
            && (show || object.parent_id.is_none_or(|id| !scope.contains(&id) || protected.contains(&id)))
    }).map(|object| object.id).collect())
}

fn set_other_visibility(session: &mut Session, command: &Command) -> CommandResult {
    let ids = other_visibility_targets(session, command)?;
    if ids.is_empty() {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }
    let before = snapshot(session);
    let show = command.name == NODE_SHOW_OTHERS;
    let mut nodes = Vec::with_capacity(ids.len());
    for object in &mut session.document.objects {
        if ids.contains(&object.id) {
            object.pickable = show;
            nodes.push(object.id.0);
        }
    }
    commit(session, command.name.as_str(), before, json!({"nodes": nodes}))
}

pub fn locked_needs_confirm() -> CommandError {
    CommandError::new(LOCKED_NEEDS_CONFIRM)
}

pub fn any_locked(session: &Session, ids: &[ObjectId]) -> bool {
    descendants(session, ids)
        .iter()
        .any(|id| session.document.get_object(*id).map(|obj| obj.locked).unwrap_or(false))
}


pub(crate) fn delete_objects(document: &mut crate::Document, doomed: &std::collections::HashSet<ObjectId>) {
    let removed_images: std::collections::HashSet<_> = document.placed_images.iter()
        .filter(|image| doomed.contains(&image.object_id)).map(|image| image.resource).collect();
    document.detach_objects(doomed);
    document.merge_bindings.retain(|binding| match binding {
        crate::data_merge::MergeBinding::Qr { object_id, .. } | crate::data_merge::MergeBinding::Image { object_id, .. } => !doomed.contains(object_id),
        crate::data_merge::MergeBinding::Text { .. } => true,
    });
    document.swatch_bindings.retain(|binding| !doomed.contains(&binding.object));
    let stories: std::collections::HashSet<_> = document.objects.iter().flat_map(crate::clipboard::object_stories)
        .chain(document.frame_chains.iter().map(|chain| chain.story)).collect();
    crate::longdoc::marks::retain_story_content(document, &stories);
    if document.notes.endnote_frame.is_some_and(|id| doomed.contains(&ObjectId(id))) {
        document.notes.endnote_frame = None;
    }
    crate::longdoc::xref::refresh_xrefs(document);
    document.toc.tocs.retain(|toc| stories.contains(&crate::StoryId(toc.story)));
    document.index.indexes.retain(|&story| stories.contains(&crate::StoryId(story)));
    document.text_links.retain(|link| stories.contains(&link.story));
    let images: std::collections::HashSet<_> = document.placed_images.iter().map(|image| image.resource).collect();
    if !removed_images.is_empty() {
        let paint_images = document.paint_image_uses();
        document.resources.retain(|resource| {
            !removed_images.contains(&resource.id)
                || images.contains(&resource.id)
                || paint_images.contains_key(&resource.id)
        });
    }
}

fn delete(session: &mut Session, command: &Command, gate_locked: bool) -> CommandResult {
    let confirm = command.params.get("confirm_locked").and_then(Value::as_bool).unwrap_or(false);
    let ids = target_ids(session, command)?;
    if gate_locked && !confirm && any_locked(session, &ids) {
        return Err(locked_needs_confirm());
    }
    let before = snapshot(session);
    let doomed = descendants(session, &ids);
    delete_objects(&mut session.document, &doomed.iter().copied().collect());
    session.selection.ids.retain(|id| !doomed.contains(id));
    let nodes: Vec<u64> = ids.iter().map(|id| id.0).collect();
    let name = command.name.clone();
    commit(session, &name, before, json!({ "nodes": nodes, "id": nodes[0] }))
}


pub fn layer_panel_tree_command(session: &Session, include_hidden: bool) -> Command {
    Command::with_params(NODE_TREE_QUERY, layer_panel_tree_params(session, include_hidden))
}

pub fn layer_panel_objects(session: &Session) -> Result<(Option<ObjectId>, Vec<&Object>), CommandError> {
    tree_objects(&session.document, &layer_panel_tree_params(session, true))
}

fn layer_panel_tree_params(session: &Session, include_hidden: bool) -> Value {
    use crate::view::ViewTarget;
    let mut params = json!({"include_hidden": include_hidden});
    let doc = &session.document;
    if crate::commands::persona::layout_active() && doc.layout_artboard_origins.is_some() {
        let target = session.view_canvas_size.and_then(|size| {
            ViewTarget::at_view_center_with_hint(doc, &session.view, size, session.view_target_hint)
        });
        if let Some(ViewTarget::Artboard(id)) = target && doc.master_pages.roots.contains(&id) {
            params["master_id"] = json!(id.0);
        } else if let Some(id) = crate::commands::pages::viewed_page(session)
            .or_else(|| doc.pages.first().map(|page| page.id))
        {
            params["page_id"] = json!(id.0);
        }
    }
    params
}

pub(crate) fn tree_query(session: &Session, command: &Command) -> CommandResult {
    let include_hidden = command
        .params
        .get("include_hidden")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let (wrapper, objects) = tree_objects(&session.document, &command.params)?;
    let mut tree = Vec::new();
    for obj in objects {
        if !include_hidden && !obj.pickable {
            continue;
        }
        tree.push(json!({
            "id": obj.id.0,
            "object_id": obj.id.0,
            "name": obj.name,
            "parent_id": obj.parent_id.filter(|parent| Some(*parent) != wrapper).map(|id| id.0),
            "layer_folder": obj.is_layer_folder(),
            "flags": {
                "visible": obj.pickable,
                "locked": obj.locked
            }
        }));
    }
    Ok(json!({ "tree": tree, "nodes": tree }))
}

fn tree_objects<'a>(doc: &'a crate::Document, params: &Value) -> Result<(Option<ObjectId>, Vec<&'a Object>), CommandError> {
    let page = if params.get("page_id").is_some() {
        let id = crate::text_cmd::param_u64(params, "page_id")?;
        Some(doc.pages.iter().position(|page| page.id.0 == id).ok_or_else(CommandError::not_found)?)
    } else {
        None
    };
    let root = if params.get("master_id").is_some() {
        let id = ObjectId(crate::text_cmd::param_u64(params, "master_id")?);
        if page.is_some() || !doc.master_pages.roots.contains(&id) {
            return Err(CommandError::invalid_params());
        }
        Some(id)
    } else {
        None
    };
    let wrapper = root.or_else(|| page.and_then(|index| doc.linked_artboard(doc.pages[index].id).map(|board| board.id)));
    let master_items = root.map(|id| doc.subtree_ids(id));
    let mut objects = Vec::new();
    for obj in &doc.objects {
        if Some(obj.id) == wrapper {
            continue;
        }
        if let Some(items) = &master_items && !items.contains(&obj.id) {
            continue;
        }
        if let Some(index) = page
            && (crate::artboard::is_artboard(obj)
                || crate::commands::masters::owns_source(doc, obj.id)
                || crate::data_merge::layout::object_page_index(doc, obj.id) != index)
        {
            continue;
        }
        objects.push(obj);
    }
    Ok((wrapper, objects))
}

fn parse_parent(command: &Command) -> Result<Option<ObjectId>, CommandError> {
    for key in ["parent", "parent_id", "layer"] {
        if let Some(value) = command.params.get(key) {
            if value.is_null() {
                return Ok(None);
            }
            return parse_id(value).map(Some).ok_or_else(CommandError::invalid_params);
        }
    }
    Ok(None)
}

pub fn would_cycle(session: &Session, child: ObjectId, parent: ObjectId) -> bool {
    if child == parent {
        return true;
    }
    let mut current = Some(parent);
    let mut guard = 0;
    while let Some(id) = current {
        if id == child {
            return true;
        }
        current = session.document.get_object(id).and_then(|obj| obj.parent_id);
        guard += 1;
        if guard > session.document.objects.len() + 1 {
            return true;
        }
    }
    false
}

fn reparent(session: &mut Session, command: &Command) -> CommandResult {
    let ids = target_ids(session, command)?;
    let parent = parse_parent(command)?;
    let placement = match command.params.get("relative_to") {
        None => None,
        Some(value) => {
            let target = parse_id(value).ok_or_else(CommandError::invalid_params)?;
            let sibling = session.document.get_object(target).ok_or_else(CommandError::not_found)?;
            let above = match command.params["placement"].as_str() {
                Some("above") => true,
                Some("below") => false,
                _ => return Err(CommandError::invalid_params()),
            };
            if sibling.parent_id != parent || ids.iter().any(|id| would_cycle(session, *id, target)) {
                return Err(CommandError::invalid_params());
            }
            Some((target, above))
        }
    };
    let mut board_page = None;
    if let Some(parent_id) = parent {
        let doc = &session.document;
        let target = doc.get_object(parent_id).ok_or_else(CommandError::not_found)?;
        let board = crate::artboard::owner_or_self(doc, parent_id);
        let wrong_parent = ids.iter().any(|id| {
            doc.get_object(*id).is_some_and(crate::artboard::is_artboard)
                || board.is_some_and(|board| {
                    target.locked || board.locked || crate::data_merge::layout::object_page_index(doc, *id)
                        != crate::data_merge::layout::object_page_index(doc, board.id)
                })
        });
        if wrong_parent || ids.iter().any(|id| would_cycle(session, *id, parent_id)) {
            return Err(CommandError::invalid_params());
        }
        board_page = board.and_then(|board| board.page_id);
    }
    let before = snapshot(session);
    for id in &ids {
        if Some(*id) == parent {
            return Err(CommandError::invalid_params());
        }
        if let Some(obj) = object_mut(session, *id) {
            obj.parent_id = parent;
        }
        if let Some(page) = board_page {
            crate::artboard::edit::set_subtree_page(&mut session.document, *id, page);
        }
    }
    if let Some((target, above)) = placement {
        let moving: std::collections::HashSet<_> = ids.iter().flat_map(|id| session.document.subtree_ids(*id)).collect();
        let target_tree = session.document.subtree_ids(target);
        let objects = &mut session.document.objects;
        let (moved, mut rest): (Vec<Object>, Vec<Object>) = objects.drain(..).partition(|obj| moving.contains(&obj.id));
        // The object list is in paint order, the reverse of the Layers panel.
        let at = if above {
            rest.iter().rposition(|obj| target_tree.contains(&obj.id)).map_or(rest.len(), |index| index + 1)
        } else {
            rest.iter().position(|obj| target_tree.contains(&obj.id)).unwrap_or(rest.len())
        };
        rest.splice(at..at, moved);
        *objects = rest;
    } else if let Some(parent_id) = parent {
        let objects = &mut session.document.objects;
        let (moved, rest): (Vec<Object>, Vec<Object>) = objects.drain(..).partition(|obj| ids.contains(&obj.id));
        let mut next = rest;
        if let Some(index) = next.iter().position(|obj| obj.id == parent_id) {
            let at = (index + 1).min(next.len());
            for (offset, obj) in moved.into_iter().enumerate() {
                next.insert(at + offset, obj);
            }
        } else {
            let mut moved = moved;
            next.append(&mut moved);
        }
        *objects = next;
    }
    let nodes: Vec<u64> = ids.iter().map(|id| id.0).collect();
    commit(session, NODE_REPARENT, before, json!({ "nodes": nodes, "parent": parent.map(|id| id.0) }))
}

fn select_inside(session: &mut Session, command: &Command) -> CommandResult {
    let group_id = match listed_ids(command)? {
        Some(ids) if ids.len() == 1 => ids[0],
        Some(_) => return Err(CommandError::invalid_params()),
        None => {
            if session.selection.ids.len() != 1 {
                return Err(CommandError::invalid_params());
            }
            session.selection.ids[0]
        }
    };
    if !is_group(session, group_id) {
        return Err(CommandError::invalid_params());
    }
    let before = snapshot(session);
    let mut children = Vec::new();
    for obj in &session.document.objects {
        if obj.parent_id == Some(group_id) && obj.pickable && !obj.locked {
            children.push(obj.id);
        }
    }
    session.selection.ids = children.clone();
    let nodes: Vec<u64> = children.iter().map(|id| id.0).collect();
    commit(session, NODE_SELECT_INSIDE, before, json!({ "ids": nodes, "nodes": nodes }))
}

#[cfg(test)]
mod delete_contracts {
    use super::*;
    use crate::document::Document;
    use crate::frame::{frame_story, FrameChain};

    fn chained() -> (Session, Vec<ObjectId>) {
        let mut session = Session::new(Document::new([400.0, 200.0], crate::document::Units::Px, 72.0, 1));
        let ids: Vec<ObjectId> = (0..3)
            .map(|i| ObjectId(session.run(&Command::with_params("text.frame.add", json!({ "rect": [0, i * 40, 100, 20] }))).unwrap()["id"].as_u64().unwrap()))
            .collect();
        session.run(&Command::with_params("text.link", json!({ "from": ids[0].0, "to": ids[1].0 }))).unwrap();
        session.run(&Command::with_params("text.link", json!({ "from": ids[1].0, "to": ids[2].0 }))).unwrap();
        session.text_edit = None;
        (session, ids)
    }

    #[test]
    fn node_delete_prunes_chain_and_undo_restores_it() {
        let (mut session, ids) = chained();
        let story = frame_story(&session.document, ids[0]).unwrap();
        let objects = session.document.objects.clone();
        let generation = session.generation;
        session.run(&Command::with_params(NODE_DELETE, json!({ "nodes": [ids[1].0] }))).unwrap();
        assert_eq!(session.generation, generation + 1);
        assert!(session.document.get_object(ids[1]).is_none());
        assert_eq!(session.document.frame_chains, vec![FrameChain { story, frames: vec![ids[0], ids[2]] }]);
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(session.document.frame_chains, vec![FrameChain { story, frames: ids.clone() }]);
        assert_eq!(serde_json::to_value(&session.document.objects).unwrap(), serde_json::to_value(&objects).unwrap());
        session.run(&Command::new("edit.redo")).unwrap();
        assert_eq!(session.document.frame_chains, vec![FrameChain { story, frames: vec![ids[0], ids[2]] }]);
    }

    #[test]
    fn object_delete_undo_restores_same_id() {
        let (mut session, ids) = chained();
        session.run(&Command::with_params(OBJECT_DELETE, json!({ "id": ids[2].0 }))).unwrap();
        assert!(session.document.get_object(ids[2]).is_none());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.get_object(ids[2]).is_some());
        assert_eq!(session.document.frame_chains[0].frames, ids);
    }

    #[test]
    fn edit_delete_uses_selection_and_gates_locked() {
        let (mut session, ids) = chained();
        assert!(!session.can_execute_named(EDIT_DELETE));
        session.selection.ids = vec![ids[0]];
        assert!(session.can_execute_named(EDIT_DELETE));
        session.document.objects.iter_mut().find(|obj| obj.id == ids[0]).unwrap().locked = true;
        let error = session.run(&Command::new(EDIT_DELETE)).unwrap_err();
        assert_eq!(error.code, LOCKED_NEEDS_CONFIRM);
        session.run(&Command::with_params(EDIT_DELETE, json!({ "confirm_locked": true }))).unwrap();
        assert!(session.document.get_object(ids[0]).is_none());
        assert!(session.selection.ids.is_empty());
    }

    #[test]
    fn text_frame_delete_prunes_story_and_undo_restores_it() {
        let mut session = Session::new(Document::new([400.0, 200.0], crate::document::Units::Px, 72.0, 1));
        let id = ObjectId(session.run(&Command::with_params("text.frame.add", json!({ "rect": [0, 0, 100, 20] }))).unwrap()["id"].as_u64().unwrap());
        session.text_edit = None;
        let story = frame_story(&session.document, id).unwrap();
        assert!(session.document.stories.iter().any(|item| item.id == story));
        let high = session.document.next_story_id;
        let generation = session.generation;
        session.run(&Command::with_params(NODE_DELETE, json!({ "nodes": [id.0] }))).unwrap();
        assert!(session.document.get_object(id).is_none());
        assert!(!session.document.stories.iter().any(|item| item.id == story));
        assert!(session.document.next_story_id >= high);
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.get_object(id).is_some());
        assert!(session.document.stories.iter().any(|item| item.id == story));
        assert!(session.document.next_story_id >= high);
        assert!(session.generation > generation);
        session.run(&Command::new("edit.redo")).unwrap();
        assert!(session.document.get_object(id).is_none());
        assert!(!session.document.stories.iter().any(|item| item.id == story));
        assert!(session.document.next_story_id >= high);
    }
}
