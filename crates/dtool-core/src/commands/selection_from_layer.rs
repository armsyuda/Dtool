use std::collections::HashSet;

use crate::command::{Session, execute_object_delete, pixel_selection_summary};
use crate::document::Document;
use crate::object::{ObjectDeleteArgs, ObjectId, ObjectKind};
use crate::selection::{PixelSelection, PixelSurface, SelectionOp};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::Deserialize;

pub const ALPHA: &str = "selection.fromLayer";
pub const INTENSITY: &str = "selection.fromLayerIntensity";
pub const DELETE_SOURCE: &str = "selection.fromLayerAndDelete";

#[derive(Default, Deserialize)]
struct Args {
    node: Option<ObjectId>,
    nodes: Option<Vec<ObjectId>>,
    #[serde(default)]
    op: SelectionOp,
}

fn selection_sources(session: &Session, command: &Command) -> Result<(Vec<ObjectId>, SelectionOp), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let args: Args = if command.params.is_null() { Args::default() } else {
        serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?
    };
    if command.name == DELETE_SOURCE && args.op != SelectionOp::New {
        return Err(CommandError::invalid_params());
    }
    let doc = &session.document;
    let mut ids = match (args.node, args.nodes) {
        (Some(id), None) => vec![id],
        (None, Some(ids)) => ids,
        (None, None) => session.selection.ids.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    if ids.is_empty() || ids.iter().any(|id| !crate::scene::effective_pickable(doc, *id)) {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    if command.name == DELETE_SOURCE && ids.iter().any(|id| !deletable(doc, *id)) {
        return Err(CommandError::new("LOCKED"));
    }
    Ok((ids, args.op))
}

fn deletable(doc: &Document, id: ObjectId) -> bool {
    let descendants = doc.subtree_ids(id);
    let ancestors = ancestors(doc, id);
    doc.objects.iter().filter(|object| descendants.contains(&object.id) || ancestors.contains(&object.id))
        .all(|object| !object.locked && (!descendants.contains(&object.id) || !matches!(object.kind, ObjectKind::Artboard)))
}

fn ancestors(doc: &Document, id: ObjectId) -> HashSet<ObjectId> {
    let mut ids = HashSet::new();
    let mut parent = doc.get_object(id).and_then(|object| object.parent_id);
    while let Some(id) = parent {
        if !ids.insert(id) { break; }
        parent = doc.get_object(id).and_then(|object| object.parent_id);
    }
    ids
}

fn handles(command: &Command) -> bool {
    matches!(command.name.as_str(), ALPHA | INTENSITY | DELETE_SOURCE)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    handles(command).then(|| selection_sources(session, command).is_ok())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    handles(command).then(|| run_selection(session, command))
}

fn run_selection(session: &mut Session, command: &Command) -> CommandResult {
    let (sources, op) = selection_sources(session, command)?;
    let doc = &session.document;
    let page_index = crate::data_merge::layout::object_page_index(doc, sources[0]);
    if sources.iter().any(|id| crate::data_merge::layout::object_page_index(doc, *id) != page_index) {
        return Err(CommandError::invalid_params());
    }
    let page_id = doc.pages.get(page_index).map(|page| page.id);
    let surface = PixelSurface::for_page(doc, page_id).ok_or_else(CommandError::no_document)?;
    let mut draft = source_mask(doc, &sources, surface, command.name == INTENSITY)?;
    draft.page_id = (page_index != 0).then_some(surface.page_id);
    if draft.is_empty() && command.name == DELETE_SOURCE {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    if op != SelectionOp::New {
        if let Some(previous) = &doc.pixel_selection {
            if !surface.matches(doc, previous) {
                return Err(CommandError::invalid_params());
            }
            let mut combined = previous.clone();
            combined.combine(&draft, op);
            draft = combined;
        } else if matches!(op, SelectionOp::Subtract | SelectionOp::Intersect) {
            draft.coverage.fill(0);
        }
    }
    if command.name == DELETE_SOURCE {
        let mut updated = doc.clone();
        for id in sources.iter().filter(|id| is_source_root(doc, &sources, **id)) {
            execute_object_delete(&mut updated, ObjectDeleteArgs { id: id.0 })?;
        }
        let selected = session.selection.ids.iter().copied()
            .filter(|id| updated.get_object(*id).is_some()).collect();
        updated.pixel_selection = Some(draft);
        let mut result = pixel_selection_summary(&updated);
        result["source_nodes"] = serde_json::json!(sources);
        return super::document_change::commit(session, DELETE_SOURCE, updated, selected, result);
    }
    let before = session.document.pixel_selection.replace(draft);
    let mut result = super::pixel_selection::record_mask(session, command, before);
    result["source_nodes"] = serde_json::json!(sources);
    Ok(result)
}

fn is_source_root(doc: &Document, sources: &[ObjectId], id: ObjectId) -> bool {
    let ancestors = ancestors(doc, id);
    !sources.iter().any(|source| ancestors.contains(source))
}

fn source_mask(doc: &Document, ids: &[ObjectId], surface: PixelSurface, intensity: bool) -> Result<PixelSelection, CommandError> {
    let [w, h] = surface.size;
    if u64::from(w) * u64::from(h) > 32 * 1024 * 1024 {
        return Err(CommandError::validation("PIXEL_SELECTION_TOO_LARGE"));
    }

    let mut visible = HashSet::new();
    for id in ids {
        visible.extend(doc.subtree_ids(*id));
        visible.extend(ancestors(doc, *id));
    }
    let mut source = doc.clone();
    source.objects.retain(|object| visible.contains(&object.id));
    let rgba = crate::layer_style::raster::render_region_scaled(
        &source, &crate::CanvasFlags::default(), &[], surface.bounds, w, h, w as f32 / surface.bounds[2],
    );
    if rgba.len() != w as usize * h as usize * 4 {
        return Err(CommandError::invalid_params());
    }
    let coverage = rgba.chunks_exact(4).map(|pixel| {
        if !intensity { return pixel[3]; }
        let luma = 0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2]);
        (luma * f32::from(pixel[3]) / 255.0).round() as u8
    }).collect();
    Ok(PixelSelection::from_coverage(w, h, coverage))
}
