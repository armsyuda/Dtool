use crate::command::Session;
use crate::document::Document;
use crate::object::{Object, ObjectId};
use crate::selection::SelectOp;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ASSET_LIST: &str = "asset.list";
pub const ASSET_ADD: &str = "asset.add";
pub const ASSET_PLACE: &str = "asset.place";
pub const ASSET_DELETE: &str = "asset.delete";
pub const ASSET_ADD_CATEGORY: &str = "asset.add_category";
pub const ASSET_ADD_GROUP: &str = "asset.add_group";
pub const LIBRARY_FILE: &str = "library.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetCategory {
    pub name: String,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryAsset {
    pub id: u64,
    pub name: String,
    pub category: String,
    pub group: String,
    pub nodes: Vec<Object>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetLibrary {
    #[serde(default)]
    pub categories: Vec<AssetCategory>,
    #[serde(default)]
    pub assets: Vec<LibraryAsset>,
    #[serde(default)]
    pub next_id: u64,
}

impl AssetLibrary {
    pub fn ensure_group(&mut self, category: &str, group: &str) {
        let index = match self.categories.iter().position(|item| item.name == category) {
            Some(index) => index,
            None => {
                self.categories.push(AssetCategory { name: category.to_string(), groups: Vec::new() });
                self.categories.len() - 1
            }
        };
        if !group.is_empty() && !self.categories[index].groups.iter().any(|item| item == group) {
            self.categories[index].groups.push(group.to_string());
        }
    }

    pub fn add(&mut self, name: &str, category: &str, group: &str, nodes: Vec<Object>) -> u64 {
        self.ensure_group(category, group);
        let id = self.next_id.max(self.assets.iter().map(|item| item.id).max().unwrap_or(0)) + 1;
        self.next_id = id;
        self.assets.push(LibraryAsset { id, name: name.to_string(), category: category.to_string(), group: group.to_string(), nodes });
        id
    }

    pub fn remove(&mut self, id: u64) -> Option<LibraryAsset> {
        let index = self.assets.iter().position(|item| item.id == id)?;
        Some(self.assets.remove(index))
    }

    pub fn get(&self, id: u64) -> Option<&LibraryAsset> {
        self.assets.iter().find(|item| item.id == id)
    }
}

pub fn library_dir() -> Option<PathBuf> {
    let root = std::env::var_os("APPDATA").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(root).join("DTool").join("assets"))
}

pub fn library_path() -> Result<PathBuf, CommandError> {
    library_dir().map(|dir| dir.join(LIBRARY_FILE)).ok_or_else(|| CommandError::new("IO.OTHER"))
}

pub fn load_library(path: &Path) -> AssetLibrary {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn save_library(path: &Path, library: &AssetLibrary) -> Result<(), CommandError> {
    let parent = path.parent().ok_or_else(CommandError::invalid_params)?;
    std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO.OTHER"))?;
    let text = serde_json::to_string_pretty(library).map_err(|_| CommandError::invalid_params())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|_| CommandError::new("IO.OTHER"))?;
    std::fs::rename(&tmp, path).map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        CommandError::new("IO.OTHER")
    })
}

fn with_library<R>(save: bool, f: impl FnOnce(&mut AssetLibrary) -> Result<R, CommandError>) -> Result<R, CommandError> {
    let path = library_path()?;
    let mut library = load_library(&path);
    let result = f(&mut library)?;
    if save {
        save_library(&path, &library)?;
    }
    Ok(result)
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    match cmd.name.as_str() {
        ASSET_ADD => Some(session.has_document && !session.selection.ids.is_empty()),
        ASSET_PLACE => Some(session.has_document),
        ASSET_LIST | ASSET_DELETE | ASSET_ADD_CATEGORY | ASSET_ADD_GROUP => Some(true),
        _ => None,
    }
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    match cmd.name.as_str() {
        ASSET_LIST => Some(list(cmd)),
        ASSET_ADD => Some(add(session, cmd)),
        ASSET_PLACE => Some(place(session, cmd)),
        ASSET_DELETE => Some(delete(cmd)),
        ASSET_ADD_CATEGORY => Some(add_category(cmd)),
        ASSET_ADD_GROUP => Some(add_group(cmd)),
        _ => None,
    }
}

fn text_param<'a>(cmd: &'a Command, key: &str) -> Option<&'a str> {
    cmd.params.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn id_param(cmd: &Command) -> Result<u64, CommandError> {
    ["asset_id", "id"].iter().find_map(|key| cmd.params.get(*key).and_then(Value::as_u64)).ok_or_else(CommandError::invalid_params)
}

fn library_value(library: &AssetLibrary, category: Option<&str>) -> Value {
    let assets: Vec<Value> = library
        .assets
        .iter()
        .filter(|item| category.is_none_or(|name| item.category == name))
        .map(|item| json!({ "id": item.id, "name": item.name, "category": item.category, "group": item.group, "nodes": item.nodes.len() }))
        .collect();
    json!({ "categories": library.categories, "assets": assets })
}

fn list(cmd: &Command) -> CommandResult {
    with_library(false, |library| Ok(library_value(library, text_param(cmd, "category"))))
}

fn add_category(cmd: &Command) -> CommandResult {
    let name = text_param(cmd, "name").ok_or_else(CommandError::invalid_params)?.to_string();
    with_library(true, |library| {
        library.ensure_group(&name, "");
        Ok(library_value(library, None))
    })
}

fn add_group(cmd: &Command) -> CommandResult {
    let category = text_param(cmd, "category").ok_or_else(CommandError::invalid_params)?.to_string();
    let name = text_param(cmd, "name").ok_or_else(CommandError::invalid_params)?.to_string();
    with_library(true, |library| {
        if !library.categories.iter().any(|item| item.name == category) {
            return Err(CommandError::not_found());
        }
        library.ensure_group(&category, &name);
        Ok(library_value(library, None))
    })
}

pub fn subtree(document: &Document, ids: &[ObjectId]) -> Vec<Object> {
    let mut keep: Vec<ObjectId> = ids.to_vec();
    let mut grew = true;
    while grew {
        grew = false;
        for object in &document.objects {
            if let Some(parent) = object.parent_id {
                if keep.contains(&parent) && !keep.contains(&object.id) {
                    keep.push(object.id);
                    grew = true;
                }
            }
        }
    }
    document.objects.iter().filter(|object| keep.contains(&object.id)).cloned().collect()
}

fn add(session: &mut Session, cmd: &Command) -> CommandResult {
    let category = text_param(cmd, "category").ok_or_else(CommandError::invalid_params)?.to_string();
    let group = text_param(cmd, "group").unwrap_or("").to_string();
    if session.selection.ids.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let nodes = subtree(&session.document, &session.selection.ids);
    if nodes.is_empty() {
        return Err(CommandError::not_found());
    }
    let name = text_param(cmd, "name").map(str::to_string).unwrap_or_else(|| nodes[0].name.clone());
    let count = nodes.len();
    with_library(true, |library| {
        let id = library.add(&name, &category, &group, nodes);
        Ok(json!({ "id": id, "asset_id": id, "nodes": count }))
    })
}

fn delete(cmd: &Command) -> CommandResult {
    let id = id_param(cmd)?;
    with_library(true, |library| {
        library.remove(id).ok_or_else(CommandError::not_found)?;
        Ok(json!({ "deleted": id }))
    })
}

fn bounds_origin(nodes: &[Object]) -> [f32; 2] {
    let ids: Vec<ObjectId> = nodes.iter().map(|node| node.id).collect();
    let roots: Vec<&Object> = nodes.iter().filter(|node| node.parent_id.is_none_or(|parent| !ids.contains(&parent))).collect();
    let pick = if roots.is_empty() { nodes.iter().collect() } else { roots };
    let x = pick.iter().map(|node| node.pos[0].min(node.pos[0] + node.size[0])).fold(f32::INFINITY, f32::min);
    let y = pick.iter().map(|node| node.pos[1].min(node.pos[1] + node.size[1])).fold(f32::INFINITY, f32::min);
    [if x.is_finite() { x } else { 0.0 }, if y.is_finite() { y } else { 0.0 }]
}

fn place(session: &mut Session, cmd: &Command) -> CommandResult {
    let id = id_param(cmd)?;
    let at = match cmd.params.get("pos") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let pos: [f32; 2] = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
            if pos.iter().any(|n| !n.is_finite()) {
                return Err(CommandError::invalid_params());
            }
            Some(pos)
        }
    };
    let asset = with_library(false, |library| library.get(id).cloned().ok_or_else(CommandError::not_found))?;
    let origin = bounds_origin(&asset.nodes);
    let delta = at.map(|pos| [pos[0] - origin[0], pos[1] - origin[1]]).unwrap_or([0.0, 0.0]);
    let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut placed = Vec::new();
    for node in &asset.nodes {
        let mut clone = node.clone();
        let old = clone.id;
        clone.id = ObjectId(0);
        clone.parent_id = node.parent_id.and_then(|parent| map.get(&parent).copied());
        clone.pos = [clone.pos[0] + delta[0], clone.pos[1] + delta[1]];
        let new_id = session.document.create_object(clone);
        map.insert(old, new_id);
    }
    for node in &asset.nodes {
        if let (Some(new_id), Some(parent)) = (map.get(&node.id), node.parent_id) {
            if let Some(mapped) = map.get(&parent).copied() {
                if let Some(object) = session.document.objects.iter_mut().find(|object| object.id == *new_id) {
                    object.parent_id = Some(mapped);
                }
            }
        }
    }
    for node in &asset.nodes {
        if let Some(object) = map.get(&node.id).and_then(|new_id| session.document.get_object(*new_id)) {
            placed.push(object.clone());
        }
    }
    let ids: Vec<u64> = placed.iter().map(|object| object.id.0).collect();
    let roots: Vec<ObjectId> = placed.iter().filter(|object| object.parent_id.is_none_or(|parent| !ids.contains(&parent.0))).map(|object| object.id).collect();
    session.selection.set(roots, SelectOp::Replace);
    session.undo.push(crate::undo::Change {
        name: cmd.name.clone(),
        redo: Command::with_params("asset.place.redo", json!({ "objects": placed })),
        undo: Command::with_params("asset.place.undo", json!({ "placed": ids })),
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(json!({ "doc_version": session.generation, "ids": ids }))
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    match cmd.name.as_str() {
        "asset.place.redo" => {
            let objects: Result<Vec<Object>, CommandError> = serde_json::from_value(cmd.params.get("objects").cloned().unwrap_or(Value::Null)).map_err(|_| CommandError::invalid_params());
            Some(objects.map(|objects| {
                for obj in objects {
                    session.document.create_object(obj);
                }
                session.document.version += 1;
            }))
        }
        "asset.place.undo" => Some(undo(cmd, &mut session.document)),
        _ => None,
    }
}

pub fn undo(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let placed: Vec<u64> = serde_json::from_value(cmd.params.get("placed").cloned().unwrap_or(Value::Null)).map_err(|_| CommandError::invalid_params())?;
    for node_id in placed.into_iter().rev() {
        if doc.get_object(ObjectId(node_id)).is_some() {
            doc.delete_object(ObjectId(node_id))?;
        }
    }
    doc.version += 1;
    Ok(())
}