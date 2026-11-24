use super::model::{BaselineOrigin, TableFrameFormat};
use crate::{Document, Object};
use crate::data_merge::layout::{object_page_index, page_origin, spread_ranges};

pub struct Guide {
    pub start: f32,
    pub spacing: f32,
    pub color: String,
}

pub fn guide(doc: &Document, frame: &TableFrameFormat, object: Option<&Object>) -> Option<Guide> {
    let scale = crate::units::units_per_point(doc.units, doc.dpi);
    let (start, spacing, origin, color) = if let Some(grid) = frame.baseline.as_ref().filter(|grid| grid.enabled) {
        (grid.start_pt * scale, grid.spacing_pt * scale, Some(grid.relative_to), grid.color.clone())
    } else if !frame.ignore_baseline_grid {
        let index = object.map_or(0, |object| object_page_index(doc, object.id));
        let start = crate::grid::baseline::start_for_page(doc, index)?;
        (start, doc.grid.baseline_spacing, None, super::model::BaselineGrid::default().color)
    } else {
        return None;
    };
    if !start.is_finite() || !spacing.is_finite() || spacing <= 0.0 { return None; }
    if origin == Some(BaselineOrigin::Frame) {
        return Some(Guide { start, spacing, color });
    }
    let (top, y_scale) = if let Some(object) = object {
        let matrix = crate::geometry::object_matrix(object);
        if matrix[1].abs() > 1e-5 || matrix[3] <= 1e-5 { return None; }
        (crate::geometry::apply_matrix(&matrix, object.pos)[1], matrix[3])
    } else { (0.0, 1.0) };
    let page_top = if matches!(origin, Some(BaselineOrigin::Page | BaselineOrigin::Spread)) {
        let index = object.map_or(0, |object| object_page_index(doc, object.id));
        if origin == Some(BaselineOrigin::Spread)
            && let Some(spread) = spread_ranges(doc).into_iter().find(|range| range.contains(&index))
        {
            spread.map(|page| page_origin(doc, page)[1]).fold(f32::INFINITY, f32::min)
        } else {
            page_origin(doc, index)[1]
        }
    } else { 0.0 };
    Some(Guide { start: (page_top + start - top) / y_scale, spacing: spacing / y_scale, color })
}
