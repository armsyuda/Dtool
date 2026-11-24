use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::object::ObjectId;

pub const TILE_SIZE: u32 = 256;
pub const TILE_BYTES: usize = (TILE_SIZE as usize) * (TILE_SIZE as usize) * 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub data: Arc<[u8]>,
}

impl Default for Tile {
    fn default() -> Self {
        Self::new()
    }
}

impl Tile {
    pub fn new() -> Self {
        Self { data: Arc::from(vec![0u8; TILE_BYTES]) }
    }

    pub fn filled(rgba: [u8; 4]) -> Self {
        let mut bytes = Vec::with_capacity(TILE_BYTES);
        for _ in 0..(TILE_BYTES / 4) {
            bytes.extend_from_slice(&rgba);
        }
        Self { data: Arc::from(bytes) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut owned = bytes.to_vec();
        owned.resize(TILE_BYTES, 0);
        Self { data: Arc::from(owned) }
    }

    pub fn byte_len(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn is_shared(&self) -> bool {
        Arc::strong_count(&self.data) > 1
    }

    pub fn shares_with(&self, other: &Tile) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    pub fn make_mut(&mut self) -> &mut [u8] {
        if Arc::get_mut(&mut self.data).is_none() {
            let copy: Arc<[u8]> = Arc::from(self.data.to_vec());
            self.data = copy;
        }
        match Arc::get_mut(&mut self.data) {
            Some(data) => data,
            None => unreachable!(),
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= TILE_SIZE || y >= TILE_SIZE {
            return [0; 4];
        }
        let at = ((y as usize) * (TILE_SIZE as usize) + x as usize) * 4;
        match self.data.get(at..at + 4) {
            Some(px) => [px[0], px[1], px[2], px[3]],
            None => [0; 4],
        }
    }

    pub fn is_transparent(&self) -> bool {
        self.data.iter().all(|b| *b == 0)
    }

    fn key(&self) -> usize {
        Arc::as_ptr(&self.data) as *const u8 as usize
    }
}

impl Serialize for Tile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.data)
    }
}

impl<'de> Deserialize<'de> for Tile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        Ok(Tile::from_bytes(&bytes))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PixelLayer {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub tiles: Vec<Option<Tile>>,
}

impl PixelLayer {
    pub fn new(width: u32, height: u32) -> Self {
        let mut layer = Self { width, height, tiles: Vec::new() };
        layer.ensure_tiles();
        layer
    }

    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut layer = Self::new(width, height);
        layer.set_region([0, 0, width, height], rgba);
        layer
    }

    pub fn tile_columns(&self) -> u32 {
        self.width.div_ceil(TILE_SIZE)
    }

    pub fn tile_rows(&self) -> u32 {
        self.height.div_ceil(TILE_SIZE)
    }

    pub fn tile_count(&self) -> usize {
        (self.tile_columns() as usize) * (self.tile_rows() as usize)
    }

    pub fn ensure_tiles(&mut self) {
        let count = self.tile_count();
        if self.tiles.len() != count {
            self.tiles.resize(count, None);
        }
    }

    pub fn tile_index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = ((y / TILE_SIZE) * self.tile_columns() + x / TILE_SIZE) as usize;
        Some(index)
    }

    pub fn tile(&self, index: usize) -> Option<&Tile> {
        self.tiles.get(index).and_then(|t| t.as_ref())
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        match self.tile_index(x, y).and_then(|i| self.tile(i)) {
            Some(tile) => tile.pixel(x % TILE_SIZE, y % TILE_SIZE),
            None => [0; 4],
        }
    }

    pub fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        self.set_region([x, y, 1, 1], &rgba);
    }

    pub fn set_region(&mut self, rect: [u32; 4], rgba: &[u8]) {
        let [rx, ry, rw, rh] = rect;
        if rw == 0 || rh == 0 {
            return;
        }
        let x_end = rx.saturating_add(rw).min(self.width);
        let y_end = ry.saturating_add(rh).min(self.height);
        if rx >= x_end || ry >= y_end {
            return;
        }
        self.ensure_tiles();
        let ts = TILE_SIZE;
        let cols = self.tile_columns();
        let stride = rw as usize * 4;
        for ty in (ry / ts)..=((y_end - 1) / ts) {
            for tx in (rx / ts)..=((x_end - 1) / ts) {
                let px0 = (tx * ts).max(rx);
                let px1 = ((tx + 1) * ts).min(x_end);
                let py0 = (ty * ts).max(ry);
                let py1 = ((ty + 1) * ts).min(y_end);
                let index = (ty * cols + tx) as usize;
                if self.tiles[index].is_none() {
                    let ink = (py0..py1).any(|py| {
                        let start = (py - ry) as usize * stride + (px0 - rx) as usize * 4;
                        let end = (start + (px1 - px0) as usize * 4).min(rgba.len());
                        start < end && rgba[start..end].iter().any(|b| *b != 0)
                    });
                    if !ink {
                        continue;
                    }
                    self.tiles[index] = Some(Tile::new());
                }
                let tile = match self.tiles[index].as_mut() {
                    Some(tile) => tile,
                    None => continue,
                };
                let data = tile.make_mut();
                for py in py0..py1 {
                    let src = (py - ry) as usize * stride + (px0 - rx) as usize * 4;
                    if src >= rgba.len() {
                        break;
                    }
                    let len = ((px1 - px0) as usize * 4).min(rgba.len() - src);
                    let dst = ((py - ty * ts) as usize * ts as usize + (px0 - tx * ts) as usize) * 4;
                    data[dst..dst + len].copy_from_slice(&rgba[src..src + len]);
                }
            }
        }
    }

    pub fn clear_region(&mut self, rect: [u32; 4]) {
        let [_, _, rw, rh] = rect;
        let zeros = vec![0u8; rw as usize * rh as usize * 4];
        self.set_region(rect, &zeros);
        self.compact();
    }

    pub fn compact(&mut self) {
        for slot in self.tiles.iter_mut() {
            if slot.as_ref().is_some_and(|t| t.is_transparent()) {
                *slot = None;
            }
        }
    }

    pub fn read_region(&self, rect: [u32; 4]) -> Vec<u8> {
        let [rx, ry, rw, rh] = rect;
        let mut out = Vec::with_capacity(rw as usize * rh as usize * 4);
        for y in 0..rh {
            for x in 0..rw {
                out.extend_from_slice(&self.get(rx.saturating_add(x), ry.saturating_add(y)));
            }
        }
        out
    }

    pub fn to_rgba(&self) -> Vec<u8> {
        self.read_region([0, 0, self.width, self.height])
    }

    pub fn live_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    pub fn live_bytes(&self) -> u64 {
        self.tiles.iter().flatten().map(Tile::byte_len).sum()
    }

    pub fn shares_tile(&self, other: &PixelLayer, index: usize) -> bool {
        match (self.tile(index), other.tile(index)) {
            (Some(a), Some(b)) => a.shares_with(b),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PixelLayerEntry {
    pub object: ObjectId,
    pub layer: PixelLayer,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryReport {
    pub live_bytes: u64,
    pub history_bytes: u64,
    pub shared_tiles: u64,
    pub spilled_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PixelLedger {
    pub entries: Vec<PixelLayerEntry>,
}

impl PixelLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn contains(&self, object: ObjectId) -> bool {
        self.entries.iter().any(|e| e.object == object)
    }

    pub fn insert(&mut self, object: ObjectId, layer: PixelLayer) -> Option<PixelLayer> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.object == object) {
            return Some(std::mem::replace(&mut entry.layer, layer));
        }
        self.entries.push(PixelLayerEntry { object, layer });
        None
    }

    pub fn get(&self, object: ObjectId) -> Option<&PixelLayer> {
        self.entries.iter().find(|e| e.object == object).map(|e| &e.layer)
    }

    pub fn get_mut(&mut self, object: ObjectId) -> Option<&mut PixelLayer> {
        self.entries.iter_mut().find(|e| e.object == object).map(|e| &mut e.layer)
    }

    pub fn remove(&mut self, object: ObjectId) -> Option<PixelLayer> {
        let at = self.entries.iter().position(|e| e.object == object)?;
        Some(self.entries.remove(at).layer)
    }

    pub fn ids(&self) -> Vec<ObjectId> {
        self.entries.iter().map(|e| e.object).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectId, &PixelLayer)> {
        self.entries.iter().map(|e| (e.object, &e.layer))
    }

    pub fn live_bytes(&self) -> u64 {
        let mut seen = HashSet::new();
        let mut total = 0u64;
        for tile in self.entries.iter().flat_map(|e| e.layer.tiles.iter().flatten()) {
            if seen.insert(tile.key()) {
                total += tile.byte_len();
            }
        }
        total
    }

    pub fn memory(&self, history: &[PixelLedger]) -> MemoryReport {
        let mut seen = HashSet::new();
        let mut report = MemoryReport::default();
        for tile in self.entries.iter().flat_map(|e| e.layer.tiles.iter().flatten()) {
            if seen.insert(tile.key()) {
                report.live_bytes += tile.byte_len();
            }
        }
        for state in history {
            for tile in state.entries.iter().flat_map(|e| e.layer.tiles.iter().flatten()) {
                if seen.insert(tile.key()) {
                    report.history_bytes += tile.byte_len();
                } else {
                    report.shared_tiles += 1;
                }
            }
        }
        report
    }
}
