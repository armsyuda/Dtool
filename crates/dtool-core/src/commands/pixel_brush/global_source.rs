use std::sync::{Arc, Mutex};

use crate::context_toolbar::{CLONE_SCOPE, CLONE_TOOL, HEAL_TOOL, uses_brush_source};
use crate::{CanvasFlags, Command, CommandError, CommandResult, Document, Object, ObjectId, Session};
use serde::Deserialize;
use serde_json::json;

use super::clone_sampling::Scope;

pub const ADD: &str = "clone.source.add";
pub const SELECT: &str = "clone.source.select";
pub const REMOVE: &str = "clone.source.remove";
pub const ANCHOR: &str = "clone.source.anchor.set";
pub const QUERY: &str = "clone.source.query";

const MAX_SOURCES: usize = 16;

#[derive(Debug, Clone)]
pub struct Source {
    pub id: u64,
    pub revision: u64,
    pub name: String,
    pub point: [f32; 2],
    pub bounds: [f32; 4],
    pub(crate) document: Arc<Document>,
    layer: ObjectId,
    scope: Scope,
}

impl Source {
    pub(super) fn render(&self, bounds: [f32; 4], size: [u32; 2]) -> Option<Vec<u8>> {
        use crate::layer_style::raster;
        let doc = &self.document;
        let flags = CanvasFlags::default();
        match self.scope {
            Scope::Current => Some(raster::render_object_alone(doc, &flags, self.layer, bounds, size[0], size[1])),
            Scope::CurrentAndBelow | Scope::Below => raster::render_relative_object_region(
                doc, &flags, self.layer, bounds, size, self.scope == Scope::CurrentAndBelow,
            ),
            Scope::All => Some(raster::render_region(doc, &flags, &[], bounds, size[0], size[1])),
            Scope::Global => None,
        }
    }

    pub fn preview(&self) -> Option<([usize; 2], Vec<u8>)> {
        let [w, h] = [self.bounds[2], self.bounds[3]];
        let factor = 384.0 / w.max(h);
        let size = [w, h].map(|axis| (axis * factor).round().max(1.0) as u32);
        let size_usize = size.map(|value| value as usize);
        let bytes = size_usize[0] * size_usize[1] * 4;
        let pixels = self.render(self.bounds, size).unwrap_or_else(|| vec![0; bytes]);
        (pixels.len() == bytes).then_some((size_usize, pixels))
    }
}

#[derive(Debug, Default)]
struct Sources {
    next_id: u64,
    entries: Vec<Source>,
    active: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Store(Arc<Mutex<Sources>>);

impl Store {
    pub fn entries(&self) -> Vec<Source> {
        self.0.lock().map(|state| state.entries.clone()).unwrap_or_default()
    }

    pub fn selected(&self) -> Option<Source> {
        let state = self.0.lock().ok()?;
        let id = state.active?;
        state.entries.iter().find(|source| source.id == id).cloned()
    }
}

fn candidate(session: &Session, id: Option<ObjectId>) -> Option<&Object> {
    let id = id.or_else(|| session.selection.ids.first().copied())?;
    let object = session.document.get_object(id)?;
    (crate::raster_source::dimensions(&session.document, object).is_some()
        && crate::scene::is_drawn(&session.document, id)).then_some(object)
}

pub(super) fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        ADD => Some(candidate(session, command.params["layer"].as_u64().map(ObjectId)).is_some()),
        SELECT | REMOVE | ANCHOR | QUERY => Some(session.has_document),
        _ => None,
    }
}

fn contains(bounds: [f32; 4], point: [f32; 2]) -> bool {
    (0..2).all(|axis| point[axis].is_finite()
        && point[axis] >= bounds[axis] && point[axis] < bounds[axis] + bounds[axis + 2])
}

fn activate(session: &mut Session) {
    if !uses_brush_source(&session.active_tool) {
        session.active_tool = CLONE_TOOL.to_owned();
    }
    let _ = session.tool_options.set(&session.active_tool, CLONE_SCOPE, &json!("global"));
}

fn add(session: &mut Session, command: &Command) -> CommandResult {
    #[derive(Deserialize, Default)]
    #[serde(default, deny_unknown_fields)]
    struct Args { layer: Option<ObjectId>, point: Option<[f32; 2]>, name: Option<String> }

    let args: Args = if command.params.is_null() { Args::default() } else {
        serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?
    };
    let object = candidate(session, args.layer).ok_or_else(|| CommandError::new("NO_CLONE_SOURCE"))?;
    let bounds = crate::geometry::object_world_bounds(object);
    if !bounds.iter().all(|value| value.is_finite()) || bounds[2] <= 0.0 || bounds[3] <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let point = args.point.unwrap_or([bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5]);
    if !contains(bounds, point) { return Err(CommandError::invalid_params()); }
    let name = args.name.as_deref().filter(|value| !value.trim().is_empty())
        .unwrap_or(object.name.as_str()).trim().chars().take(128).collect::<String>();
    let scope = match Scope::current(session) {
        Scope::Global if session.active_tool == HEAL_TOOL => Scope::Current,
        Scope::Global => Scope::CurrentAndBelow,
        scope => scope,
    };
    let id = {
        let mut state = session.clone_sources.0.lock().map_err(|_| CommandError::invalid_params())?;
        if state.entries.len() >= MAX_SOURCES { return Err(CommandError::new("CLONE_SOURCE_LIMIT")); }
        state.next_id += 1;
        let id = state.next_id;
        let layer = object.id;
        let document = state.entries.iter().find(|source| {
            source.document.instance == session.document.instance && source.document.version == session.document.version
        }).map(|source| Arc::clone(&source.document)).unwrap_or_else(|| Arc::new(session.document.clone()));
        state.entries.push(Source { id, revision: 0, name, point, bounds, document, layer, scope });
        state.active = Some(id);
        id
    };
    activate(session);
    Ok(json!({"id": id, "changes": []}))
}

pub(super) fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !matches!(command.name.as_str(), ADD | SELECT | REMOVE | ANCHOR | QUERY) { return None; }
    Some(change(session, command))
}

fn change(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    if command.name == ADD { return add(session, command); }
    let mut state = session.clone_sources.0.lock().map_err(|_| CommandError::invalid_params())?;
    if command.name == QUERY {
        let entries: Vec<_> = state.entries.iter().map(|source| json!({
            "id": source.id, "name": source.name, "point": source.point, "bounds": source.bounds,
            "revision": source.revision,
        })).collect();
        return Ok(json!({"sources": entries, "selected": state.active, "limit": MAX_SOURCES}));
    }
    let id = command.params["id"].as_u64().ok_or_else(CommandError::invalid_params)?;
    let index = state.entries.iter().position(|source| source.id == id).ok_or_else(CommandError::not_found)?;
    match command.name.as_str() {
        SELECT => state.active = Some(id),
        REMOVE => {
            state.entries.remove(index);
            if state.active == Some(id) {
                state.active = state.entries.get(index).or_else(|| state.entries.last()).map(|source| source.id);
            }
        }
        ANCHOR => {
            let source = &mut state.entries[index];
            let point = serde_json::from_value(command.params["point"].clone())
                .map_err(|_| CommandError::invalid_params())?;
            if !contains(source.bounds, point) { return Err(CommandError::invalid_params()); }
            source.point = point;
            source.revision = source.revision.wrapping_add(1);
        }
        _ => unreachable!(),
    }
    let active = state.active;
    drop(state);
    if command.name == SELECT { activate(session); }
    Ok(json!({"id": id, "selected": active, "changes": []}))
}
