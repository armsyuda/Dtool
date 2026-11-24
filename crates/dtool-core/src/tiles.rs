use crate::command::Session;
use crate::object::ObjectId;
use crate::pixel::TILE_BYTES;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub use crate::pixel::{MemoryReport, PixelLayer, PixelLedger, Tile, TILE_SIZE};

pub const HISTORY_MEMORY: &str = "history.memory";
pub const DEFAULT_MEMORY_BUDGET: u64 = 512 * 1024 * 1024;

static SPILL_SEQ: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static HISTORY: RefCell<TileHistory> = RefCell::new(TileHistory::new());
}

#[derive(Clone)]
struct StoredTile {
    data: Option<Arc<[u8]>>,
    spill_path: Option<PathBuf>,
    bytes: u64,
}

#[derive(Clone)]
struct StoredLayer {
    width: u32,
    height: u32,
    tiles: Vec<Option<StoredTile>>,
}

#[derive(Clone)]
struct PackedLayer {
    layer: StoredLayer,
}

#[derive(Clone)]
struct StoredState {
    layers: Vec<(ObjectId, StoredLayer)>,
    rasters: Vec<(ObjectId, PackedLayer)>,
    mask: Option<StoredLayer>,
    masks: Vec<(ObjectId, StoredLayer)>,
    resources: Vec<(u64, PackedLayer)>,
}

struct TileHistory {
    budget: u64,
    steps: Vec<StoredState>,
    redo: Vec<StoredState>,
    current: Option<StoredState>,
}

impl TileHistory {
    fn new() -> Self {
        Self {
            budget: DEFAULT_MEMORY_BUDGET,
            steps: Vec::new(),
            redo: Vec::new(),
            current: None,
        }
    }
}

pub fn swap_dir() -> PathBuf {
    std::env::temp_dir().join("DTool").join("swap")
}

fn tile_ptr(data: &Arc<[u8]>) -> usize {
    Arc::as_ptr(data) as *const u8 as usize
}

impl StoredTile {
    fn from_arc(data: Arc<[u8]>) -> Self {
        let bytes = data.len() as u64;
        Self { data: Some(data), spill_path: None, bytes }
    }

    fn from_tile(tile: &Tile) -> Self {
        Self::from_arc(tile.data.clone())
    }
}

fn write_spill(data: &Arc<[u8]>) -> Option<PathBuf> {
    let dir = swap_dir();
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let name = format!("tile_{:016x}.bin", SPILL_SEQ.fetch_add(1, Ordering::Relaxed));
    let path = dir.join(name);
    if fs::write(&path, data.as_ref()).is_err() {
        return None;
    }
    Some(path)
}

impl StoredLayer {
    fn from_pixel(layer: &PixelLayer) -> Self {
        let tiles = layer.tiles.iter().map(|slot| slot.as_ref().map(StoredTile::from_tile)).collect();
        Self { width: layer.width, height: layer.height, tiles }
    }

    fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        Self::from_pixel(&PixelLayer::from_rgba(width, height, rgba))
    }

    fn from_coverage(width: u32, height: u32, coverage: &[u8]) -> Self {
        let mut rgba = Vec::with_capacity(coverage.len().saturating_mul(4));
        for value in coverage {
            rgba.extend_from_slice(&[*value, *value, *value, *value]);
        }
        Self::from_rgba(width, height, &rgba)
    }

    fn from_chunks(bytes: &[u8]) -> Self {
        let mut tiles = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + TILE_BYTES).min(bytes.len());
            let mut block = vec![0u8; TILE_BYTES];
            block[..end - offset].copy_from_slice(&bytes[offset..end]);
            tiles.push(Some(StoredTile::from_arc(Arc::from(block))));
            offset = end;
        }
        Self { width: bytes.len() as u32, height: 1, tiles }
    }

    fn fingerprint(&self) -> Vec<u64> {
        let mut out = vec![self.width as u64, self.height as u64, self.tiles.len() as u64];
        for slot in &self.tiles {
            match slot {
                Some(tile) => {
                    if let Some(data) = &tile.data {
                        out.push(tile_ptr(data) as u64);
                    } else if let Some(path) = &tile.spill_path {
                        let mut hash = 0u64;
                        for byte in path.to_string_lossy().as_bytes() {
                            hash = hash.wrapping_mul(16777619) ^ (*byte as u64);
                        }
                        out.push(hash | (1u64 << 63));
                    } else {
                        out.push(0);
                    }
                }
                None => out.push(u64::MAX),
            }
        }
        out
    }
}

fn packed_from_bytes(width: u32, height: u32, data: &[u8]) -> PackedLayer {
    if width > 0 && height > 0 && data.len() == width as usize * height as usize * 4 {
        PackedLayer { layer: StoredLayer::from_rgba(width, height, data) }
    } else {
        PackedLayer { layer: StoredLayer::from_chunks(data) }
    }
}

fn collect_state(session: &Session) -> StoredState {
    let mut layers: Vec<(ObjectId, StoredLayer)> = session
        .document
        .pixel_layers
        .iter()
        .map(|(id, layer)| (id, StoredLayer::from_pixel(layer)))
        .collect();
    layers.sort_by_key(|(id, _)| id.0);

    let mut rasters = Vec::new();
    for object in &session.document.objects {
        if session.document.pixel_layers.get(object.id).is_some() {
            continue;
        }
        if let (Some(data), Some((width, height))) = (&object.raster_data, object.raster_size) {
            rasters.push((object.id, packed_from_bytes(width, height, data)));
        }
    }
    rasters.sort_by_key(|(id, _)| id.0);

    let mask = session.document.pixel_selection.as_ref().map(|sel| {
        StoredLayer::from_coverage(sel.width, sel.height, &sel.coverage)
    });

    let mut resources = Vec::new();
    for resource in &session.document.resources {
        if let Some(data) = &resource.data {
            let [width, height] = resource.pixel_size;
            resources.push((resource.id.0, packed_from_bytes(width, height, data)));
        }
    }
    resources.sort_by_key(|(id, _)| *id);

    let mut masks: Vec<(ObjectId, StoredLayer)> = session
        .document
        .layer_masks
        .iter()
        .map(|mask| {
            (
                mask.object,
                StoredLayer::from_coverage(mask.coverage.width, mask.coverage.height, &mask.coverage.coverage),
            )
        })
        .collect();
    masks.sort_by_key(|(id, _)| id.0);

    StoredState { layers, rasters, mask, masks, resources }
}

fn for_tiles(state: &StoredState, visit: &mut impl FnMut(&StoredTile)) {
    for (_, layer) in &state.layers {
        for slot in &layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    for (_, packed) in &state.rasters {
        for slot in &packed.layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    if let Some(mask) = &state.mask {
        for slot in &mask.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    for (_, layer) in &state.masks {
        for slot in &layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    for (_, packed) in &state.resources {
        for slot in &packed.layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
}

fn for_tiles_mut(state: &mut StoredState, visit: &mut impl FnMut(&mut StoredTile)) {
    for (_, layer) in &mut state.layers {
        for slot in &mut layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    for (_, packed) in &mut state.rasters {
        for slot in &mut packed.layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    if let Some(mask) = &mut state.mask {
        for slot in &mut mask.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    for (_, layer) in &mut state.masks {
        for slot in &mut layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
    for (_, packed) in &mut state.resources {
        for slot in &mut packed.layer.tiles {
            if let Some(tile) = slot {
                visit(tile);
            }
        }
    }
}


fn content_fingerprint(layer: &StoredLayer) -> Vec<u64> {
        let mut out = vec![layer.width as u64, layer.height as u64, layer.tiles.len() as u64];
        let mut hash = 0xcbf29ce484222325u64;
        for slot in &layer.tiles {
            match slot {
                Some(tile) => {
                    if let Some(data) = &tile.data {
                        out.push(tile.bytes);
                        for byte in data.iter() {
                            hash ^= u64::from(*byte);
                            hash = hash.wrapping_mul(0x100000001b3);
                        }
                    } else if let Some(path) = &tile.spill_path {
                        for byte in path.to_string_lossy().as_bytes() {
                            hash ^= u64::from(*byte);
                            hash = hash.wrapping_mul(0x100000001b3);
                        }
                        out.push(hash | (1u64 << 63));
                    } else {
                        out.push(0);
                    }
                }
                None => out.push(u64::MAX),
            }
        }
        out.push(hash);
        out
    }

impl StoredState {
    fn fingerprint(&self) -> Vec<u64> {
        let mut out = Vec::new();
        out.push(self.layers.len() as u64);
        for (id, layer) in &self.layers {
            out.push(id.0);
            out.extend(layer.fingerprint());
        }
        out.push(self.rasters.len() as u64);
        for (id, packed) in &self.rasters {
            out.push(id.0);
            out.extend(packed.layer.fingerprint());
        }
        match &self.mask {
            Some(layer) => {
                out.push(1);
                out.extend(content_fingerprint(layer));
            }
            None => out.push(0),
        }
        out.push(self.masks.len() as u64);
        for (id, layer) in &self.masks {
            out.push(id.0);
            out.extend(content_fingerprint(layer));
        }
        out.push(self.resources.len() as u64);
        for (id, packed) in &self.resources {
            out.push(*id);
            out.extend(packed.layer.fingerprint());
        }
        out
    }
}

fn collect_ptrs(state: &StoredState, into: &mut HashSet<usize>) {
    for_tiles(state, &mut |tile| {
        if let Some(data) = &tile.data {
            into.insert(tile_ptr(data));
        }
    });
}

fn report_of(history: &TileHistory) -> MemoryReport {
    let mut seen = HashSet::new();
    let mut spilled = HashSet::new();
    let mut report = MemoryReport::default();
    if let Some(current) = &history.current {
        for_tiles(current, &mut |tile| {
            if let Some(data) = &tile.data {
                if seen.insert(tile_ptr(data)) {
                    report.live_bytes += tile.bytes;
                }
            } else if let Some(path) = &tile.spill_path {
                if spilled.insert(path.clone()) {
                    report.spilled_bytes += tile.bytes;
                }
            }
        });
    }
    for state in history.steps.iter().chain(history.redo.iter()) {
        for_tiles(state, &mut |tile| {
            if let Some(data) = &tile.data {
                if seen.insert(tile_ptr(data)) {
                    report.history_bytes += tile.bytes;
                } else {
                    report.shared_tiles += 1;
                }
            } else if let Some(path) = &tile.spill_path {
                if spilled.insert(path.clone()) {
                    report.spilled_bytes += tile.bytes;
                }
            }
        });
    }
    report
}

fn clear_ptr(history: &mut TileHistory, ptr: usize, path: PathBuf) {
    let apply = |state: &mut StoredState| {
        for_tiles_mut(state, &mut |tile| {
            let matches = tile.data.as_ref().is_some_and(|data| tile_ptr(data) == ptr);
            if matches {
                tile.spill_path = Some(path.clone());
                tile.data = None;
            }
        });
    };
    if let Some(current) = &mut history.current {
        apply(current);
    }
    for state in &mut history.steps {
        apply(state);
    }
    for state in &mut history.redo {
        apply(state);
    }
}

impl TileHistory {
    fn maybe_spill(&mut self) {
        loop {
            let report = report_of(self);
            if report.live_bytes.saturating_add(report.history_bytes) <= self.budget {
                break;
            }
            if !self.spill_one() {
                break;
            }
        }
    }

    fn spill_one(&mut self) -> bool {
        let mut protected = HashSet::new();
        if let Some(current) = &self.current {
            collect_ptrs(current, &mut protected);
        }
        let mut candidate: Option<(usize, Arc<[u8]>)> = None;
        for state in self.steps.iter().chain(self.redo.iter()) {
            for_tiles(state, &mut |tile| {
                if candidate.is_some() {
                    return;
                }
                if let Some(data) = &tile.data {
                    let ptr = tile_ptr(data);
                    if !protected.contains(&ptr) {
                        candidate = Some((ptr, data.clone()));
                    }
                }
            });
            if candidate.is_some() {
                break;
            }
        }
        let Some((ptr, data)) = candidate else {
            return false;
        };
        let Some(path) = write_spill(&data) else {
            return false;
        };
        drop(data);
        clear_ptr(self, ptr, path);
        true
    }

    fn sync(&mut self, session: &Session) {
        if !session.has_document {
            return;
        }
        let live = collect_state(session);
        match &self.current {
            Some(current) if current.fingerprint() == live.fingerprint() => {}
            Some(_) => {
                if let Some(current) = self.current.take() {
                    self.steps.push(current);
                }
                self.redo.clear();
                self.current = Some(live);
                self.maybe_spill();
            }
            None => {
                self.current = Some(live);
            }
        }
    }
}

pub fn history_memory(session: &Session) -> MemoryReport {
    HISTORY.with(|slot| {
        let mut history = slot.borrow_mut();
        history.sync(session);
        report_of(&history)
    })
}

pub fn set_memory_budget(budget: u64) {
    HISTORY.with(|slot| {
        let mut history = slot.borrow_mut();
        history.budget = budget;
        history.maybe_spill();
    });
}

pub fn memory_budget() -> u64 {
    HISTORY.with(|slot| slot.borrow().budget)
}

fn layer_size(session: &Session, layer_id: ObjectId, rect: [u32; 4]) -> (u32, u32) {
    if let Some(layer) = session.document.pixel_layers.get(layer_id) {
        return (layer.width, layer.height);
    }
    if let Some(object) = session.document.objects.iter().find(|item| item.id == layer_id) {
        if let Some((width, height)) = object.raster_size {
            return (width.max(rect[0].saturating_add(rect[2])), height.max(rect[1].saturating_add(rect[3])));
        }
    }
    (rect[0].saturating_add(rect[2]).max(1), rect[1].saturating_add(rect[3]).max(1))
}

fn blit_rgba(data: &mut [u8], width: u32, height: u32, rect: [u32; 4], rgba: &[u8]) {
    let [rx, ry, rw, rh] = rect;
    let x_end = rx.saturating_add(rw).min(width);
    let y_end = ry.saturating_add(rh).min(height);
    if rx >= x_end || ry >= y_end {
        return;
    }
    let stride = rw as usize * 4;
    for y in ry..y_end {
        let src = (y - ry) as usize * stride;
        if src >= rgba.len() {
            break;
        }
        let len = ((x_end - rx) as usize * 4).min(rgba.len() - src);
        let dst = (y as usize * width as usize + rx as usize) * 4;
        if dst + len <= data.len() {
            data[dst..dst + len].copy_from_slice(&rgba[src..src + len]);
        }
    }
}

pub fn paint_region(session: &mut Session, layer_id: ObjectId, rect: [u32; 4], rgba: &[u8]) -> Result<(), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let [_, _, rw, rh] = rect;
    if rw == 0 || rh == 0 || rgba.len() != rw as usize * rh as usize * 4 {
        return Err(CommandError::invalid_params());
    }
    let before_document = session.document.clone();
    let before_selection = session.selection.ids.clone();
    HISTORY.with(|slot| {
        let mut history = slot.borrow_mut();
        history.sync(session);
        if session.document.pixel_layers.get(layer_id).is_none() {
            let (width, height) = layer_size(session, layer_id, rect);
            let mut layer = PixelLayer::new(width, height);
            if let Some(object) = session.document.objects.iter().find(|item| item.id == layer_id) {
                if let (Some(data), Some((rw, rh))) = (&object.raster_data, object.raster_size) {
                    if data.len() == rw as usize * rh as usize * 4 {
                        layer.set_region([0, 0, rw, rh], data);
                    }
                }
            }
            session.document.pixel_layers.insert(layer_id, layer);
            history.current = Some(collect_state(session));
        }
        let before = history.current.clone().unwrap_or_else(|| collect_state(session));
        history.steps.push(before);
        history.redo.clear();
        if let Some(layer) = session.document.pixel_layers.get_mut(layer_id) {
            layer.set_region(rect, rgba);
        }
        if let Some(object) = session.document.objects.iter_mut().find(|item| item.id == layer_id) {
            if let (Some(data), Some((width, height))) = (object.raster_data.as_mut(), object.raster_size) {
                blit_rgba(data, width, height, rect, rgba);
            }
        }
        history.current = Some(collect_state(session));
        history.maybe_spill();
    });
    session.commit_replacement("paint_region", &before_document, &before_selection);
    Ok(())
}

pub fn undo_history(session: &mut Session) -> Result<(), CommandError> {
    step_history(session, "edit.undo")
}

pub fn redo_history(session: &mut Session) -> Result<(), CommandError> {
    step_history(session, "edit.redo")
}

fn step_history(session: &mut Session, name: &str) -> Result<(), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    session.run(&Command::new(name))?;
    HISTORY.with(|slot| slot.borrow_mut().sync(session));
    Ok(())
}

fn params_ok(params: &serde_json::Value) -> bool {
    params.is_null() || params.is_object()
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if command.name != HISTORY_MEMORY {
        return None;
    }
    Some(session.has_document && params_ok(&command.params))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name != HISTORY_MEMORY {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        if !params_ok(&command.params) {
            return Err(CommandError::invalid_params());
        }
        let report = history_memory(session);
        Ok(json!({
            "live_bytes": report.live_bytes,
            "history_bytes": report.history_bytes,
            "shared_tiles": report.shared_tiles,
            "spilled_bytes": report.spilled_bytes
        }))
    })())
}

pub fn commit(session: &Session) {
    HISTORY.with(|slot| {
        slot.borrow_mut().sync(session);
    });
}

#[cfg(test)]
mod k09_contracts {
    use super::*;
    use crate::document::{Document, Units};
    use crate::object::Object;
    use crate::raster_source::LayerMask;
    use crate::selection::PixelSelection;

    fn tile_bytes(session: &Session, id: ObjectId) -> Vec<u8> {
        session.document.pixel_layers.get(id).and_then(|layer| layer.tile(0)).map(|tile| tile.data.to_vec()).unwrap_or_default()
    }

    fn spill_paths() -> Vec<PathBuf> {
        HISTORY.with(|slot| {
            let history = slot.borrow();
            let mut paths = Vec::new();
            for state in history.current.iter().chain(history.steps.iter()).chain(history.redo.iter()) {
                for_tiles(state, &mut |tile| {
                    if let Some(path) = &tile.spill_path {
                        if !paths.contains(path) {
                            paths.push(path.clone());
                        }
                    }
                });
            }
            paths
        })
    }

    #[test]
    fn stamp_on_4096_layer_keeps_one_tile_per_step_spills_and_undoes_20_steps() {
        let mut session = Session::new(Document::new([4096.0, 4096.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [4096.0, 4096.0], ..Object::default() });
        let mut layer = PixelLayer::new(4096, 4096);
        layer.set_region([0, 0, TILE_SIZE, TILE_SIZE], &vec![9u8; TILE_BYTES]);
        session.document.pixel_layers.insert(id, layer);
        let tile = TILE_BYTES as u64;
        let mut states = vec![tile_bytes(&session, id)];
        let mut last = history_memory(&session).history_bytes;
        for step in 0..20u32 {
            let stamp: Vec<u8> = (0..100).flat_map(|_| [step as u8 + 1, 2, 3, 255]).collect();
            paint_region(&mut session, id, [step * 12, 0, 10, 10], &stamp).unwrap();
            let report = history_memory(&session);
            assert_eq!(report.history_bytes - last, tile);
            assert_eq!(report.live_bytes, tile);
            last = report.history_bytes;
            states.push(tile_bytes(&session, id));
        }
        assert_eq!(session.document.pixel_layers.get(id).unwrap().tiles.iter().flatten().count(), 1);

        set_memory_budget(tile);
        let report = history_memory(&session);
        assert_eq!(report.history_bytes, 0);
        assert_eq!(report.spilled_bytes, 20 * tile);
        HISTORY.with(|slot| {
            let history = slot.borrow();
            assert_eq!(history.steps.len(), 20);
            for (index, step) in history.steps.iter().enumerate() {
                let stored = step.layers[0].1.tiles[0].as_ref().unwrap();
                assert!(stored.data.is_none());
                let path = stored.spill_path.as_ref().unwrap();
                assert_eq!(fs::read(path).unwrap(), states[index]);
            }
        });

        for back in 1..=20 {
            undo_history(&mut session).unwrap();
            assert_eq!(tile_bytes(&session, id), states[20 - back]);
        }
        for forward in 1..=20 {
            redo_history(&mut session).unwrap();
            assert_eq!(tile_bytes(&session, id), states[forward]);
        }

        for path in spill_paths() {
            let _ = fs::remove_file(path);
        }
        set_memory_budget(DEFAULT_MEMORY_BUDGET);
    }

    #[test]
    fn short_or_empty_buffer_is_rejected_without_any_change() {
        let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [4.0, 4.0], ..Object::default() });
        session.document.pixel_layers.insert(id, PixelLayer::from_rgba(4, 4, &[255u8; 64]));
        let before = session.document.pixel_layers.clone();
        let version = session.document.version;
        let generation = session.generation;
        let error = paint_region(&mut session, id, [0, 0, 2, 2], &[1, 2, 3, 4]).unwrap_err();
        assert_eq!(error.code, "INVALID_PARAMS");
        assert!(paint_region(&mut session, id, [0, 0, 0, 2], &[]).is_err());
        assert_eq!(session.document.pixel_layers, before);
        assert_eq!(session.document.version, version);
        assert_eq!(session.generation, generation);
    }

    #[test]
    fn delete_then_undo_restores_painted_pixels_and_mask_together() {
        let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [4.0, 4.0], ..Object::default() });
        session.document.pixel_layers.insert(id, PixelLayer::from_rgba(4, 4, &[255u8; 64]));
        session.document.layer_masks.push(LayerMask { object: id, coverage: PixelSelection::from_coverage(4, 4, vec![200; 16]) });
        paint_region(&mut session, id, [1, 1, 1, 1], &[1, 2, 3, 4]).unwrap();
        let painted = session.document.pixel_layers.get(id).unwrap().to_rgba();
        session.run_named("object.delete", json!({ "id": id.0 })).unwrap();
        assert!(session.document.pixel_layers.get(id).is_none());
        assert!(session.document.layer_masks.is_empty());
        session.run_named("edit.undo", json!({})).unwrap();
        assert_eq!(session.document.pixel_layers.get(id).unwrap().to_rgba(), painted);
        assert_eq!(session.document.layer_masks.len(), 1);
        assert_eq!(session.document.layer_masks[0].coverage.coverage, vec![200; 16]);
        undo_history(&mut session).unwrap();
        assert_eq!(session.document.pixel_layers.get(id).unwrap().get(1, 1), [255, 255, 255, 255]);
        assert_eq!(session.document.layer_masks[0].coverage.coverage, vec![200; 16]);
    }
}
