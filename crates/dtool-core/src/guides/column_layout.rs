use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    #[default]
    Filled,
    Outline,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColumnLayout {
    pub columns: u16,
    pub rows: u16,
    pub gutter: f32,
    pub style: Style,
}

impl Default for ColumnLayout {
    fn default() -> Self {
        Self { columns: 1, rows: 1, gutter: 0.0, style: Style::Filled }
    }
}

impl ColumnLayout {
    pub const MAX_COUNT: u16 = 64;

    pub fn valid(self) -> bool {
        (1..=Self::MAX_COUNT).contains(&self.columns) && (1..=Self::MAX_COUNT).contains(&self.rows)
            && self.gutter.is_finite() && self.gutter >= 0.0
    }

    pub fn geometry(self, [x, y, width, height]: [f32; 4], doc: &crate::Document) -> Option<Geometry> {
        if !self.valid() || ![x, y, width, height].iter().all(|value| value.is_finite()) {
            return None;
        }
        let margins = doc.setup.margins;
        let [left, top, right, bottom] = if margins.enabled {
            [margins.left, margins.top, margins.right, margins.bottom]
        } else { [0.0; 4] };
        let [w, h] = [width - left - right, height - top - bottom];
        let dims = [w, h];
        let counts = [self.columns, self.rows];
        let starts = [x + left, y + top];
        let axes = std::array::from_fn(|i| {
            let count = counts[i];
            let size = (dims[i] - f32::from(count - 1) * self.gutter) / f32::from(count);
            Axis { start: starts[i], size, pitch: size + self.gutter, count }
        });
        if !axes.iter().all(|axis| axis.start.is_finite() && axis.size > 0.0 && axis.pitch.is_finite()) {
            return None;
        }
        Some(Geometry { axes, bounds: [starts[0], starts[1], w, h] })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Axis {
    start: f32,
    size: f32,
    pitch: f32,
    count: u16,
}

impl Axis {
    pub fn spans(self) -> impl Iterator<Item = [f32; 2]> {
        (0..self.count).map(move |index| {
            let start = self.start + f32::from(index) * self.pitch;
            [start, start + self.size]
        })
    }
}

pub struct Geometry {
    pub axes: [Axis; 2],
    pub bounds: [f32; 4],
}

impl Geometry {
    pub fn boundaries(&self, axis: usize) -> impl Iterator<Item = f32> + '_ {
        let track = self.axes[axis];
        (1..track.count).flat_map(move |index| {
            let next = track.start + f32::from(index) * track.pitch;
            let edge = next - (track.pitch - track.size);
            [Some(edge), (edge != next).then_some(next)].into_iter().flatten()
        })
    }
}

pub fn layouts(doc: &crate::Document) -> impl Iterator<Item = (Style, Geometry)> + '_ {
    let active = |page: &crate::document::Page| page.column_guides.is_some_and(|grid| grid.rows > 1 || grid.columns > 1);
    let rects = doc.pages.iter().any(active).then(|| crate::scene::page_rects(doc));
    doc.pages.iter().zip(rects.into_iter().flatten()).filter(move |(page, _)| active(page)).filter_map(move |(page, rect)| {
        let grid = page.column_guides?;
        Some((grid.style, grid.geometry(rect, doc)?))
    })
}
