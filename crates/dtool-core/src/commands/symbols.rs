pub(crate) mod curves;
pub(crate) mod clipboard;
pub(crate) mod structure;
pub mod constraints;
pub(crate) mod page_import;

use crate::blend::BlendMode;
use crate::clipboard::{ClipboardBundle, collect_bundle, paste_bundle_mapped};
use crate::colour::swatch::SwatchTarget;
use crate::document::{PageId, Story, StoryId};
use crate::paint::{Paint, StrokeStyle, paint_from_param};
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, ObjectKind, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Symbols {
    pub definitions: Vec<Definition>,
    pub instances: Vec<Instance>,
    #[serde(default, skip_serializing_if = "reserved_id_is_zero")]
    last_removed_id: u64,
}

fn reserved_id_is_zero(id: &u64) -> bool {
    *id == 0
}

impl Symbols {
    pub(crate) fn stories(&self) -> impl Iterator<Item = &Story> {
        self.definitions.iter().flat_map(|definition| &definition.bundle.stories)
    }

    pub(crate) fn stories_mut(&mut self) -> impl Iterator<Item = (u64, &mut Story)> {
        self.definitions.iter_mut().flat_map(|definition| {
            let id = definition.id;
            definition.bundle.stories.iter_mut().map(move |story| (id, story))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty() && self.instances.is_empty() && self.last_removed_id == 0
    }

    pub(crate) fn convert_units(&mut self, factor: f32) -> Result<(), CommandError> {
        for definition in &mut self.definitions {
            crate::doc_setup::transform_object_dimensions(
                &mut definition.bundle.objects, factor, [factor; 2],
                |pos, size, _| (pos * factor, size * factor),
            )?;
            definition.origin = definition.origin.map(|value| value * factor);
            definition.size = definition.size.map(|value| value * factor);
        }
        for instance in &mut self.instances {
            for entry in &mut instance.curve_overrides {
                // Reference and edited frames store a relative change. Canvas
                // placement is applied later when that change is projected.
                entry.relocate(factor, [0.0; 2], [0.0; 2]);
            }
        }
        Ok(())
    }

    fn next_definition_id(&self) -> Result<u64, CommandError> {
        self.definitions.iter().map(|definition| definition.id)
            .fold(self.last_removed_id, u64::max)
            .checked_add(1).ok_or_else(CommandError::invalid_params)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub id: u64,
    pub name: String,
    origin: [f32; 2],
    size: [f32; 2],
    bundle: ClipboardBundle,
    /// Canonical text layers whose guide ID refers to an object outside the symbol.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    external_path_nodes: Vec<ObjectId>,
}

impl Definition {
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// Reconstruct the shared artwork without any instance-specific edits.
    pub fn preview_document(&self, units: crate::document::Units, dpi: f32) -> (Document, Vec<ObjectId>) {
        let mut doc = Document::new(self.size.map(|side| side.max(1.0)), units, dpi, 1);
        let roots = crate::clipboard::paste_bundle(&mut doc, &self.bundle, None, [0.0, 0.0]);
        (doc, roots)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub root: ObjectId,
    pub definition: u64,
    // Canonical object IDs are stable even after the source instance is deleted.
    links: Vec<(ObjectId, ObjectId)>,
    fill_overrides: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stroke_overrides: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    corner_overrides: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    constraint_overrides: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    text_overrides: Vec<StoryId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    curve_overrides: Vec<curves::CurveOverride>,
    #[serde(default)]
    structure_override: bool,
}

const COMMANDS: &[&str] = &[
    "symbol.create", "symbol.place", "symbol.query", "symbol.edit",
    "symbol.override", "symbol.revert", "symbol.detach", "symbol.instance", "symbol.sync",
    "symbol.rename", "symbol.delete",
];

fn id(value: &Value) -> Result<ObjectId, CommandError> {
    value.as_u64().or_else(|| {
        let text = value.as_str()?;
        text.strip_prefix("node:").unwrap_or(text).parse().ok()
    }).map(ObjectId).ok_or_else(CommandError::invalid_params)
}

fn is_descendant(doc: &Document, node: ObjectId, root: ObjectId) -> bool {
    let mut parent = doc.get_object(node).and_then(|object| object.parent_id);
    for _ in 0..doc.objects().len() {
        let Some(current) = parent else { break };
        if current == root {
            return true;
        }
        parent = doc.get_object(current).and_then(|object| object.parent_id);
    }
    false
}

fn live_instance(doc: &Document, instance: &Instance) -> bool {
    doc.get_object(instance.root).is_some_and(|object| matches!(object.kind, ObjectKind::Group))
}

#[derive(Clone, Copy)]
pub struct LayerLink {
    pub is_root: bool,
    pub modified: bool,
}

/// Index the live links for layer rows, including the state of a collapsed root.
pub fn layer_links(doc: &Document) -> HashMap<ObjectId, LayerLink> {
    let mut links = HashMap::new();
    if doc.symbols.instances.is_empty() {
        return links;
    }
    let objects: HashMap<_, _> = doc.objects.iter().map(|object| (object.id, object)).collect();
    for instance in &doc.symbols.instances {
        let Some(definition) = doc.symbols.definitions.iter().find(|entry| entry.id == instance.definition) else { continue };
        if !objects.get(&instance.root).is_some_and(|root| root.kind == ObjectKind::Group) {
            continue;
        }
        let mut local: HashSet<_> = instance.fill_overrides.iter()
            .chain(&instance.stroke_overrides).chain(&instance.corner_overrides)
            .chain(&instance.constraint_overrides).copied().collect();
        local.extend(instance.curve_overrides.iter().map(|entry| entry.node));
        if !instance.text_overrides.is_empty() {
            local.extend(definition.bundle.objects.iter().filter(|object| {
                crate::clipboard::object_stories(object).iter().any(|id| instance.text_overrides.contains(id))
            }).map(|object| object.id));
        }
        let mut modified_root = instance.structure_override;
        for &(canonical, actual) in &instance.links {
            let mut parent = objects.get(&actual).and_then(|object| object.parent_id);
            for _ in 0..objects.len() {
                let Some(id) = parent else { break };
                if id == instance.root {
                    let modified = local.contains(&canonical);
                    modified_root |= modified;
                    links.entry(actual).or_insert(LayerLink { is_root: false, modified });
                    break;
                }
                parent = objects.get(&id).and_then(|object| object.parent_id);
            }
        }
        links.insert(instance.root, LayerLink { is_root: true, modified: modified_root });
    }
    links
}

fn linked_node(doc: &Document, instance: &Instance, canonical: ObjectId) -> Option<ObjectId> {
    let (_, actual) = instance.links.iter().find(|(key, _)| *key == canonical)?;
    is_descendant(doc, *actual, instance.root).then_some(*actual)
}

pub fn selection_link(session: &Session) -> Option<(ObjectId, u64, Option<ObjectId>)> {
    let [selected] = session.selection.ids.as_slice() else { return None };
    let doc = &session.document;
    for instance in &doc.symbols.instances {
        if !live_instance(doc, instance) {
            continue;
        }
        if *selected == instance.root {
            return Some((instance.root, instance.definition, None));
        }
        if is_descendant(doc, *selected, instance.root)
            && let Some((canonical, _)) = instance.links.iter().find(|(_, actual)| *actual == *selected)
        {
            return Some((instance.root, instance.definition, Some(*canonical)));
        }
    }
    None
}

fn source_ids(session: &Session, params: &Value) -> Result<Vec<ObjectId>, CommandError> {
    let mut ids = match params.get("nodes") {
        Some(value) => value.as_array().ok_or_else(CommandError::invalid_params)?
            .iter().map(id).collect::<Result<Vec<_>, _>>()?,
        None => session.selection.ids.clone(),
    };
    ids.sort_unstable_by_key(|id| id.0);
    ids.dedup();
    let doc = &session.document;
    let first = ids.first().and_then(|id| doc.get_object(*id)).ok_or_else(CommandError::invalid_params)?;
    for &id in &ids {
        let object = doc.get_object(id).ok_or_else(CommandError::invalid_params)?;
        if object.locked || matches!(object.kind, ObjectKind::Artboard)
            || object.parent_id != first.parent_id || object.page_id != first.page_id
            || doc.symbols.instances.iter().filter(|instance| live_instance(doc, instance))
                .any(|instance| instance.root == id
                    || is_descendant(doc, instance.root, id)
                    || is_descendant(doc, id, instance.root))
        {
            return Err(CommandError::invalid_params());
        }
    }
    Ok(ids)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let name = command.name.as_str();
    COMMANDS.contains(&name).then(|| session.has_document && match name {
        "symbol.create" => source_ids(session, &command.params).is_ok(),
        "symbol.place" | "symbol.instance" => !session.document.symbols.definitions.is_empty(),
        "symbol.detach" => command.params.get("instance").is_some() || selection_link(session).is_some(),
        "symbol.rename" | "symbol.delete" => definition_index(&session.document, &command.params).is_ok(),
        _ => true,
    })
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    COMMANDS.contains(&name).then(|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        match name {
            "symbol.create" => create(session, command),
            "symbol.place" | "symbol.instance" => place(session, command),
            "symbol.query" => {
                let mut result = query(&session.document);
                result["sync"] = json!(session.symbol_sync);
                Ok(result)
            }
            "symbol.sync" => {
                session.symbol_sync = command.params["enabled"].as_bool().ok_or_else(CommandError::invalid_params)?;
                Ok(json!({"enabled": session.symbol_sync}))
            }
            "symbol.detach" => detach(session, command),
            "symbol.rename" | "symbol.delete" => manage_definition(session, command),
            _ => edit_style(session, command),
        }
    })
}

fn query(doc: &Document) -> Value {
    let definitions: Vec<_> = doc.symbols.definitions.iter().map(|definition| json!({
        "id": definition.id,
        "name": definition.name,
        "nodes": definition.bundle.objects.iter().map(|object| object.id).collect::<Vec<_>>(),
    })).collect();
    let instances: Vec<_> = doc.symbols.instances.iter().filter(|instance| live_instance(doc, instance))
        .map(|instance| json!({
            "root": instance.root,
            "definition": instance.definition,
            "links": instance.links.iter().filter(|(canonical, _)| linked_node(doc, instance, *canonical).is_some()).collect::<Vec<_>>(),
            "fill_overrides": instance.fill_overrides,
            "stroke_overrides": instance.stroke_overrides,
            "corner_overrides": instance.corner_overrides,
            "constraint_overrides": instance.constraint_overrides,
            "text_overrides": instance.text_overrides,
            "curve_overrides": instance.curve_overrides.iter().map(|entry| entry.node).collect::<Vec<_>>(),
            "structure_override": instance.structure_override,
        })).collect();
    json!({"definitions": definitions, "instances": instances})
}

fn make_group(name: String, pos: [f32; 2], size: [f32; 2], parent_id: Option<ObjectId>, page_id: Option<PageId>) -> Object {
    Object {
        name, pos, size, parent_id, page_id,
        kind: ObjectKind::Group,
        blend_mode: BlendMode::PassThrough,
        ..Object::default()
    }
}

fn create(session: &mut Session, command: &Command) -> CommandResult {
    let params = &command.params;
    let ids = source_ids(session, params)?;
    let mut doc = session.document.clone();
    let definition_id = doc.symbols.next_definition_id()?;
    let bundle = collect_bundle(&doc, &ids);
    if bundle.roots.len() != ids.len() || bundle.roots.iter().any(|id| !ids.contains(id)) {
        return Err(CommandError::invalid_params());
    }
    let bounds = ids.iter().filter_map(|id| doc.get_object(*id)).map(|object| {
        let [x, y, width, height] = crate::geometry::object_world_bounds(object);
        [x, y, x + width, y + height]
    })
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
        .ok_or_else(CommandError::invalid_params)?;
    let origin = [bounds[0], bounds[1]];
    let size = [bounds[2] - bounds[0], bounds[3] - bounds[1]];
    let name = params["name"].as_str().map(str::trim).filter(|name| !name.is_empty())
        .map(str::to_owned).unwrap_or_else(|| format!("{} {definition_id}", dtool_strings::text("panel.symbols")));
    let mut selection = session.selection.ids.clone();
    let mut root = None;
    if params["replace_source"].as_bool().unwrap_or(true) {
        let first = doc.get_object(ids[0]).ok_or_else(CommandError::invalid_params)?;
        let group = make_group(name.clone(), origin, size, first.parent_id, first.page_id);
        let top = doc.objects().iter().rposition(|object| ids.contains(&object.id)).ok_or_else(CommandError::invalid_params)?;
        let group_id = doc.create_object(group);
        if let Some(created) = doc.objects.pop() {
            doc.objects.insert(top + 1, created);
        }
        for &id in &ids {
            if let Some(object) = doc.find_object_mut(id.0) {
                object.parent_id = Some(group_id);
            }
        }
        let links = bundle.objects.iter().map(|object| (object.id, object.id)).collect();
        doc.symbols.instances.push(Instance { root: group_id, definition: definition_id, links, fill_overrides: Vec::new(), stroke_overrides: Vec::new(), corner_overrides: Vec::new(), constraint_overrides: Vec::new(), text_overrides: Vec::new(), curve_overrides: Vec::new(), structure_override: false });
        selection = vec![group_id];
        root = Some(group_id);
    }
    let members: std::collections::HashSet<_> = bundle.objects.iter().map(|object| object.id).collect();
    let external_path_nodes = bundle.objects.iter().filter_map(|object| {
        let path = object.path_text_data()?;
        (!members.contains(&path.path)).then_some(object.id)
    }).collect();
    doc.symbols.definitions.push(Definition { id: definition_id, name, origin, size, bundle, external_path_nodes });
    super::document_change::commit(session, &command.name, doc, selection, json!({"definition": definition_id, "instance": root}))
}

fn canvas_destination(session: &Session, center: [f32; 2]) -> (Option<ObjectId>, Option<PageId>) {
    let doc = &session.document;
    let artboard = crate::view::ViewTarget::artboards(doc).filter(|object| !object.locked)
        .filter(|object| {
            crate::geometry::doc_to_object_local(object, center).is_some_and(|[x, y]| {
                let [left, top] = object.pos;
                left <= x && x <= left + object.size[0].abs() && top <= y && y <= top + object.size[1].abs()
            })
        }).last();
    if let Some(artboard) = artboard {
        return (Some(artboard.id), artboard.page_id);
    }
    let page = doc.pages.iter().enumerate().find_map(|(index, page)| {
        let [x, y] = crate::data_merge::layout::page_origin(doc, index);
        let [width, height] = page.size;
        (x <= center[0] && center[0] <= x + width && y <= center[1] && center[1] <= y + height).then_some(page.id)
    });
    (None, page)
}

fn place(session: &mut Session, command: &Command) -> CommandResult {
    let params = &command.params;
    let mut doc = session.document.clone();
    let definition = params["definition"].as_u64().or_else(|| params["symbol"].as_u64())
        .and_then(|id| doc.symbols.definitions.iter().find(|definition| definition.id == id))
        .cloned().ok_or_else(CommandError::invalid_params)?;
    let center: Option<[f32; 2]> = params.get("center")
        .map(|value| serde_json::from_value(value.clone())).transpose().map_err(|_| CommandError::invalid_params())?;
    if center.is_some() && (params.get("pos").is_some() || params.get("page").is_some()) {
        return Err(CommandError::invalid_params());
    }
    let pos: [f32; 2] = if let Some([x, y]) = center {
        [x - definition.size[0] * 0.5, y - definition.size[1] * 0.5]
    } else if let Some(value) = params.get("pos") {
        serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?
    } else {
        let offset = 24.0 * doc.symbols.instances.iter().filter(|instance| instance.definition == definition.id && live_instance(&doc, instance)).count().max(1) as f32;
        definition.origin.map(|value| value + offset)
    };
    if pos.iter().any(|coordinate| !coordinate.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    let (parent, page) = if let Some(center) = center {
        canvas_destination(session, center)
    } else {
        let page = if let Some(value) = params.get("page") {
            let page = PageId(id(value)?.0);
            if !doc.pages.iter().any(|entry| entry.id == page) {
                return Err(CommandError::invalid_params());
            }
            Some(page)
        } else {
            session.selection.ids.first().and_then(|id| doc.get_object(*id)).and_then(|object| object.page_id)
                .or_else(|| definition.bundle.objects.first().and_then(|object| object.page_id)
                    .filter(|page| doc.pages.iter().any(|entry| entry.id == *page)))
        };
        (None, page)
    };
    let root = doc.create_object(make_group(definition.name.clone(), pos, definition.size, parent, page));
    let offset = [pos[0] - definition.origin[0], pos[1] - definition.origin[1]];
    let map = paste_bundle_mapped(&mut doc, &definition.bundle, Some(root), offset);
    structure::restore_external_guides(&mut doc, &definition, &map);
    let links = definition.bundle.objects.iter().map(|object| (object.id, map[&object.id])).collect::<Vec<_>>();
    for &(_, actual) in &links {
        if let Some(object) = doc.find_object_mut(actual.0) {
            object.page_id = page;
        }
    }
    let stories = links.iter().filter_map(|(_, id)| doc.get_object(*id)?.story_id()).collect::<std::collections::HashSet<_>>();
    for story in stories {
        crate::text_cmd::refresh_art_bounds(&mut doc, story)?;
    }
    doc.symbols.instances.push(Instance { root, definition: definition.id, links, fill_overrides: Vec::new(), stroke_overrides: Vec::new(), corner_overrides: Vec::new(), constraint_overrides: Vec::new(), text_overrides: Vec::new(), curve_overrides: Vec::new(), structure_override: false });
    super::document_change::commit(session, &command.name, doc, vec![root], json!({"definition": definition.id, "instance": root}))
}

fn instance_index(session: &Session, params: &Value) -> Result<usize, CommandError> {
    let doc = &session.document;
    let root = if let Some(value) = params.get("instance") {
        id(value)?
    } else {
        selection_link(session).map(|(root, _, _)| root).ok_or_else(CommandError::invalid_params)?
    };
    doc.symbols.instances.iter().position(|instance| instance.root == root && live_instance(doc, instance))
        .ok_or_else(CommandError::invalid_params)
}

fn detach(session: &mut Session, command: &Command) -> CommandResult {
    let index = instance_index(session, &command.params)?;
    let mut doc = session.document.clone();
    let instance = doc.symbols.instances.remove(index);
    super::document_change::commit(session, &command.name, doc, session.selection.ids.clone(), json!({"instance": instance.root}))
}

fn definition_index(doc: &Document, params: &Value) -> Result<usize, CommandError> {
    let definition = params["definition"].as_u64().ok_or_else(CommandError::invalid_params)?;
    doc.symbols.definitions.iter().position(|entry| entry.id == definition)
        .ok_or_else(CommandError::not_found)
}

fn manage_definition(session: &mut Session, command: &Command) -> CommandResult {
    let index = definition_index(&session.document, &command.params)?;
    let mut doc = session.document.clone();
    let definition = &doc.symbols.definitions[index];
    let definition_id = definition.id;
    let result = if command.name == "symbol.rename" {
        let name = command.params["name"].as_str().map(str::trim)
            .filter(|name| !name.is_empty()).ok_or_else(CommandError::invalid_params)?;
        if definition.name == name {
            return Ok(json!({"definition": definition_id, "name": name, "changes": []}));
        }
        let old_name = std::mem::replace(&mut doc.symbols.definitions[index].name, name.to_owned());
        let roots: Vec<_> = doc.symbols.instances.iter()
            .filter(|instance| instance.definition == definition_id && live_instance(&doc, instance))
            .map(|instance| instance.root).collect();
        for root in roots {
            if let Some(object) = doc.find_object_mut(root.0) && object.name == old_name {
                object.name = name.to_owned();
            }
        }
        json!({"definition": definition_id, "name": name})
    } else {
        let detached: Vec<_> = doc.symbols.instances.iter()
            .filter(|instance| instance.definition == definition_id && live_instance(&doc, instance))
            .map(|instance| instance.root).collect();
        doc.symbols.instances.retain(|instance| instance.definition != definition_id);
        doc.symbols.definitions.remove(index);
        doc.symbols.last_removed_id = doc.symbols.last_removed_id.max(definition_id);
        json!({"definition": definition_id, "detached": detached})
    };
    super::document_change::commit(session, &command.name, doc, session.selection.ids.clone(), result)
}

pub(crate) fn fill_link(doc: &Document, actual: ObjectId, paint: &Paint) -> Option<(ObjectId, u64, ObjectId)> {
    if matches!(paint, Paint::Bitmap(_)) {
        return None;
    }
    for instance in &doc.symbols.instances {
        if !live_instance(doc, instance) || !is_descendant(doc, actual, instance.root) {
            continue;
        }
        let Some(&(canonical, _)) = instance.links.iter().find(|(_, id)| *id == actual) else { continue };
        let original = doc.symbols.definitions.iter().find(|definition| definition.id == instance.definition)?
            .bundle.objects.iter().find(|object| object.id == canonical)?;
        if !matches!(original.fill, Paint::Bitmap(_)) {
            return Some((instance.root, instance.definition, canonical));
        }
    }
    None
}

pub fn has_corner_override(session: &Session, root: ObjectId, node: ObjectId) -> bool {
    session.document.symbols.instances.iter().any(|entry| entry.root == root && entry.corner_overrides.contains(&node))
}

pub fn has_structure_override(session: &Session, root: ObjectId) -> bool {
    session.document.symbols.instances.iter().any(|entry| entry.root == root && entry.structure_override)
}

fn restore_structure(session: &mut Session, command: &Command) -> CommandResult {
    let index = instance_index(session, &command.params)?;
    let root = session.document.symbols.instances[index].root;
    let mut doc = session.document.clone();
    if !structure::restore(&mut doc, root)? {
        return Ok(json!({"doc_version": session.generation, "changes": []}));
    }
    super::document_change::commit(session, &command.name, doc, vec![root], json!({"instance": root, "property": "structure"}))
}

fn edit_style(session: &mut Session, command: &Command) -> CommandResult {
    let params = &command.params;
    let shared = command.name == "symbol.edit";
    let revert = command.name == "symbol.revert";
    if revert && params["property"].as_str() == Some("structure") {
        return restore_structure(session, command);
    }
    let node = id(&params["node"])?;
    let doc = &session.document;
    let instance = if shared {
        None
    } else {
        Some(&doc.symbols.instances[instance_index(session, params)?])
    };
    let definition = instance.map(|entry| entry.definition).or_else(|| params["definition"].as_u64())
        .ok_or_else(CommandError::invalid_params)?;
    let original = original_object(doc, definition, node).ok_or_else(CommandError::invalid_params)?;
    let root = instance.map(|entry| entry.root);
    let mut updated = doc.clone();
    let kept_override = if revert {
        match params.get("property").and_then(Value::as_str).unwrap_or("fill") {
            "stroke" => set_stroke(&mut updated, definition, root, node, original.stroke.clone(), false, true)?,
            "constraint" => {
                let index = instance_index(session, params)?;
                if !constraints::revert(&mut updated, definition, index, node)? {
                    return Ok(json!({"doc_version": session.generation, "changes": []}));
                }
                false
            }
            "corner" => {
                let index = instance_index(session, params)?;
                if !doc.symbols.instances[index].corner_overrides.contains(&node) {
                    return Ok(json!({"doc_version": session.generation, "changes": []}));
                }
                let actual = linked_node(doc, &doc.symbols.instances[index], node).ok_or_else(CommandError::not_found)?;
                let corner = super::shape::rounded_corner(original).cloned().ok_or_else(CommandError::invalid_params)?;
                let target = updated.find_object_mut(actual.0).ok_or_else(CommandError::not_found)?;
                apply_corner(target, &super::shape::CornerEdit::Replace(corner))?;
                updated.symbols.instances[index].corner_overrides.retain(|id| *id != node);
                false
            }
            "curve" => {
                if !curves::revert(&mut updated, definition, root.ok_or_else(CommandError::invalid_params)?, node)? {
                    return Ok(json!({"doc_version": session.generation, "changes": []}));
                }
                false
            }
            "text" => {
                revert_text(&mut updated, definition, root.ok_or_else(CommandError::invalid_params)?, node)?;
                false
            }
            "fill" => update_fill(&mut updated, definition, root, node, original.fill.clone(), false, true)?,
            _ => return Err(CommandError::invalid_params()),
        }
    } else {
        let set = params["set"].as_object().filter(|set| set.len() == 1).ok_or_else(CommandError::invalid_params)?;
        if let Some(fill) = set.get("fill") {
            update_fill(&mut updated, definition, root, node, paint_from_param(fill)?, shared, false)?
        } else if let Some(stroke) = set.get("stroke").filter(|stroke| stroke.is_object()) {
            let baseline = instance.and_then(|entry| linked_node(doc, entry, node))
                .and_then(|id| doc.get_object(id)).unwrap_or(original);
            let patch = super::stroke::StrokePatch::from_params(stroke)?;
            set_stroke(&mut updated, definition, root, node, patch.applied(&baseline.stroke)?, shared, false)?
        } else if let Some(rule) = set.get("constraint") {
            let value = serde_json::from_value(rule.clone()).map_err(|_| CommandError::invalid_params())?;
            let edit = crate::constraints::Edit::Replace(value);
            let (changed, kept) = if let Some(instance) = instance {
                let actual = linked_node(doc, instance, node).ok_or_else(CommandError::not_found)?;
                if crate::constraints::target_parent(doc, actual).is_none() {
                    return Err(CommandError::invalid_params());
                }
                constraints::update(&mut updated, &[actual], &edit, false)?
            } else {
                constraints::shared(&mut updated, definition, node, &edit)?
            };
            if !changed {
                return Ok(json!({"doc_version": session.generation, "changes": []}));
            }
            kept
        } else {
            return Err(CommandError::invalid_params());
        }
    };
    let warnings = warnings(kept_override);
    super::document_change::commit(session, &command.name, updated, session.selection.ids.clone(),
        json!({"definition": definition, "node": node, "warnings": warnings}))
}

pub(crate) fn update_fill(
    doc: &mut Document, definition: u64, root: Option<ObjectId>, node: ObjectId,
    paint: Paint, shared: bool, revert: bool,
) -> Result<bool, CommandError> {
    let definition_index = doc.symbols.definitions.iter().position(|entry| entry.id == definition)
        .ok_or_else(CommandError::invalid_params)?;
    let bundle = &mut doc.symbols.definitions[definition_index].bundle;
    let original = bundle.objects.iter_mut().find(|object| object.id == node)
        .ok_or_else(CommandError::invalid_params)?;
    if matches!(paint, Paint::Bitmap(_)) || matches!(original.fill, Paint::Bitmap(_)) {
        return Err(CommandError::invalid_params());
    }
    let mut kept_override = false;
    let mut targets = Vec::new();
    if shared {
        original.fill = paint.clone();
        bundle.swatch_bindings.retain(|binding| binding.object != node || binding.target != SwatchTarget::Fill);
        for instance in doc.symbols.instances.iter().filter(|entry| entry.definition == definition && live_instance(doc, entry)) {
            if let Some(actual) = linked_node(doc, instance, node) {
                if instance.fill_overrides.contains(&node) {
                    kept_override = true;
                } else {
                    targets.push(actual);
                }
            }
        }
    } else {
        let index = doc.symbols.instances.iter().position(|entry| Some(entry.root) == root && live_instance(doc, entry))
            .ok_or_else(CommandError::invalid_params)?;
        let target = linked_node(doc, &doc.symbols.instances[index], node).ok_or_else(CommandError::invalid_params)?;
        let overrides = &mut doc.symbols.instances[index].fill_overrides;
        overrides.retain(|id| *id != node);
        if !revert {
            overrides.push(node);
        }
        targets.push(target);
    }
    for target in targets {
        if let Some(object) = doc.find_object_mut(target.0) {
            object.fill = paint.clone();
        }
        doc.swatch_bindings.retain(|binding| binding.object != target || binding.target != SwatchTarget::Fill);
    }
    Ok(kept_override)
}

fn original_object(doc: &Document, definition: u64, node: ObjectId) -> Option<&Object> {
    doc.symbols.definitions.iter().find(|entry| entry.id == definition)?
        .bundle.objects.iter().find(|object| object.id == node)
}

pub(crate) fn object_link(doc: &Document, actual: ObjectId) -> Option<(ObjectId, u64, ObjectId)> {
    for instance in &doc.symbols.instances {
        if !live_instance(doc, instance) {
            continue;
        }
        let Some(&(node, _)) = instance.links.iter().find(|(_, id)| *id == actual) else { continue };
        if is_descendant(doc, actual, instance.root) && original_object(doc, instance.definition, node).is_some() {
            return Some((instance.root, instance.definition, node));
        }
    }
    None
}

fn apply_corner(object: &mut Object, edit: &super::shape::CornerEdit) -> Result<bool, CommandError> {
    let corner = edit.for_object(object).ok_or_else(CommandError::invalid_params)?;
    let shape = object.shape.as_mut().ok_or_else(CommandError::invalid_params)?;
    if shape.corner.as_ref() == Some(&corner) {
        return Ok(false);
    }
    shape.corner = Some(corner);
    Ok(true)
}

pub(super) fn update_corners(
    doc: &mut Document, ids: &[ObjectId], edit: &super::shape::CornerEdit, shared: bool,
) -> Result<(bool, bool), CommandError> {
    let mut changed = false;
    let mut kept_override = false;
    let mut addressed = std::collections::HashSet::new();
    for id in ids {
        let link = object_link(doc, *id).filter(|&(_, definition, node)| {
            original_object(doc, definition, node).and_then(super::shape::rounded_corner).is_some()
        });
        let Some((root, definition, node)) = link else {
            let target = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
            changed |= apply_corner(target, edit)?;
            continue;
        };
        let index = doc.symbols.instances.iter().position(|entry| entry.root == root).ok_or_else(CommandError::not_found)?;
        if !shared || doc.symbols.instances[index].corner_overrides.contains(&node) {
            let target = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
            if apply_corner(target, edit)? {
                let overrides = &mut doc.symbols.instances[index].corner_overrides;
                if !overrides.contains(&node) {
                    overrides.push(node);
                }
                changed = true;
                kept_override |= shared;
            }
            continue;
        }
        if !addressed.insert((definition, node)) {
            continue;
        }
        let targets: Vec<_> = doc.symbols.instances.iter()
            .filter(|entry| entry.definition == definition && live_instance(doc, entry))
            .filter_map(|entry| {
                let actual = linked_node(doc, entry, node)?;
                doc.get_object(actual).and_then(super::shape::rounded_corner)?;
                if entry.corner_overrides.contains(&node) {
                    kept_override = true;
                    None
                } else {
                    Some(actual)
                }
            }).collect();
        let original = doc.symbols.definitions.iter_mut().find(|entry| entry.id == definition)
            .and_then(|entry| entry.bundle.objects.iter_mut().find(|object| object.id == node))
            .ok_or_else(CommandError::not_found)?;
        changed |= apply_corner(original, edit)?;
        for actual in targets {
            let target = doc.find_object_mut(actual.0).ok_or_else(CommandError::not_found)?;
            changed |= apply_corner(target, edit)?;
        }
    }
    Ok((changed, kept_override))
}

pub(crate) fn object_update_is_linked(doc: &Document, args: &crate::object::ObjectUpdateArgs) -> bool {
    let id = ObjectId(args.id);
    args.fill.as_ref().is_some_and(|paint| fill_link(doc, id, paint).is_some())
        || args.stroke.is_some() && object_link(doc, id).is_some()
}

pub(crate) fn apply_object_update(
    document: &mut Document, mut args: crate::object::ObjectUpdateArgs, shared: bool,
) -> Result<bool, CommandError> {
    let id = ObjectId(args.id);
    crate::command::check_object_style_edit(document, &args)?;
    let fill = args.fill.as_ref().and_then(|paint| fill_link(document, id, paint))
        .and_then(|link| args.fill.take().map(|paint| (link, paint)));
    let stroke = if let Some(next) = args.stroke.take_if(|_| object_link(document, id).is_some()) {
        let before = document.get_object(id).ok_or_else(CommandError::not_found)?.stroke.clone();
        Some((id, before, next))
    } else {
        None
    };
    crate::command::execute_object_update(document, args)?;
    let mut kept_override = false;
    if let Some(((instance, definition, node), paint)) = fill {
        kept_override |= update_fill(document, definition, Some(instance), node, paint, shared, false)?;
    }
    if let Some(stroke) = stroke {
        kept_override |= update_strokes(document, &[stroke], shared, StrokeEdit::ChangedFields)?;
    }
    Ok(kept_override)
}

pub(crate) enum StrokeEdit<'a> {
    ChangedFields,
    Patch(&'a super::stroke::StrokePatch),
    SwapArrows,
}

impl StrokeEdit<'_> {
    fn apply(&self, mut canonical: StrokeStyle, before: &StrokeStyle, next: &StrokeStyle) -> Result<StrokeStyle, CommandError> {
        match self {
            Self::ChangedFields => copy_changed_stroke_fields(&mut canonical, before, next),
            Self::Patch(patch) => return patch.applied(&canonical),
            Self::SwapArrows => std::mem::swap(&mut canonical.start_arrow, &mut canonical.end_arrow),
        }
        Ok(canonical)
    }
}

pub(crate) fn update_strokes(
    doc: &mut Document, changes: &[(ObjectId, StrokeStyle, StrokeStyle)], shared: bool, edit: StrokeEdit<'_>,
) -> Result<bool, CommandError> {
    let mut kept_override = false;
    let mut addressed = std::collections::HashSet::new();
    for (actual, before, next) in changes {
        if before == next {
            continue;
        }
        if let Some((root, definition, node)) = object_link(doc, *actual) {
            if shared && !addressed.insert((definition, node)) {
                continue;
            }
            let stroke = if shared {
                let canonical = original_object(doc, definition, node)
                    .ok_or_else(CommandError::invalid_params)?.stroke.clone();
                edit.apply(canonical, before, next)?
            } else {
                next.clone()
            };
            kept_override |= set_stroke(doc, definition, Some(root), node, stroke, shared, false)?;
        } else {
            write_stroke(doc, *actual, next.clone())?;
        }
    }
    Ok(kept_override)
}

fn copy_changed_stroke_fields(canonical: &mut StrokeStyle, before: &StrokeStyle, next: &StrokeStyle) {
    macro_rules! field {
        ($($name:ident),+ $(,)?) => {
            $(if before.$name != next.$name { canonical.$name = next.$name.clone(); })+
        };
    }
    field!(paint, width, dash, cap, join, align, start_arrow, end_arrow, scale_with_object, pressure);
}

fn set_stroke(
    doc: &mut Document, definition: u64, root: Option<ObjectId>, node: ObjectId,
    stroke: StrokeStyle, shared: bool, revert: bool,
) -> Result<bool, CommandError> {
    if !crate::paint::validate_stroke(&stroke) {
        return Err(CommandError::invalid_params());
    }
    let mut kept_override = false;
    let mut targets = Vec::new();
    if shared {
        let bundle = &mut doc.symbols.definitions.iter_mut().find(|entry| entry.id == definition)
            .ok_or_else(CommandError::invalid_params)?.bundle;
        let original = bundle.objects.iter_mut().find(|object| object.id == node)
            .ok_or_else(CommandError::invalid_params)?;
        if original.stroke.paint != stroke.paint {
            bundle.swatch_bindings.retain(|binding| binding.object != node || binding.target != SwatchTarget::Stroke);
        }
        original.stroke = stroke.clone();
        for instance in doc.symbols.instances.iter().filter(|entry| entry.definition == definition && live_instance(doc, entry)) {
            if let Some(actual) = linked_node(doc, instance, node) {
                if instance.stroke_overrides.contains(&node) {
                    kept_override = true;
                } else {
                    targets.push(actual);
                }
            }
        }
    } else {
        let index = doc.symbols.instances.iter().position(|entry| Some(entry.root) == root && live_instance(doc, entry))
            .ok_or_else(CommandError::invalid_params)?;
        let target = linked_node(doc, &doc.symbols.instances[index], node).ok_or_else(CommandError::invalid_params)?;
        let overrides = &mut doc.symbols.instances[index].stroke_overrides;
        overrides.retain(|id| *id != node);
        if !revert {
            overrides.push(node);
        }
        targets.push(target);
    }
    for target in targets {
        write_stroke(doc, target, stroke.clone())?;
    }
    Ok(kept_override)
}

fn write_stroke(doc: &mut Document, target: ObjectId, stroke: StrokeStyle) -> Result<(), CommandError> {
    let object = doc.find_object_mut(target.0).ok_or_else(CommandError::not_found)?;
    let paint_changed = object.stroke.paint != stroke.paint;
    object.stroke = stroke;
    if paint_changed {
        doc.swatch_bindings.retain(|binding| binding.object != target || binding.target != SwatchTarget::Stroke);
    }
    Ok(())
}

pub(crate) fn warnings(kept_override: bool) -> Vec<&'static str> {
    if kept_override { vec!["SYMBOL_OVERRIDE_KEPT"] } else { Vec::new() }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct LinkedTextState {
    definition: u64,
    canonical: Story,
    instances: Vec<LinkedTextInstance>,
}

impl LinkedTextState {
    pub(crate) fn stories(&self) -> impl Iterator<Item = &Story> {
        self.instances.iter().map(|entry| &entry.story)
    }

    pub(crate) fn set_source_snapshot(&mut self, source: &Story) {
        if let Some(instance) = self.instances.iter_mut().find(|entry| entry.story.id == source.id) {
            instance.story.clone_from(source);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LinkedTextInstance {
    root: ObjectId,
    story: Story,
    overridden: bool,
}

fn story_at(doc: &Document, id: StoryId) -> Option<&Story> {
    doc.stories.iter().find(|story| story.id == id)
}

fn instance_story(doc: &Document, instance: &Instance, definition: &Definition, story: StoryId) -> Option<StoryId> {
    definition.bundle.objects.iter().filter(|object| object.story_id() == Some(story))
        .filter_map(|object| linked_node(doc, instance, object.id))
        .find_map(|id| doc.get_object(id)?.story_id())
}

pub(crate) fn capture_text(doc: &Document, source: StoryId) -> Option<LinkedTextState> {
    if doc.symbols.instances.is_empty() {
        return None;
    }
    let (definition, canonical) = doc.objects().iter().filter(|object| object.story_id() == Some(source))
        .find_map(|object| {
            let (_, definition, node) = object_link(doc, object.id)?;
            Some((definition, original_object(doc, definition, node)?.story_id()?))
        })?;
    let definition = doc.symbols.definitions.iter().find(|entry| entry.id == definition)?;
    let canonical = definition.bundle.stories.iter().find(|entry| entry.id == canonical)?.clone();
    let instances = doc.symbols.instances.iter().filter(|entry| entry.definition == definition.id && live_instance(doc, entry))
        .filter_map(|entry| {
            let story = story_at(doc, instance_story(doc, entry, definition, canonical.id)?)?.clone();
            Some(LinkedTextInstance { root: entry.root, story, overridden: entry.text_overrides.contains(&canonical.id) })
        }).collect();
    Some(LinkedTextState { definition: definition.id, canonical, instances })
}

pub(crate) fn sync_text(doc: &mut Document, source: StoryId, shared: bool) -> Option<(LinkedTextState, LinkedTextState)> {
    let before = capture_text(doc, source)?;
    let source_story = story_at(doc, source)?.clone();
    let mut after = before.clone();
    let source_instance = after.instances.iter_mut().find(|entry| entry.story.id == source)?;
    if shared && !source_instance.overridden {
        after.canonical = story_with_id(&source_story, after.canonical.id);
        for entry in &mut after.instances {
            if !entry.overridden && entry.story.id != source {
                entry.story = story_with_id(&source_story, entry.story.id);
            }
        }
    } else {
        source_instance.overridden = true;
    }
    restore_text(doc, source, &after);
    Some((before, after))
}

fn story_with_id(story: &Story, id: StoryId) -> Story {
    Story { id, ..story.clone() }
}

pub(crate) fn restore_text(doc: &mut Document, source: StoryId, state: &LinkedTextState) {
    let Some(definition) = doc.symbols.definitions.iter_mut().find(|entry| entry.id == state.definition) else { return };
    if let Some(canonical) = definition.bundle.stories.iter_mut().find(|entry| entry.id == state.canonical.id) {
        canonical.clone_from(&state.canonical);
    }
    for entry in &state.instances {
        if let Some(instance) = doc.symbols.instances.iter_mut().find(|instance| instance.root == entry.root) {
            instance.text_overrides.retain(|id| *id != state.canonical.id);
            if entry.overridden {
                instance.text_overrides.push(state.canonical.id);
            }
        }
        if entry.story.id != source {
            replace_story(doc, &entry.story);
        }
    }
}

fn replace_story(doc: &mut Document, story: &Story) {
    if let Some(current) = doc.stories.iter_mut().find(|entry| entry.id == story.id) && current != story {
        current.clone_from(story);
        let _ = crate::text_cmd::refresh_art_bounds(doc, story.id);
    }
}

fn revert_text(doc: &mut Document, definition: u64, root: ObjectId, node: ObjectId) -> Result<(), CommandError> {
    let definition = doc.symbols.definitions.iter().find(|entry| entry.id == definition).ok_or_else(CommandError::invalid_params)?;
    let canonical = original_object(doc, definition.id, node).and_then(Object::story_id).ok_or_else(CommandError::invalid_params)?;
    let instance = doc.symbols.instances.iter().find(|entry| entry.root == root && live_instance(doc, entry)).ok_or_else(CommandError::invalid_params)?;
    let actual = instance_story(doc, instance, definition, canonical).ok_or_else(CommandError::invalid_params)?;
    let story = definition.bundle.stories.iter().find(|entry| entry.id == canonical)
        .map(|entry| story_with_id(entry, actual)).ok_or_else(CommandError::invalid_params)?;
    replace_story(doc, &story);
    if let Some(instance) = doc.symbols.instances.iter_mut().find(|entry| entry.root == root) {
        instance.text_overrides.retain(|id| *id != canonical);
    }
    Ok(())
}
