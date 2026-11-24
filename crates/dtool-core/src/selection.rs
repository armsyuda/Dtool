use serde::{Deserialize, Serialize};

use dtool_protocol::CommandError;

use crate::document::{Document, Units};
use crate::object::{ObjectId, ObjectKind};
use crate::raster_view::RasterView;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub ids: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    explicit_key_id: Option<ObjectId>,
}

impl Selection {
    pub fn from_ids(ids: Vec<ObjectId>) -> Self {
        Self { ids, ..Self::default() }
    }

    pub fn query(&self) -> &[ObjectId] {
        &self.ids
    }

    pub fn key(&self) -> Option<ObjectId> {
        self.ids.first().copied()
    }

    pub fn explicit_key(&self) -> Option<ObjectId> {
        self.explicit_key_id.filter(|id| self.ids.len() > 1 && self.key() == Some(*id))
    }

    pub fn set_key(&mut self, id: ObjectId) -> bool {
        let Some(index) = self.ids.iter().position(|known| *known == id) else { return false };
        let key = self.ids.remove(index);
        self.ids.insert(0, key);
        self.explicit_key_id = (self.ids.len() > 1).then_some(key);
        true
    }

    pub fn set(&mut self, ids: Vec<ObjectId>, op: SelectOp) {
        match op {
            SelectOp::Replace => {
                self.ids = ids;
                self.explicit_key_id = None;
            }
            SelectOp::Add => {
                for id in ids {
                    if !self.ids.contains(&id) {
                        self.ids.push(id);
                    }
                }
            }
            SelectOp::Subtract => {
                self.ids.retain(|id| !ids.contains(id));
            }
            SelectOp::Intersect => {
                self.ids.retain(|id| ids.contains(id));
            }
        }
        self.explicit_key_id = self.explicit_key();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MarqueeMode {
    Touch,
    Enclosed,
}

impl Default for MarqueeMode {
    fn default() -> Self {
        MarqueeMode::Touch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SelectOp {
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl Default for SelectOp {
    fn default() -> Self {
        SelectOp::Replace
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CycleDirection {
    Next,
    Previous,
}

pub struct MarqueeArgs {
    pub rect: [f32; 4],
    pub mode: MarqueeMode,
    pub op: SelectOp,
}

fn iter_objects(doc: &Document) -> impl Iterator<Item = &crate::object::Object> {
    doc.objects.iter()
}

pub fn selection_marquee(
    doc: &Document,
    rect: [f32; 4],
    mode: MarqueeMode,
    op: SelectOp,
    current: &Selection,
) -> Selection {
    selection_marquee_with_flags(doc, rect, mode, op, current, &Default::default())
}

pub fn selection_marquee_with_flags(
    doc: &Document,
    [rx, ry, rw, rh]: [f32; 4],
    mode: MarqueeMode,
    op: SelectOp,
    current: &Selection,
    flags: &crate::CanvasFlags,
) -> Selection {
    use crate::geometry::{clip_convex_polygon, object_world_quad, rect_quad, IDENTITY};
    let right = rx + rw;
    let bottom = ry + rh;
    let marquee = rect_quad([rx, ry, rw, rh], &IDENTITY);
    let clip_to_artboards = !matches!(flags.view_mode, crate::ViewMode::Outline | crate::ViewMode::Wireframe);

    let mut matched = Vec::new();
    for obj in iter_objects(doc) {
        let quad = object_world_quad(obj);
        let intersects = |[left, top, end_x, end_y]: [f32; 4]| {
            left < right && end_x > rx && top < bottom && end_y > ry
        };
        let Some(bounds) = crate::geometry::points_bounds(&quad) else { continue };
        let [left, top, end_x, end_y] = bounds;
        if left > right || end_x < rx || top > bottom || end_y < ry
            || !crate::scene_hit::canvas_candidate(doc, obj)
            || !crate::scene::is_drawn_with_flags(doc, flags, obj.id)
        {
            continue;
        }
        let mut visible = quad.to_vec();
        if clip_to_artboards {
            let mut ancestor = obj.parent_id;
            for _ in 0..doc.objects.len() {
                let Some(parent) = ancestor.and_then(|id| doc.get_object(id)) else { break };
                if crate::artboard::is_artboard(parent) {
                    visible = clip_convex_polygon(visible, &object_world_quad(parent));
                    if visible.is_empty() {
                        break;
                    }
                }
                ancestor = parent.parent_id;
            }
        }
        if visible.is_empty() {
            continue;
        }
        let hit = match mode {
            MarqueeMode::Touch => crate::geometry::points_bounds(&visible).is_some_and(intersects)
                && !clip_convex_polygon(visible, &marquee).is_empty(),
            MarqueeMode::Enclosed => visible.iter().all(|&[x, y]| x >= rx && x <= right && y >= ry && y <= bottom),
        };
        if hit {
            matched.push(obj.id);
        }
    }

    let mut result = current.clone();
    result.set(matched, op);
    result
}

pub fn selection_hit(
    doc: &Document,
    point: [f32; 2],
    cycle: bool,
    current: &Selection,
) -> Selection {
    let px = point[0];
    let py = point[1];

    let mut candidates: Vec<ObjectId> = Vec::new();

    for obj in iter_objects(doc) {
        if !crate::scene_hit::canvas_candidate(doc, obj) {
            continue;
        }

        let ox = obj.pos[0];
        let oy = obj.pos[1];
        let ow = obj.size[0];
        let oh = obj.size[1];

        if px >= ox && px <= ox + ow && py >= oy && py <= oy + oh {
            candidates.push(obj.id);
        }
    }

    if candidates.is_empty() {
        return Selection::default();
    }

    if cycle {
        let current_ids: Vec<ObjectId> = current.ids.clone();

        let mut next_candidates: Vec<ObjectId> = Vec::new();

        for &id in &candidates {
            if !current_ids.contains(&id) {
                next_candidates.push(id);
            }
        }

        if !next_candidates.is_empty() {
            return Selection::from_ids(vec![next_candidates[0]]);
        }

        if candidates.len() > 1 {
            let first_id = candidates[0];
            for &id in &candidates[1..] {
                if !current_ids.contains(&id) {
                    return Selection::from_ids(vec![id]);
                }
            }
            return Selection::from_ids(vec![first_id]);
        }

        return Selection::from_ids(vec![candidates[0]]);
    }

    let topmost = candidates.last().copied();

    if let Some(id) = topmost {
        return Selection::from_ids(vec![id]);
    }

    Selection::default()
}

pub fn selection_enter(
    doc: &Document,
    current: &Selection,
) -> Selection {
    if current.ids.len() != 1 {
        return current.clone();
    }

    let group_id = current.ids[0];

    let group_obj = match doc.get_object(group_id) {
        Some(obj) => obj,
        None => return current.clone(),
    };

    if !matches!(group_obj.kind, ObjectKind::Group | ObjectKind::Text(_)) {
        return current.clone();
    }

    let mut child_ids: Vec<ObjectId> = Vec::new();

    for obj in iter_objects(doc) {
        if obj.parent_id == Some(group_id) && obj.pickable && !obj.locked {
            child_ids.push(obj.id);
        }
    }

    Selection::from_ids(child_ids)
}

pub fn selection_exit(
    doc: &Document,
    current: &Selection,
) -> Selection {
    if current.ids.is_empty() {
        return current.clone();
    }

    let first_id = current.ids[0];

    let obj = match doc.get_object(first_id) {
        Some(obj) => obj,
        None => return current.clone(),
    };

    if let Some(parent_id) = obj.parent_id {
        if let Some(parent_obj) = doc.get_object(parent_id) {
            if matches!(parent_obj.kind, ObjectKind::Group | ObjectKind::Text(_)) {
                return Selection::from_ids(vec![parent_id]);
            }
        }
    }

    current.clone()
}

pub fn selection_cycle(
    doc: &Document,
    direction: CycleDirection,
    current: &Selection,
) -> Selection {
    let mut eligible_ids: Vec<ObjectId> = Vec::new();

    for obj in iter_objects(doc) {
        if !obj.pickable || obj.locked {
            continue;
        }

        if !current.ids.is_empty() {
            let first_id = current.ids[0];
            if let Some(first_obj) = doc.get_object(first_id) {
                if first_obj.parent_id == Some(obj.id) {
                    continue;
                }

                if obj.parent_id != first_obj.parent_id {
                    continue;
                }
            }
        }

        eligible_ids.push(obj.id);
    }

    if eligible_ids.is_empty() {
        return current.clone();
    }

    let current_index = if !current.ids.is_empty() {
        let first_id = current.ids[0];
        eligible_ids.iter().position(|&id| id == first_id)
    } else {
        None
    };

    let next_index = match direction {
        CycleDirection::Next => {
            if let Some(idx) = current_index {
                (idx + 1) % eligible_ids.len()
            } else {
                0
            }
        }
        CycleDirection::Previous => {
            if let Some(idx) = current_index {
                if idx == 0 {
                    eligible_ids.len() - 1
                } else {
                    idx - 1
                }
            } else {
                eligible_ids.len() - 1
            }
        }
    };

    Selection::from_ids(vec![eligible_ids[next_index]])
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PixelSelection {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<crate::document::PageId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    outside_canvas: Vec<OutsideSelectionRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OutsideSelectionRow {
    x: i64,
    y: i64,
    coverage: Vec<u8>,
}

impl PixelSelection {
    pub fn from_coverage(width: u32, height: u32, coverage: Vec<u8>) -> Self {
        Self { width, height, coverage, page_id: None, outside_canvas: Vec::new() }
    }

    pub fn empty(width: u32, height: u32) -> Self {
        Self::from_coverage(width, height, vec![0; width as usize * height as usize])
    }

    pub fn all(width: u32, height: u32) -> Self {
        Self::from_coverage(width, height, vec![255; width as usize * height as usize])
    }

    pub(crate) fn for_resized_canvas(
        &self, width: u32, height: u32, [dx, dy]: [i64; 2],
    ) -> Result<Self, CommandError> {
        // The public mask remains canvas-sized. Only the portions clipped by
        // resize are stored separately so another resize can expose them again.
        let bytes = u64::from(width) * u64::from(height);
        if bytes > 128 * 1024 * 1024 {
            return Err(CommandError::new("REQUEST_TOO_LARGE"));
        }
        let mut coverage = Vec::new();
        coverage.try_reserve_exact(bytes as usize).map_err(|_| CommandError::new("REQUEST_TOO_LARGE"))?;
        coverage.resize(bytes as usize, 0);
        let mut resized = Self::from_coverage(width, height, coverage);
        resized.page_id = self.page_id;

        for row in &self.outside_canvas {
            let x = row.x.checked_add(dx).ok_or_else(CommandError::invalid_params)?;
            let y = row.y.checked_add(dy).ok_or_else(CommandError::invalid_params)?;
            resized.project_row(x, y, &row.coverage);
        }
        if self.width > 0 {
            for (y, row) in self.coverage.chunks(self.width as usize).take(self.height as usize).enumerate() {
                resized.project_row(dx, y as i64 + dy, row);
            }
        }
        Ok(resized)
    }

    pub(crate) fn magnify(&self, factor: u32) -> Result<Self, CommandError> {
        let [width, height] = crate::pixel_art::dimensions([self.width, self.height], factor)?;
        let mut next = Self::empty(width, height);
        next.page_id = self.page_id;
        let scale = factor as usize;
        for (y, row) in self.coverage.chunks(self.width as usize).take(self.height as usize).enumerate() {
            let start = y * scale * width as usize;
            let (first, rest) = next.coverage[start..start + scale * width as usize].split_at_mut(width as usize);
            for (pixel, value) in first.chunks_exact_mut(scale).zip(row) { pixel.fill(*value); }
            for row in rest.chunks_exact_mut(width as usize) { row.copy_from_slice(first); }
        }
        for row in &self.outside_canvas {
            let factor = i64::from(factor);
            let x = row.x.checked_mul(factor).ok_or_else(CommandError::invalid_params)?;
            let y = row.y.checked_mul(factor).ok_or_else(CommandError::invalid_params)?;
            let coverage: Vec<u8> = row.coverage.iter().flat_map(|value| std::iter::repeat_n(*value, scale)).collect();
            for offset in 0..factor {
                next.project_row(x, y.checked_add(offset).ok_or_else(CommandError::invalid_params)?, &coverage);
            }
        }
        Ok(next)
    }

    fn project_row(&mut self, x: i64, y: i64, row: &[u8]) {
        if !(0..i64::from(self.height)).contains(&y) {
            self.keep_outside(x, y, row);
            return;
        }

        let left = x.saturating_neg().clamp(0, row.len() as i64) as usize;
        let right = i64::from(self.width).saturating_sub(x).clamp(left as i64, row.len() as i64) as usize;
        if left < right {
            let start = y as usize * self.width as usize + (x + left as i64) as usize;
            self.coverage[start..start + right - left].copy_from_slice(&row[left..right]);
        }
        self.keep_outside(x, y, &row[..left]);
        self.keep_outside(x.saturating_add(right as i64), y, &row[right..]);
    }

    fn keep_outside(&mut self, x: i64, y: i64, row: &[u8]) {
        let Some(first) = row.iter().position(|value| *value != 0) else { return; };
        let last = row.iter().rposition(|value| *value != 0).unwrap_or(first);
        self.outside_canvas.push(OutsideSelectionRow {
            x: x.saturating_add(first as i64),
            y,
            coverage: row[first..=last].to_vec(),
        });
    }

    pub(crate) fn clear_outside_canvas(&mut self) {
        self.outside_canvas.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.coverage.iter().all(|c| *c == 0)
    }

    pub fn get(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.coverage
            .get(y as usize * self.width as usize + x as usize)
            .copied()
            .unwrap_or(0)
    }

    pub fn bounds(&self) -> Option<[u32; 4]> {
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut found = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) > 0 {
                    found = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if !found {
            return None;
        }
        Some([min_x, min_y, max_x - min_x + 1, max_y - min_y + 1])
    }

    pub fn area(&self) -> f64 {
        self.coverage.iter().map(|c| *c as f64).sum::<f64>() / 255.0
    }

    pub fn combine(&mut self, other: &PixelSelection, op: SelectionOp) {
        if op == SelectionOp::New {
            self.clone_from(other);
            return;
        }
        if op == SelectionOp::Intersect {
            self.clear_outside_canvas();
        }
        if self.width != other.width || self.height != other.height {
            match op {
                SelectionOp::New => {
                    *self = other.clone();
                }
                SelectionOp::Add => {
                    let mut new_coverage = vec![0u8; (other.width as usize) * (other.height as usize)];
                    for y in 0..other.height {
                        for x in 0..other.width {
                            let other_val = other.get(x, y);
                            let self_val = if x < self.width && y < self.height {
                                self.get(x, y)
                            } else {
                                0
                            };
                            new_coverage[y as usize * other.width as usize + x as usize] = self_val.max(other_val);
                        }
                    }
                    self.width = other.width;
                    self.height = other.height;
                    self.coverage = new_coverage;
                    self.clear_outside_canvas();
                }
                SelectionOp::Subtract | SelectionOp::Intersect => {
                    *self = PixelSelection::empty(other.width, other.height);
                }
            }
            return;
        }

        for (a, b) in self.coverage.iter_mut().zip(other.coverage.iter()) {
            *a = match op {
                SelectionOp::New => *b,
                SelectionOp::Add => (*a).max(*b),
                SelectionOp::Subtract => (*a).min(255 - *b),
                SelectionOp::Intersect => (*a).min(*b),
            };
        }
    }

    pub fn invert(&mut self) {
        self.clear_outside_canvas();
        for c in self.coverage.iter_mut() {
            *c = 255 - *c;
        }
    }

    pub fn grow(&mut self, radius: f32, circular: bool) {
        self.change_extent(radius, circular, true);
    }

    pub fn shrink(&mut self, radius: f32, circular: bool) {
        self.change_extent(radius, circular, false);
    }

    fn change_extent(&mut self, radius: f32, circular: bool, grow: bool) {
        if !radius.is_finite() || radius < 1.0 || self.coverage.is_empty() { return; }
        let size = [self.width as usize, self.height as usize];
        if size[0].checked_mul(size[1]) != Some(self.coverage.len()) { return; }
        self.coverage = morphology::extrema(&self.coverage, size, radius, circular, grow);
        self.clear_outside_canvas();
    }

    pub fn feather(&mut self, radius: f32) {
        if !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let sigma = radius / 2.0;
        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 {
            return;
        }

        let kernel_radius = (sigma * 3.0).ceil() as i32;
        let kernel_size = (kernel_radius * 2 + 1) as usize;
        
        let mut kernel = vec![0.0f64; kernel_size];
        let mut sum = 0.0f64;
        for i in 0..kernel_size {
            let x = i as f64 - kernel_radius as f64;
            let sigma_f64 = sigma as f64;
            let val = (-x * x / (2.0 * sigma_f64 * sigma_f64)).exp();
            kernel[i] = val;
            sum += val;
        }
        for k in kernel.iter_mut() {
            *k /= sum;
        }

        let mut temp1 = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0f64;
                for k_idx in 0..kernel_size {
                    let src_x = (x as i32 + k_idx as i32 - kernel_radius).max(0).min(w as i32 - 1) as usize;
                    acc += self.coverage[y * w + src_x] as f64 * kernel[k_idx];
                }
                temp1[y * w + x] = (acc.round().clamp(0.0, 255.0)) as u8;
            }
        }

        let mut temp2 = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0f64;
                for k_idx in 0..kernel_size {
                    let src_y = (y as i32 + k_idx as i32 - kernel_radius).max(0).min(h as i32 - 1) as usize;
                    acc += temp1[src_y * w + x] as f64 * kernel[k_idx];
                }
                temp2[y * w + x] = (acc.round().clamp(0.0, 255.0)) as u8;
            }
        }

        self.clear_outside_canvas();
        self.coverage = temp2;
    }

    pub fn smooth(&mut self, radius: f32) {
        if !radius.is_finite() || radius <= 0.0 {
            return;
        }
        
        self.feather(radius);

        for c in self.coverage.iter_mut() {
            *c = ((u16::from(*c).saturating_sub(96) * 255 + 32) / 64).min(255) as u8;
        }
    }

    pub fn outline(&mut self, width: f32, align: OutlineAlign) {
        if !width.is_finite() || width <= 0.0 || self.coverage.is_empty() { return; }
        let size = [self.width as usize, self.height as usize];
        if size[0].checked_mul(size[1]) != Some(self.coverage.len()) { return; }
        let (outside, inside) = match align {
            OutlineAlign::Outside => (width, 0.0),
            OutlineAlign::Centre => (width / 2.0, width / 2.0),
            OutlineAlign::Inside => (0.0, width),
        };
        let outline_edge = |radius: f32, grow| {
            let mut coverage = morphology::extrema(&self.coverage, size, radius.floor(), true, grow);
            let fraction = radius.fract();
            if fraction > 0.0 {
                let next = morphology::extrema(&self.coverage, size, radius.ceil(), true, grow);
                for (value, next) in coverage.iter_mut().zip(next) {
                    *value = (f32::from(*value) * (1.0 - fraction) + f32::from(next) * fraction).round() as u8;
                }
            }
            coverage
        };
        let outer = outline_edge(outside, true);
        let inner = outline_edge(inside, false);
        self.coverage = outer.into_iter().zip(inner).map(|(a, b)| a.saturating_sub(b)).collect();
        self.clear_outside_canvas();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionOp {
    #[default]
    New,
    Add,
    Subtract,
    Intersect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutlineAlign {
    Outside,
    Centre,
    Inside,
}

impl Default for OutlineAlign {
    fn default() -> Self {
        OutlineAlign::Centre
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefinePreview {
    Overlay,
    BlackMatte,
    WhiteMatte,
    BlackAndWhite,
    Transparent,
}

impl Default for RefinePreview {
    fn default() -> Self {
        RefinePreview::Overlay
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefineOutput {
    Selection,
    Mask,
    NewLayer,
    NewLayerWithMask,
}

impl Default for RefineOutput {
    fn default() -> Self {
        RefineOutput::Selection
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RefineSettings {
    pub border_width: f32,
    pub smooth: f32,
    pub feather: f32,
    pub ramp: f32,
    pub decontaminate: bool,
    pub preview: RefinePreview,
    pub output: RefineOutput,
}

impl Default for RefineSettings {
    fn default() -> Self {
        Self {
            border_width: 0.0,
            smooth: 0.0,
            feather: 0.0,
            ramp: 1.0,
            decontaminate: false,
            preview: RefinePreview::Overlay,
            output: RefineOutput::Selection,
        }
    }
}

pub fn length_error() -> CommandError {
    crate::units::length_error()
}

pub fn unit_to_px(value: f32, unit: &str, dpi: f32) -> Option<f32> {
    let token = if unit.is_empty() { crate::units::UnitToken::Unit(Units::Px) } else { crate::units::parse_unit(unit)? };
    crate::units::token_to_px(value, token, dpi)
}

pub fn parse_length(v: &serde_json::Value, dpi: f32) -> Result<f32, CommandError> {
    if !(dpi.is_finite() && dpi > 0.0) {
        return Err(length_error());
    }
    crate::units::parse_length_value(v, dpi)
}

pub fn document_px_size(doc: &Document) -> Option<(u32, u32)> {
    let [w, h] = PixelSurface::for_page(doc, None)?.size;
    Some((w, h))
}

#[derive(Clone, Copy, Debug)]
pub struct PixelSurface {
    pub page_id: crate::document::PageId,
    pub bounds: [f32; 4],
    pub size: [u32; 2],
    pub scale: f32,
}

impl PixelSurface {
    pub fn for_page(doc: &Document, page_id: Option<crate::document::PageId>) -> Option<Self> {
        let index = match page_id {
            Some(id) => doc.pages.iter().position(|page| page.id == id)?,
            None => 0,
        };
        let page = doc.pages.get(index)?;
        let [x, y] = crate::data_merge::layout::page_origin(doc, index);
        let scale = crate::units::to_px(1.0, doc.units, doc.dpi)?;
        let [w, h] = page.size;
        if ![x, y, w, h, scale].into_iter().all(f32::is_finite) || w <= 0.0 || h <= 0.0 || scale <= 0.0 {
            return None;
        }
        let size = [w, h].map(|side| (side * scale).round().max(1.0) as u32);
        Some(Self { page_id: page.id, bounds: [x, y, w, h], size, scale })
    }

    pub fn at_point(doc: &Document, point: [f32; 2]) -> Option<Self> {
        let index = crate::artboard::edit::page_at_point(doc, point)?;
        Self::for_page(doc, Some(doc.pages[index].id))
    }

    pub fn at_view_center(doc: &Document, view: &crate::view_state::ViewState, viewport: [f32; 2]) -> Option<Self> {
        use crate::view::ViewTarget;
        let page_id = match ViewTarget::at_view_center(doc, view, viewport)? {
            ViewTarget::Page(id) => id,
            ViewTarget::Artboard(id) => doc.pages.get(crate::data_merge::layout::object_page_index(doc, id))?.id,
        };
        Self::for_page(doc, Some(page_id))
    }

    pub fn for_command(doc: &Document, params: &serde_json::Value) -> Result<Self, CommandError> {
        let page_id = params.get("page_id").filter(|value| !value.is_null()).map(|value| {
            value.as_u64().map(crate::document::PageId).ok_or_else(CommandError::invalid_params)
        }).transpose()?;
        Self::for_page(doc, page_id).ok_or_else(|| {
            if page_id.is_some() { CommandError::not_found() } else { CommandError::no_document() }
        })
    }

    pub fn doc_to_pixel(self, point: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|axis| (point[axis] - self.bounds[axis]) * self.scale)
    }

    pub fn pixel_to_doc(self, point: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|axis| self.bounds[axis] + point[axis] / self.scale)
    }

    pub fn matches(self, doc: &Document, mask: &PixelSelection) -> bool {
        let page_id = mask.page_id.or_else(|| doc.pages.first().map(|page| page.id));
        page_id == Some(self.page_id) && self.size == [mask.width, mask.height]
    }

    pub fn coverage_at(self, mask: &PixelSelection, point: [f32; 2]) -> u8 {
        let [x, y] = self.doc_to_pixel(point);
        if x >= 0.0 && y >= 0.0 { mask.get(x as u32, y as u32) } else { 0 }
    }
}

pub(crate) fn combine_page_mask(doc: &mut Document, mask: PixelSelection, op: SelectionOp) -> Result<(), CommandError> {
    let surface = PixelSurface::for_page(doc, mask.page_id).ok_or_else(CommandError::no_document)?;
    if [mask.width, mask.height] != surface.size {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    if op != SelectionOp::New && doc.pixel_selection.as_ref().is_some_and(|current| {
        !current.is_empty() && !surface.matches(doc, current)
    }) {
        return Err(CommandError::invalid_params());
    }
    if op == SelectionOp::New {
        doc.pixel_selection = Some(mask);
    } else {
        let mut next = doc.pixel_selection.take().filter(|current| surface.matches(doc, current))
            .unwrap_or_else(|| PixelSelection::empty(mask.width, mask.height));
        next.combine(&mask, op);
        next.page_id = mask.page_id;
        doc.pixel_selection = Some(next);
    }
    Ok(())
}

fn span_overlap(start: f32, end: f32, cell: u32) -> f32 {
    let lo = (cell as f32).max(start);
    let hi = (cell as f32 + 1.0).min(end);
    (hi - lo).max(0.0)
}

pub fn rect_mask(w: u32, h: u32, rect: [f32; 4], antialias: bool) -> PixelSelection {
    let mut sel = PixelSelection::empty(w, h);
    let x0 = rect[0].min(rect[0] + rect[2]);
    let x1 = rect[0].max(rect[0] + rect[2]);
    let y0 = rect[1].min(rect[1] + rect[3]);
    let y1 = rect[1].max(rect[1] + rect[3]);
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) {
        return sel;
    }
    if x1 <= x0 || y1 <= y0 {
        return sel;
    }
    let col_start = x0.floor().max(0.0) as u32;
    let col_end = (x1.ceil().max(0.0) as u32).min(w);
    let row_start = y0.floor().max(0.0) as u32;
    let row_end = (y1.ceil().max(0.0) as u32).min(h);
    for py in row_start..row_end {
        for px in col_start..col_end {
            let value = if antialias {
                let cover = span_overlap(x0, x1, px) * span_overlap(y0, y1, py);
                (cover * 255.0).round().clamp(0.0, 255.0) as u8
            } else {
                let cx = px as f32 + 0.5;
                let cy = py as f32 + 0.5;
                if cx >= x0 && cx < x1 && cy >= y0 && cy < y1 { 255 } else { 0 }
            };
            sel.coverage[py as usize * w as usize + px as usize] = value;
        }
    }
    sel
}

pub fn ellipse_mask(w: u32, h: u32, rect: [f32; 4], antialias: bool) -> PixelSelection {
    let mut sel = PixelSelection::empty(w, h);
    let x0 = rect[0].min(rect[0] + rect[2]);
    let x1 = rect[0].max(rect[0] + rect[2]);
    let y0 = rect[1].min(rect[1] + rect[3]);
    let y1 = rect[1].max(rect[1] + rect[3]);
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) {
        return sel;
    }
    if x1 <= x0 || y1 <= y0 {
        return sel;
    }

    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let rx = (x1 - x0) * 0.5;
    let ry = (y1 - y0) * 0.5;

    if rx <= 0.0 || ry <= 0.0 {
        return sel;
    }

    let col_start = x0.floor().max(0.0) as u32;
    let col_end = (x1.ceil().max(0.0) as u32).min(w);
    let row_start = y0.floor().max(0.0) as u32;
    let row_end = (y1.ceil().max(0.0) as u32).min(h);

    for py in row_start..row_end {
        for px in col_start..col_end {
            let value = if antialias {
                let cover = ellipse_coverage(px, py, cx, cy, rx, ry);
                (cover * 255.0).round().clamp(0.0, 255.0) as u8
            } else {
                let pcx = px as f32 + 0.5;
                let pcy = py as f32 + 0.5;
                let dx = (pcx - cx) / rx;
                let dy = (pcy - cy) / ry;
                if dx * dx + dy * dy <= 1.0 { 255 } else { 0 }
            };
            sel.coverage[py as usize * w as usize + px as usize] = value;
        }
    }

    sel
}

fn ellipse_coverage(px: u32, py: u32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let x0 = px as f32;
    let y0 = py as f32;
    let x1 = x0 + 1.0;
    let y1 = y0 + 1.0;

    let dx0 = (x0 - cx) / rx;
    let dx1 = (x1 - cx) / rx;
    let dy0 = (y0 - cy) / ry;
    let dy1 = (y1 - cy) / ry;

    let min_dx = dx0.min(dx1);
    let max_dx = dx0.max(dx1);
    let min_dy = dy0.min(dy1);
    let max_dy = dy0.max(dy1);

    let far_x = dx0.abs().max(dx1.abs());
    let far_y = dy0.abs().max(dy1.abs());
    let near_x = if min_dx <= 0.0 && max_dx >= 0.0 { 0.0 } else { dx0.abs().min(dx1.abs()) };
    let near_y = if min_dy <= 0.0 && max_dy >= 0.0 { 0.0 } else { dy0.abs().min(dy1.abs()) };
    if far_x * far_x + far_y * far_y <= 1.0 {
        return 1.0;
    }
    if near_x * near_x + near_y * near_y >= 1.0 {
        return 0.0;
    }

    let samples = 4;
    let step_x = (max_dx - min_dx) / samples as f32;
    let step_y = (max_dy - min_dy) / samples as f32;
    if step_x <= 0.0 || step_y <= 0.0 {
        return 0.0;
    }

    let mut count = 0;
    for i in 0..samples {
        for j in 0..samples {
            let sx = min_dx + (i as f32 + 0.5) * step_x;
            let sy = min_dy + (j as f32 + 0.5) * step_y;
            if sx * sx + sy * sy <= 1.0 {
                count += 1;
            }
        }
    }

    count as f32 / (samples * samples) as f32
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LassoMode {
    #[default]
    Freehand,
    Polygonal,
    Magnetic,
}

pub mod magnetic;
pub(crate) mod morphology;
pub mod refinement;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleSource {
    #[default]
    CurrentLayer,
    AllLayers,
}

pub fn polygon_mask(w: u32, h: u32, points: &[[f32; 2]], antialias: bool) -> PixelSelection {
    let Some(length) = (w as usize).checked_mul(h as usize) else {
        return PixelSelection::empty(0, 0);
    };
    let mut coverage = Vec::new();
    if coverage.try_reserve_exact(length).is_err() {
        return PixelSelection::empty(0, 0);
    }
    coverage.resize(length, 0);
    let mut result = PixelSelection::from_coverage(w, h, coverage);
    if points.len() < 3 || points.iter().flatten().any(|value| !value.is_finite()) {
        return result;
    }
    let min_x = points.iter().map(|point| point[0]).fold(f32::INFINITY, f32::min).floor().max(0.0).min(w as f32) as u32;
    let max_x = points.iter().map(|point| point[0]).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0).min(w as f32) as u32;
    let min_y = points.iter().map(|point| point[1]).fold(f32::INFINITY, f32::min).floor().max(0.0).min(h as f32) as u32;
    let max_y = points.iter().map(|point| point[1]).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0).min(h as f32) as u32;
    let samples = if antialias { 4usize } else { 1 };
    for y in min_y..max_y {
        let mut counts = vec![0u16; (max_x - min_x) as usize];
        for sy in 0..samples {
            let sample_y = f64::from(y) + (sy as f64 + 0.5) / samples as f64;
            let mut intersections = Vec::new();
            for index in 0..points.len() {
                let a = points[index];
                let b = points[(index + 1) % points.len()];
                let ay = f64::from(a[1]);
                let by = f64::from(b[1]);
                if (ay <= sample_y && sample_y < by) || (by <= sample_y && sample_y < ay) {
                    let x = f64::from(a[0]) + (sample_y - ay) * (f64::from(b[0]) - f64::from(a[0])) / (by - ay);
                    intersections.push((x, if by > ay { 1i64 } else { -1i64 }));
                }
            }
            intersections.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut edge = 0;
            let mut winding = 0i64;
            for x in min_x..max_x {
                for sx in 0..samples {
                    let sample_x = f64::from(x) + (sx as f64 + 0.5) / samples as f64;
                    while edge < intersections.len() && intersections[edge].0 <= sample_x {
                        winding += intersections[edge].1;
                        edge += 1;
                    }
                    if winding != 0 {
                        counts[(x - min_x) as usize] += 1;
                    }
                }
            }
        }
        let total = (samples * samples) as u16;
        for (index, count) in counts.into_iter().enumerate() {
            result.coverage[y as usize * w as usize + min_x as usize + index] = ((count * 255 + total / 2) / total) as u8;
        }
    }
    result
}

pub fn flood_mask(
    src: &RasterView,
    seed: [u32; 2],
    tolerance: f32,
    contiguous: bool,
    antialias: bool,
) -> PixelSelection {
    let w = src.width;
    let h = src.height;
    let mut sel = PixelSelection::empty(w, h);

    if !src.contains(seed[0], seed[1]) {
        return sel;
    }

    let seed_colour = match src.pixel(seed[0], seed[1]) {
        Some(c) => c,
        None => return sel,
    };

    let tol = tolerance.clamp(0.0, 100.0);
    let max_dist_sq = (tol / 100.0 * 255.0).powi(2);

    if contiguous {
        let mut visited = vec![false; w as usize * h as usize];
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((seed[0], seed[1]));
        visited[seed[1] as usize * w as usize + seed[0] as usize] = true;

        while let Some((x, y)) = queue.pop_front() {
            if let Some(colour) = src.pixel(x, y) {
                let dr = colour[0] as f32 - seed_colour[0] as f32;
                let dg = colour[1] as f32 - seed_colour[1] as f32;
                let db = colour[2] as f32 - seed_colour[2] as f32;
                let da = colour[3] as f32 - seed_colour[3] as f32;
                let dist_sq = dr * dr + dg * dg + db * db + da * da;

                if dist_sq <= max_dist_sq {
                    sel.coverage[y as usize * w as usize + x as usize] = 255;

                    let neighbors = [
                        (x.saturating_sub(1), y),
                        (x + 1, y),
                        (x, y.saturating_sub(1)),
                        (x, y + 1),
                    ];

                    for (nx, ny) in neighbors {
                        if nx < w && ny < h {
                            let idx = ny as usize * w as usize + nx as usize;
                            if !visited[idx] {
                                visited[idx] = true;
                                queue.push_back((nx, ny));
                            }
                        }
                    }
                }
            }
        }
    } else {
        for y in 0..h {
            for x in 0..w {
                if let Some(colour) = src.pixel(x, y) {
                    let dr = colour[0] as f32 - seed_colour[0] as f32;
                    let dg = colour[1] as f32 - seed_colour[1] as f32;
                    let db = colour[2] as f32 - seed_colour[2] as f32;
                    let da = colour[3] as f32 - seed_colour[3] as f32;
                    let dist_sq = dr * dr + dg * dg + db * db + da * da;

                    if dist_sq <= max_dist_sq {
                        sel.coverage[y as usize * w as usize + x as usize] = 255;
                    }
                }
            }
        }
    }

    if antialias {
        let mut new_coverage = vec![0u8; w as usize * h as usize];
        for y in 0..h {
            for x in 0..w {
                if sel.get(x, y) > 0 {
                    new_coverage[y as usize * w as usize + x as usize] = 255;
                } else {
                    let neighbors = [
                        (x.saturating_sub(1), y),
                        (x + 1, y),
                        (x, y.saturating_sub(1)),
                        (x, y + 1),
                    ];
                    let mut edge_count = 0;
                    for (nx, ny) in neighbors {
                        if nx < w && ny < h && sel.get(nx, ny) > 0 {
                            edge_count += 1;
                        }
                    }
                    if edge_count > 0 {
                        new_coverage[y as usize * w as usize + x as usize] = (edge_count * 255 / 4) as u8;
                    }
                }
            }
        }
        sel.coverage = new_coverage;
    }

    sel
}

pub fn colour_mask(src: &RasterView, colour: [u8; 4], tolerance: f32) -> PixelSelection {
    let w = src.width;
    let h = src.height;
    let mut sel = PixelSelection::empty(w, h);

    if colour[3] == 0 {
        return sel;
    }

    let tol = tolerance.clamp(0.0, 100.0);
    let max_dist_sq = (tol / 100.0 * 255.0).powi(2);

    for y in 0..h {
        for x in 0..w {
            if let Some(px_colour) = src.pixel(x, y) {
                if px_colour[3] == 0 {
                    continue;
                }
                let dr = px_colour[0] as f32 - colour[0] as f32;
                let dg = px_colour[1] as f32 - colour[1] as f32;
                let db = px_colour[2] as f32 - colour[2] as f32;
                let dist_sq = dr * dr + dg * dg + db * db;
                if dist_sq <= max_dist_sq {
                    sel.coverage[y as usize * w as usize + x as usize] = px_colour[3];
                }
            }
        }
    }

    sel
}

pub fn brush_mask(w: u32, h: u32, points: &[[f32; 3]], size_px: f32, hardness: f32) -> PixelSelection {
    let mut sel = PixelSelection::empty(w, h);
    if points.is_empty() || size_px <= 0.0 {
        return sel;
    }

    let spacing = (size_px * 0.25).max(0.25);
    let mut dab = |[px, py, pressure]: [f32; 3]| {
        if !px.is_finite() || !py.is_finite() || !pressure.is_finite() || pressure <= 0.0 { return; }
        let radius = size_px * 0.5 * pressure.min(1.0);
        let hard_radius = radius * hardness.clamp(0.0, 1.0);

        let start_x = (px - radius).floor().max(0.0) as u32;
        let end_x = (px + radius).ceil().min(w as f32) as u32;
        let start_y = (py - radius).floor().max(0.0) as u32;
        let end_y = (py + radius).ceil().min(h as f32) as u32;

        for y in start_y..end_y {
            for x in start_x..end_x {
                let dx = x as f32 + 0.5 - px;
                let dy = y as f32 + 0.5 - py;
                let dist = (dx * dx + dy * dy).sqrt();

                if dist <= radius {
                    let coverage = if dist <= hard_radius {
                        255.0
                    } else {
                        let t = (dist - hard_radius) / (radius - hard_radius);
                        255.0 * (1.0 - t)
                    };

                    let val = coverage.round() as u8;
                    sel.coverage[y as usize * w as usize + x as usize] = sel.coverage[y as usize * w as usize + x as usize].max(val);
                }
            }
        }
    };
    dab(points[0]);
    for pair in points.windows(2) {
        let [from, to] = [pair[0], pair[1]];
        let [dx, dy, dp] = std::array::from_fn(|axis| to[axis] - from[axis]);
        let steps = (dx.hypot(dy) / spacing).ceil().clamp(1.0, 65_536.0) as u32;
        for step in 1..steps {
            let ratio = step as f32 / steps as f32;
            dab([from[0] + dx * ratio, from[1] + dy * ratio, from[2] + dp * ratio]);
        }
        dab(to);
    }

    sel
}

pub fn refine(sel: &PixelSelection, src: &RasterView, settings: &RefineSettings) -> PixelSelection {
    refinement::refine(sel, src, settings, &[], 0.0)
}

fn range_mask(src:&RasterView,mut coverage:impl FnMut([u8;4])->u8)->PixelSelection {
    let Some(count)=(src.width as usize).checked_mul(src.height as usize) else {return PixelSelection::empty(0,0);};
    if count.checked_mul(4).is_none_or(|bytes|src.rgba.len()<bytes) {return PixelSelection::empty(0,0);}
    PixelSelection::from_coverage(src.width, src.height, src.rgba.chunks_exact(4).take(count).map(|p| coverage([p[0], p[1], p[2], p[3]])).collect())
}
pub fn hue_range_mask(src:&RasterView,hue:&str)->PixelSelection {
    let center=match hue {"reds"=>Some(0.0),"yellows"=>Some(60.0),"greens"=>Some(120.0),"cyans"=>Some(180.0),"blues"=>Some(240.0),"magentas"=>Some(300.0),_=>None};
    range_mask(src,|[r,g,b,a]| {
        let Some(center)=center else {return 0;};
        let r=f64::from(r)/255.0;let g=f64::from(g)/255.0;let b=f64::from(b)/255.0;
        let high=r.max(g).max(b);let low=r.min(g).min(b);let chroma=high-low;
        if chroma==0.0 || a==0 {return 0;}
        let angle=60.0*if high==r {((g-b)/chroma).rem_euclid(6.0)} else if high==g {(b-r)/chroma+2.0} else {(r-g)/chroma+4.0};
        let distance=(angle-center).abs();let distance=distance.min(360.0-distance);
        let weight=if distance>=60.0 {0.0} else {0.5*(1.0+(std::f64::consts::PI*distance/60.0).cos())};
        (weight*(chroma/high)*f64::from(a)).round().clamp(0.0,255.0) as u8
    })
}
pub fn tonal_range_mask(src:&RasterView,range:&str)->PixelSelection {
    range_mask(src,|[r,g,b,a]| {
        let luminance=(0.2126*f64::from(r)+0.7152*f64::from(g)+0.0722*f64::from(b))/255.0;
        let weight=match range {"shadows"=>(1.0-2.0*luminance).max(0.0),"highlights"=>(2.0*luminance-1.0).max(0.0),"midtones"=>1.0-(2.0*luminance-1.0).abs(),_=>0.0};
        (weight*f64::from(a)).round().clamp(0.0,255.0) as u8
    })
}
pub fn alpha_range_mask(src:&RasterView,range:&str)->PixelSelection {
    range_mask(src,|p|if match range {"transparent"=>p[3]==0,"partial"=>p[3]>0 && p[3]<255,"opaque"=>p[3]==255,_=>false} {255} else {0})
}

pub const SPARE_CHANNELS: &str = "Document.spare_channels";
pub const AFSELECTION_EXT: &str = "afselection";
const AFSELECTION_MAGIC: &[u8; 4] = b"AFSL";
const AFSELECTION_VERSION: u16 = 1;

pub fn default_spare_channel_name(channels: &[(String, PixelSelection)]) -> String {
    let n = channels.len() + 1;
    let numbered = dtool_strings::format("selection.channel_name", &[("n", &n.to_string())]);
    if numbered.contains('{') || numbered == "selection.channel_name" {
        format!("Selection {n}")
    } else {
        numbered
    }
}

pub fn push_spare_channel(
    channels: &mut Vec<(String, PixelSelection)>,
    name: String,
    coverage: PixelSelection,
) {
    let _ = SPARE_CHANNELS;
    channels.push((name, coverage));
}

pub fn spare_channel<'a>(
    channels: &'a [(String, PixelSelection)],
    name: &str,
) -> Result<&'a PixelSelection, CommandError> {
    channels
        .iter()
        .find(|(existing, _)| existing == name)
        .map(|(_, coverage)| coverage)
        .ok_or_else(CommandError::not_found)
}

fn coverage_bytes(sel: &PixelSelection) -> Vec<u8> {
    let expected = (sel.width as usize).saturating_mul(sel.height as usize);
    let mut coverage = sel.coverage.clone();
    if coverage.len() < expected {
        coverage.resize(expected, 0);
    } else if coverage.len() > expected {
        coverage.truncate(expected);
    }
    coverage
}

pub fn encode_afselection(sel: &PixelSelection) -> Vec<u8> {
    let coverage = coverage_bytes(sel);
    let mut out = Vec::with_capacity(14 + coverage.len());
    out.extend_from_slice(AFSELECTION_MAGIC);
    out.extend_from_slice(&AFSELECTION_VERSION.to_le_bytes());
    out.extend_from_slice(&sel.width.to_le_bytes());
    out.extend_from_slice(&sel.height.to_le_bytes());
    out.extend_from_slice(&coverage);
    out
}

pub fn decode_afselection(bytes: &[u8]) -> Result<PixelSelection, CommandError> {
    if bytes.len() < 14 || &bytes[0..4] != AFSELECTION_MAGIC {
        return Err(CommandError::new("IO.INVALID_FORMAT"));
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != AFSELECTION_VERSION {
        return Err(CommandError::new("IO.INVALID_FORMAT"));
    }
    let width = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
    let height = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
    let expected = (width as usize).checked_mul(height as usize).ok_or_else(|| CommandError::new("IO.INVALID_FORMAT"))?;
    if bytes.len() != 14 + expected {
        return Err(CommandError::new("IO.INVALID_FORMAT"));
    }
    Ok(PixelSelection::from_coverage(width, height, bytes[14..].to_vec()))
}

pub fn with_afselection_extension(path: &std::path::Path) -> std::path::PathBuf {
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case(AFSELECTION_EXT)).unwrap_or(false) {
        path.to_path_buf()
    } else if path.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        path.with_extension(AFSELECTION_EXT)
    }
}

fn map_selection_io(err: std::io::Error) -> CommandError {
    let code = match err.kind() {
        std::io::ErrorKind::NotFound => "IO.NOT_FOUND",
        std::io::ErrorKind::PermissionDenied => "IO.PERMISSION",
        std::io::ErrorKind::InvalidData => "IO.INVALID_FORMAT",
        _ => "IO.OTHER",
    };
    CommandError::new(code)
}

pub fn write_afselection_file(path: &std::path::Path, sel: &PixelSelection) -> Result<std::path::PathBuf, CommandError> {
    let path = with_afselection_extension(path);
    if path.as_os_str().is_empty() {
        return Err(CommandError::invalid_params());
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(map_selection_io)?;
        }
    }
    std::fs::write(&path, encode_afselection(sel)).map_err(map_selection_io)?;
    Ok(path)
}

pub fn read_afselection_file(path: &std::path::Path) -> Result<PixelSelection, CommandError> {
    if path.as_os_str().is_empty() {
        return Err(CommandError::invalid_params());
    }
    let bytes = std::fs::read(path).map_err(map_selection_io)?;
    decode_afselection(&bytes)
}

pub fn selection_size_matches_document(doc: &Document, sel: &PixelSelection) -> bool {
    match PixelSurface::for_page(doc, sel.page_id).map(|surface| (surface.size[0], surface.size[1])) {
        Some((width, height)) => sel.width == width && sel.height == height,
        None => false,
    }
}

#[cfg(test)]
mod range_mask_contracts {
    use super::*;
    #[test]
    fn hue_falloff_respects_hue_saturation_and_alpha() {
        let bytes=[255,0,0,255,0,0,255,255,255,128,0,255,128,128,128,255,255,0,0,0];
        let mask=hue_range_mask(&RasterView::new(5,1,&bytes),"reds");
        assert_eq!(mask.coverage[0],255);assert_eq!(mask.coverage[1],0);assert!((1..255).contains(&mask.coverage[2]));assert_eq!(&mask.coverage[3..],&[0,0]);
    }
    #[test]
    fn alpha_classes_partition_soft_values_exactly() {
        let bytes=[0,0,0,0,0,0,0,1,0,0,0,128,0,0,0,254,0,0,0,255];let src=RasterView::new(5,1,&bytes);
        assert_eq!(alpha_range_mask(&src,"transparent").coverage,vec![255,0,0,0,0]);
        assert_eq!(alpha_range_mask(&src,"partial").coverage,vec![0,255,255,255,0]);
        assert_eq!(alpha_range_mask(&src,"opaque").coverage,vec![0,0,0,0,255]);
    }
    #[test]
    fn tonal_endpoints_and_midtone_are_not_binary_thresholds() {
        let bytes=[0,0,0,255,128,128,128,255,255,255,255,255];let src=RasterView::new(3,1,&bytes);
        assert_eq!(tonal_range_mask(&src,"shadows").coverage[0],255);assert_eq!(tonal_range_mask(&src,"highlights").coverage[2],255);
        assert!(tonal_range_mask(&src,"midtones").coverage[1]>250);
    }
}
