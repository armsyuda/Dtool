use dtool_core::document::Document;
use dtool_core::table::model::{CellFormat, CellId, CellRotation, InitialAdvance, TableNode, VerticalAlign};
use dtool_core::table::geometry::TableGrid;
use dtool_core::table::baseline::Guide;
use dtool_core::table::resolve::{resolve_cell, resolve_frame, text_insets};
use crate::fonts::FontLibrary;
use crate::metrics::{family_metrics, FontMetrics};
use crate::shape::{GlyphTransform, ShapedRun};
use crate::story::{CharStyle, Line, ParaStyle, Run, TextRange};
use dtool_core::paint::Paint;

pub struct TableLayout {
    pub generation: u64,
    pub cells: Vec<CellLayout>,
}

pub struct CellLayout {
    pub id: CellId,
    pub content_rect: [f32; 4],
    pub lines: Vec<ShapedRun>,
    pub ranges: Vec<[usize; 2]>,
    pub baselines: Vec<f32>,
    pub overflow: bool,
    pub rotation: CellRotation,
}

pub fn rotation_degrees(rotation: CellRotation) -> f32 {
    match rotation {
        CellRotation::R0 => 0.0,
        CellRotation::R90 => 90.0,
        CellRotation::R180 => 180.0,
        CellRotation::R270 => 270.0,
    }
}

pub fn aligned_baselines(baselines: &[f32], block_height: f32, available: f32, align: VerticalAlign) -> Vec<f32> {
    let free = available - block_height;
    if baselines.is_empty() || !free.is_finite() || free <= 0.0 {
        return baselines.to_vec();
    }
    match align {
        VerticalAlign::Top => baselines.to_vec(),
        VerticalAlign::Center => baselines.iter().map(|b| b + free * 0.5).collect(),
        VerticalAlign::Bottom => baselines.iter().map(|b| b + free).collect(),
        VerticalAlign::Justify if baselines.len() > 1 => {
            let step = free / (baselines.len() - 1) as f32;
            baselines.iter().enumerate().map(|(i, b)| b + step * i as f32).collect()
        }
        VerticalAlign::Justify => baselines.to_vec(),
    }
}

fn style_metrics(lib: &FontLibrary, style: &CharStyle) -> FontMetrics {
    let metrics = family_metrics(lib, &style.family, &style.style, style.size_pt).metrics;
    if metrics.ascent_pt.is_finite() && metrics.descent_pt.is_finite() && metrics.ascent_pt + metrics.descent_pt > 0.0 {
        metrics
    } else {
        FontMetrics::approximate(style.size_pt)
    }
}

fn line_run(line: Line, metrics: &FontMetrics) -> ShapedRun {
    ShapedRun {
        glyphs: line.glyphs,
        width: line.width,
        ascent: metrics.ascent_pt,
        descent: metrics.descent_pt,
        language: None,
        features: Vec::new(),
        transform: GlyphTransform::default(),
    }
}

fn cell_story(doc: &Document, table: &TableNode, core: &dtool_core::Story, format: &CellFormat) -> (crate::story::Story, CharStyle) {
    let preview = crate::font_preview::previewed_core_story(doc, core);
    let core = preview.as_ref().unwrap_or(core);
    let styled = dtool_core::table::resolve::styled_cell_story(doc, table, core, format);
    let mut story = crate::bridge::story_in_document(styled.as_ref().unwrap_or(core), doc);
    let len = story.text.chars().count();
    // Legacy point-unit cells without character runs already use one-em line spacing.
    if len > 0 && story.chars.is_empty() {
        story.paras.insert(0, Run {
            range: TextRange::new(0, len + 1),
            style: ParaStyle { leading: 1.0, ..Default::default() },
        });
    }
    if len > 0 && !story.chars.iter().any(|run| run.range.start == 0 && run.range.end >= len) {
        story.chars.insert(0, Run {
            range: TextRange::new(0, len),
            style: CharStyle { family: "sans-serif".into(), ..Default::default() },
        });
    }
    let style = story.chars.iter().rfind(|run| run.range.start == 0).map(|run| run.style.clone())
        .unwrap_or_else(|| default_style(doc));
    (story, style)
}

fn default_style(doc: &Document) -> CharStyle {
    CharStyle {
        size_pt: dtool_core::text_cmd::DEFAULT_SIZE_PT * dtool_core::units::units_per_point(doc.units, doc.dpi),
        ..Default::default()
    }
}

fn initial_advance(doc: &Document, format: &CellFormat, style: &CharStyle, metrics: &FontMetrics) -> f32 {
    let minimum = format.minimum_pt * dtool_core::units::units_per_point(doc.units, doc.dpi);
    (match format.initial_advance {
        InitialAdvance::Default | InitialAdvance::Ascent => metrics.ascent_pt,
        InitialAdvance::CapHeight => metrics.cap_height_pt,
        InitialAdvance::XHeight => metrics.x_height_pt,
        InitialAdvance::Leading => style.leading.unwrap_or(metrics.ascent_pt + metrics.descent_pt + metrics.line_gap_pt),
        InitialAdvance::Fixed => minimum,
    }).max(minimum).max(0.0)
}

pub fn layout_table(
    lib: &FontLibrary,
    doc: &Document,
    table: &TableNode,
    grid: &TableGrid,
) -> TableLayout {
    layout_table_impl(lib, doc, table, grid, None, |_, _, _, _, _| {})
}

fn guide(doc: &Document, table: &TableNode, frame: &dtool_core::table::model::TableFrameFormat) -> Option<Guide> {
    let object = doc.objects.iter().find(|object| matches!(&object.kind,
        dtool_core::ObjectKind::Table(candidate) if std::ptr::eq(candidate, table)));
    dtool_core::table::baseline::guide(doc, frame, object)
}

fn snap_line(value: f32, start: f32, spacing: f32) -> f32 {
    let (start, spacing) = (f64::from(start), f64::from(spacing));
    let step = (f64::from(value) - start) / spacing;
    let tolerance = 4.0 * f64::from(f32::EPSILON) * step.abs().clamp(1.0, 16.0);
    let snapped = (start + (step - tolerance).ceil() * spacing) as f32;
    if snapped.is_finite() { snapped } else { value }
}

fn snapped_baselines(baselines: &mut [f32], guide: &Guide, start: f32, minimum: f32) {
    let mut previous = None;
    for baseline in baselines {
        let unadjusted = *baseline;
        let target = previous.map_or(unadjusted, |(raw, snapped)| snapped + unadjusted - raw);
        *baseline = snap_line(target, start, guide.spacing).max(minimum);
        previous = Some((unadjusted, *baseline));
    }
}

pub(crate) fn measure_for_fit(doc: &Document, table: &TableNode, rows: bool, selected: &[bool]) -> Vec<dtool_core::table::fit::CellExtent> {
    use dtool_core::table::fit::CellExtent;
    let lib = FontLibrary::shared();
    let stories: std::collections::HashMap<_, _> = doc.stories.iter().map(|story| (story.id, story)).collect();
    let grid = dtool_core::table::table_grid(table);
    let frame = resolve_frame(doc, table);
    let baseline = guide(doc, table, &frame);
    let other_edges = if rows { &grid.col_x } else { &grid.row_y };
    let point = dtool_core::units::units_per_point(doc.units, doc.dpi);
    table.cells.iter().filter(|cell| cell.merged_into.is_none()).filter_map(|cell| {
        let (start, span, cross, cross_span) = if rows {
            (cell.row, cell.row_span, cell.col, cell.col_span)
        } else {
            (cell.col, cell.col_span, cell.row, cell.row_span)
        };
        let (start, span, cross, cross_span) = (start as usize, span as usize, cross as usize, cross_span as usize);
        if !selected.get(start..start.checked_add(span)?)?.iter().any(|&chosen| chosen) {
            return None;
        }
        let cross_extent = other_edges.get(cross.checked_add(cross_span)?)? - other_edges.get(cross)?;
        let format = resolve_cell(doc, table, cell);
        let insets = text_insets(table, cell, format.insets, &frame);
        let horizontal_insets = insets.left + insets.right;
        let vertical_insets = insets.top + insets.bottom;
        let (along_insets, other_insets) = if rows { (vertical_insets, horizontal_insets) } else { (horizontal_insets, vertical_insets) };
        let sideways = matches!(format.rotation, CellRotation::R90 | CellRotation::R270);
        let fit_block = rows != sideways;
        let (story, style) = stories.get(&cell.story).map(|core| cell_story(doc, table, core, &format))
            .unwrap_or_else(|| (crate::story::Story::default(), default_style(doc)));
        let size = if fit_block {
            let width = (cross_extent - other_insets).max(f32::EPSILON);
            let lines = crate::layout::layout_story(&lib, &story, Some(width));
            let metrics = style_metrics(&lib, &style);
            let advance = initial_advance(doc, &format, &style, &metrics);
            let baseline_span = if let Some(guide) = baseline.as_ref().filter(|_| rows && !sideways && !lines.is_empty()) {
                let mut previous = lines[0].baseline;
                let span: f32 = lines.iter().skip(1).map(|line| {
                    let distance = line.baseline - previous;
                    previous = line.baseline;
                    snap_line(distance, 0.0, guide.spacing)
                }).sum();
                // Earlier fitted rows may shift the phase of the first baseline.
                span + guide.spacing.max(guide.start - insets.top - advance)
            } else {
                lines.first().zip(lines.last()).map_or(0.0, |(first, last)| last.baseline - first.baseline)
            };
            advance + baseline_span + metrics.descent_pt
        } else {
            crate::layout::layout_story(&lib, &story, None).iter()
                .map(|line| crate::layout::intrinsic_line_width(&story, line))
                .fold(point, f32::max)
        };
        Some(CellExtent { start, span, size: (size + along_insets).max(point) })
    }).collect()
}

fn layout_table_impl(
    lib: &FontLibrary,
    doc: &Document,
    table: &TableNode,
    grid: &TableGrid,
    only: Option<CellId>,
    mut visit: impl FnMut(&dtool_core::Story, &crate::story::Story, &mut [Line], &[f32], CellTextBox),
) -> TableLayout {
    let mut cells = Vec::new();
    let frame = resolve_frame(doc, table);
    let baseline_grid = guide(doc, table, &frame);

    for cell in &table.cells {
        if cell.merged_into.is_some() || only.is_some_and(|id| id != cell.id) {
            continue;
        }

        let rect_idx = find_cell_rect(grid, cell.id);
        let Some(rect) = rect_idx.map(|i| grid.rects[i].rect) else {
            continue;
        };

        let format = resolve_cell(doc, table, cell);
        let insets = text_insets(table, cell, format.insets, &frame);

        let content_x0 = rect[0] + insets.left;
        let content_y0 = rect[1] + insets.top;
        let content_x1 = rect[2] - insets.right;
        let content_y1 = rect[3] - insets.bottom;

        let content_width = (content_x1 - content_x0).max(0.0);
        let content_height = (content_y1 - content_y0).max(0.0);
        let sideways = matches!(format.rotation, CellRotation::R90 | CellRotation::R270);
        let (line_width, block_space) = if sideways { (content_height, content_width) } else { (content_width, content_height) };

        let (story, style, mut story_lines) = match doc.stories.iter().find(|s| s.id == cell.story) {
            Some(core) => {
                let (story, style) = cell_story(doc, table, core, &format);
                let lines = crate::layout::layout_story(lib, &story, Some(line_width.max(f32::EPSILON)));
                (Some((core, story)), style, lines)
            }
            None => (None, CharStyle::default(), Vec::new()),
        };
        let metrics = style_metrics(lib, &style);

        let initial_advance = initial_advance(doc, &format, &style, &metrics);

        let first = story_lines.first().map(|line| line.baseline).unwrap_or(0.0);
        let mut top_baselines: Vec<f32> = story_lines
            .iter()
            .map(|line| content_y0 + initial_advance.max(0.0) + (line.baseline - first))
            .collect();
        let guide = baseline_grid.as_ref().filter(|_| !sideways);
        let grid_start = guide.map(|guide| {
            if format.rotation == CellRotation::R180 { content_y0 + content_y1 - guide.start }
            else { guide.start }
        });
        let grid_minimum = if format.rotation == CellRotation::R180 { f32::NEG_INFINITY } else { grid_start.unwrap_or(f32::NEG_INFINITY) };
        if let (Some(guide), Some(start)) = (guide, grid_start) {
            snapped_baselines(&mut top_baselines, guide, start, grid_minimum);
        }
        let total_text_height = top_baselines.last().map(|last| last - content_y0 + metrics.descent_pt).unwrap_or(0.0);
        let overflow = (!story_lines.is_empty() && line_width <= 0.0)
            || total_text_height > block_space + 8.0 * f32::EPSILON * rect.iter().fold(1.0_f32, |extent, value| extent.max(value.abs()));
        let mut baselines = aligned_baselines(&top_baselines, total_text_height, block_space, format.vertical_align);
        if let Some(guide) = guide {
            for (aligned, &top) in baselines.iter_mut().zip(&top_baselines) {
                *aligned = top + ((*aligned - top) / guide.spacing).floor() * guide.spacing;
            }
        }
        let ranges = story_lines.iter().map(|line| [line.range.start, line.range.end]).collect();
        let content_rect = [content_x0, content_y0, content_x1, content_y1];
        if let Some((core, story)) = &story {
            let text_box = CellTextBox {
                rect: content_rect,
                rotation: format.rotation,
                empty_baseline: if let (Some(guide), Some(start)) = (guide, grid_start) {
                    snap_line(content_y0 + initial_advance, start, guide.spacing).max(grid_minimum)
                } else { content_y0 + initial_advance },
                size: style.size_pt,
            };
            visit(core, story, &mut story_lines, &baselines, text_box);
        }
        let lines = story_lines.into_iter().map(|line| line_run(line, &metrics)).collect();

        cells.push(CellLayout {
            id: cell.id,
            content_rect,
            lines,
            ranges,
            baselines,
            overflow,
            rotation: format.rotation,
        });
    }

    TableLayout {
        generation: doc.version,
        cells,
    }
}
fn find_cell_rect(grid: &TableGrid, id: CellId) -> Option<usize> {
    grid.rects.iter().position(|r| r.id == id.0)
}

pub fn table_overflow_with(lib: &FontLibrary, doc: &Document, id: dtool_core::object::ObjectId) -> Option<std::collections::HashMap<CellId, bool>> {
    let object = doc.get_object(id)?;
    let dtool_core::object::ObjectKind::Table(table) = &object.kind else {
        return None;
    };
    let grid = dtool_core::table::geometry::table_grid(table);
    let layout = layout_table(lib, doc, table, &grid);
    Some(layout.cells.iter().map(|cell| (cell.id, cell.overflow)).collect())
}

pub fn cell_lines(cell: &CellLayout) -> dtool_core::table::query::CellLines {
    dtool_core::table::query::CellLines {
        content_rect: cell.content_rect,
        rotation_deg: rotation_degrees(cell.rotation),
        overflow: cell.overflow,
        lines: cell
            .lines
            .iter()
            .zip(cell.baselines.iter())
            .enumerate()
            .map(|(i, (run, baseline))| dtool_core::table::query::CellLine {
                range: cell.ranges.get(i).copied().unwrap_or([0, 0]),
                baseline: *baseline,
                width: run.width,
                ascent: run.ascent,
                descent: run.descent,
                glyphs: run.glyphs.len(),
            })
            .collect(),
    }
}

pub fn table_lines_with(lib: &FontLibrary, doc: &Document, id: dtool_core::object::ObjectId) -> Option<dtool_core::table::query::TableLines> {
    let object = doc.get_object(id)?;
    let dtool_core::object::ObjectKind::Table(table) = &object.kind else {
        return None;
    };
    let font_generation = crate::fonts::font_generation();
    let grid = dtool_core::table::geometry::table_grid(table);
    let layout = layout_table(lib, doc, table, &grid);
    let cells = layout.cells.iter().map(|cell| (cell.id, cell_lines(cell))).collect();
    Some(dtool_core::table::query::TableLines { font_generation, cells })
}

pub fn table_lines_shared(doc: &Document, id: dtool_core::object::ObjectId) -> Option<dtool_core::table::query::TableLines> {
    table_lines_with(&FontLibrary::shared(), doc, id)
}

pub fn table_overflow_shared(doc: &Document, id: dtool_core::object::ObjectId) -> Option<std::collections::HashMap<CellId, bool>> {
    table_overflow_with(&FontLibrary::shared(), doc, id)
}

#[derive(Clone, Copy)]
pub struct CellTextBox {
    pub rect: [f32; 4],
    pub rotation: CellRotation,
    pub empty_baseline: f32,
    pub size: f32,
}

impl CellTextBox {
    pub fn to_cell(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [x0, y0, x1, y1] = self.rect;
        match self.rotation {
            CellRotation::R0 => [x, y],
            CellRotation::R90 => [x1 - (y - y0), y0 + (x - x0)],
            CellRotation::R180 => [x1 - (x - x0), y1 - (y - y0)],
            CellRotation::R270 => [x0 + (y - y0), y1 - (x - x0)],
        }
    }

    pub fn to_layout(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [x0, y0, x1, y1] = self.rect;
        match self.rotation {
            CellRotation::R0 => [x, y],
            CellRotation::R90 => [x0 + (y - y0), y0 + (x1 - x)],
            CellRotation::R180 => [x0 + (x1 - x), y0 + (y1 - y)],
            CellRotation::R270 => [x0 + (y1 - y), y0 + (x - x0)],
        }
    }
}

pub fn cell_text_layout(lib: &FontLibrary, doc: &Document, table: &TableNode, id: CellId) -> Option<(CellTextBox, Vec<Line>)> {
    let grid = dtool_core::table::geometry::table_grid(table);
    let mut result = None;
    layout_table_impl(lib, doc, table, &grid, Some(id), |_, _, lines, baselines, text_box| {
        let mut lines = lines.to_vec();
        for (line, baseline) in lines.iter_mut().zip(baselines) {
            line.baseline = *baseline;
        }
        result = Some((text_box, lines));
    });
    result
}

/// Local cell glyphs clipped to their content box, ready for the table transform.
pub fn table_text_contours(
    lib: &FontLibrary, doc: &Document, table: &TableNode,
) -> Vec<(Paint, Vec<Vec<[f32; 2]>>)> {
    let grid = dtool_core::table::geometry::table_grid(table);
    let mut ink = Vec::new();
    layout_table_impl(lib, doc, table, &grid, None, |core, story, lines, baselines, text_box| {
        let rect = text_box.rect;
        let [x0, y0, x1, y1] = rect;
        if x1 <= x0 || y1 <= y0 { return; }
        for (line, baseline) in lines.iter_mut().zip(baselines) {
            line.baseline = *baseline;
        }
        let paint = dtool_core::text_lines::story_paint(core);
        let contours = crate::canvas_text::line_outlines(lib, story, lines, [x0, 0.0], |_, _, _| true)
            .into_iter()
            .map(|contour| clip_cell_contour(contour.into_iter().map(|point| text_box.to_cell(point)).collect(), rect))
            .filter(|contour| contour.len() >= 3)
            .collect::<Vec<_>>();
        if !contours.is_empty() {
            ink.push((paint, contours));
        }
    });
    ink
}

fn clip_cell_contour(mut contour: Vec<[f32; 2]>, rect: [f32; 4]) -> Vec<[f32; 2]> {
    if contour.iter().all(|p| (rect[0]..=rect[2]).contains(&p[0]) && (rect[1]..=rect[3]).contains(&p[1])) {
        return contour;
    }
    for (axis, boundary, sign) in [(0, rect[0], 1.0), (1, rect[1], 1.0), (0, rect[2], -1.0), (1, rect[3], -1.0)] {
        let Some(mut previous) = contour.last().copied() else { break; };
        let inside = |p: [f32; 2]| (p[axis] - boundary) * sign >= 0.0;
        let mut clipped = Vec::new();
        for point in contour {
            if inside(previous) != inside(point) {
                let t = (boundary - previous[axis]) / (point[axis] - previous[axis]);
                let mut crossing = previous;
                crossing[axis] = boundary;
                crossing[1 - axis] += (point[1 - axis] - previous[1 - axis]) * t;
                clipped.push(crossing);
            }
            if inside(point) { clipped.push(point); }
            previous = point;
        }
        contour = clipped;
    }
    contour
}
