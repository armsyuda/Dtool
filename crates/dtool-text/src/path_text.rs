use serde::{Deserialize, Serialize};
use crate::story::StoryId;
use dtool_core::{ObjectId, ObjectKind};
use crate::fonts::{FontLibrary, UNMAPPED_FONT};
use crate::layout::{append_tab_char, path_tabs, tab_gap, TabSource};
use crate::para_style::Leader;
use crate::shape::{ShapedGlyph, shape_run_with, ShapeOptions};
use crate::story::{CharStyle, Story, TextRange};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathFlow {
    #[serde(default)]
    pub start_pct: f32,
    #[serde(default)]
    pub end_pct: f32,
    #[serde(default)]
    pub baseline_pct: f32,
    #[serde(default)]
    pub reversed: bool,
    #[serde(default)]
    pub flipped: bool,
}

impl Default for PathFlow {
    fn default() -> Self {
        Self {
            start_pct: 0.0,
            end_pct: 1.0,
            baseline_pct: 0.5,
            reversed: false,
            flipped: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathTextData {
    pub path: ObjectId,
    pub story: StoryId,
    #[serde(default)]
    pub flows: Vec<PathFlow>,
    #[serde(default)]
    pub source_paint: Option<String>,
}

impl Default for PathTextData {
    fn default() -> Self {
        Self {
            path: ObjectId(0),
            story: StoryId(0),
            flows: vec![PathFlow::default()],
            source_paint: None,
        }
    }
}

impl From<&dtool_core::object::PathFlow> for PathFlow {
    fn from(flow: &dtool_core::object::PathFlow) -> Self {
        Self {
            start_pct: flow.start_pct,
            end_pct: flow.end_pct,
            baseline_pct: flow.baseline_pct,
            reversed: flow.reversed,
            flipped: flow.flipped,
        }
    }
}

impl From<&PathFlow> for dtool_core::object::PathFlow {
    fn from(flow: &PathFlow) -> Self {
        Self {
            start_pct: flow.start_pct,
            end_pct: flow.end_pct,
            baseline_pct: flow.baseline_pct,
            reversed: flow.reversed,
            flipped: flow.flipped,
        }
    }
}

impl From<&dtool_core::object::PathTextData> for PathTextData {
    fn from(data: &dtool_core::object::PathTextData) -> Self {
        Self {
            path: data.path,
            story: StoryId(data.story.0),
            flows: data.flows.iter().map(PathFlow::from).collect(),
            source_paint: data.source_paint.clone(),
        }
    }
}

impl From<&PathTextData> for dtool_core::object::PathTextData {
    fn from(data: &PathTextData) -> Self {
        Self {
            path: data.path,
            story: dtool_core::StoryId(data.story.0),
            flows: data.flows.iter().map(dtool_core::object::PathFlow::from).collect(),
            source_paint: data.source_paint.clone(),
        }
    }
}

const PATH_SAMPLES: usize = 64;

fn cubic_eval(segment: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    [
        a * segment[0][0] + b * segment[1][0] + c * segment[2][0] + d * segment[3][0],
        a * segment[0][1] + b * segment[1][1] + c * segment[2][1] + d * segment[3][1],
    ]
}

fn cubic_deriv(segment: &[[f64; 2]; 4], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    let a = 3.0 * u * u;
    let b = 6.0 * u * t;
    let c = 3.0 * t * t;
    [
        a * (segment[1][0] - segment[0][0]) + b * (segment[2][0] - segment[1][0]) + c * (segment[3][0] - segment[2][0]),
        a * (segment[1][1] - segment[0][1]) + b * (segment[2][1] - segment[1][1]) + c * (segment[3][1] - segment[2][1]),
    ]
}

struct MeasuredSegment {
    points: [[f64; 2]; 4],
    table: Vec<f64>,
    length: f64,
}

struct PathMeasure {
    segments: Vec<MeasuredSegment>,
    length: f64,
}

impl PathMeasure {
    fn new(path: &[[[f32; 2]; 4]]) -> Self {
        let segments: Vec<_> = path.iter().filter_map(|segment| {
            let points = segment.map(|point| [f64::from(point[0]), f64::from(point[1])]);
            if !points.iter().all(|point| point[0].is_finite() && point[1].is_finite()) { return None; }
            let mut table = Vec::with_capacity(PATH_SAMPLES + 1);
            table.push(0.0);
            let mut previous = points[0];
            let mut length = 0.0f64;
            for step in 1..=PATH_SAMPLES {
                let next = cubic_eval(&points, step as f64 / PATH_SAMPLES as f64);
                length += (next[0] - previous[0]).hypot(next[1] - previous[1]);
                table.push(length);
                previous = next;
            }
            (length.is_finite() && length > 0.0).then_some(MeasuredSegment { points, table, length })
        }).collect();
        let length = segments.iter().map(|segment| segment.length).sum();
        Self { segments, length }
    }

    fn point(&self, distance: f64) -> ([f32; 2], f32) {
        if !distance.is_finite() || !self.length.is_finite() || self.length <= 0.0 { return ([0.0, 0.0], 0.0); }
        let mut remaining = distance.clamp(0.0, self.length);
        for (index, segment) in self.segments.iter().enumerate() {
            if remaining <= segment.length || index + 1 == self.segments.len() {
                let target = remaining.clamp(0.0, segment.length);
                let step = segment.table.partition_point(|value| *value < target).clamp(1, PATH_SAMPLES);
                let low = segment.table[step - 1];
                let high = segment.table[step];
                let fraction = if high > low { (target - low) / (high - low) } else { 0.0 };
                let t = ((step - 1) as f64 + fraction) / PATH_SAMPLES as f64;
                let point = cubic_eval(&segment.points, t);
                let mut tangent = cubic_deriv(&segment.points, t);
                if tangent[0].hypot(tangent[1]) <= f64::EPSILON {
                    let after = cubic_eval(&segment.points, (t + 1e-5).min(1.0));
                    let before = cubic_eval(&segment.points, (t - 1e-5).max(0.0));
                    tangent = [after[0] - before[0], after[1] - before[1]];
                }
                return ([point[0] as f32, point[1] as f32], tangent[1].atan2(tangent[0]) as f32);
            }
            remaining -= segment.length;
        }
        ([0.0, 0.0], 0.0)
    }
}

pub fn line_segment(from: [f32; 2], to: [f32; 2]) -> [[f32; 2]; 4] {
    let third = [(to[0] - from[0]) / 3.0, (to[1] - from[1]) / 3.0];
    [from, [from[0] + third[0], from[1] + third[1]], [to[0] - third[0], to[1] - third[1]], to]
}

pub fn path_point_at(path: &[[[f32; 2]; 4]], s: f32) -> ([f32; 2], f32) {
    PathMeasure::new(path).point(f64::from(s))
}

pub fn kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Shape => "Shape",
        ObjectKind::Table(_) => "Table",
        ObjectKind::Group => "Group",
        ObjectKind::Text(_) => "Text",
        ObjectKind::ArtText { .. } => "ArtText",
        ObjectKind::FrameText { .. } => "FrameText",
        ObjectKind::PathText(_) => "PathText",
        ObjectKind::Curve(_) => "Curve",
        ObjectKind::Dimension(_) => "Dimension",
        ObjectKind::Artboard => "Artboard",
        ObjectKind::Image => "Image",
        ObjectKind::PictureFrame => "PictureFrame",
        ObjectKind::Compound(_) => "Compound",
    }
}

pub fn as_path_text(kind: &ObjectKind) -> Option<PathTextData> {
    match kind {
        ObjectKind::PathText(data) => Some(PathTextData::from(data)),
        _ => None,
    }
}
#[derive(Debug, Clone)]
pub struct PlacedGlyph {
    pub glyph: ShapedGlyph,
    pub position: [f32; 2],
    pub angle: f32,
    pub cluster: TextRange,
    pub flipped: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PathTextLayout {
    pub glyphs: Vec<PlacedGlyph>,
    pub overset: Option<TextRange>,
    pub bounds: [f32; 4],
    tab_contours: Vec<Vec<[f32; 2]>>,
}

fn overset(layout: &mut PathTextLayout, start: usize, end: usize) {
    if start < end { layout.overset = Some(TextRange::new(start, end)); }
}

fn include_bounds(layout: &mut PathTextLayout, origin: [f32; 2], angle: f32, advance: f32, ascent: f32, descent: f32, flipped: bool) {
    let (sin, cos) = angle.sin_cos();
    let side = if flipped { -1.0 } else { 1.0 };
    if layout.glyphs.is_empty() && layout.bounds == [0.0, 0.0, 0.0, 0.0] { layout.bounds = [origin[0], origin[1], origin[0], origin[1]]; }
    for x in [0.0, advance] {
        for y in [-ascent, descent] {
            let y = y * side;
            let point = [origin[0] + x * cos - y * sin, origin[1] + x * sin + y * cos];
            layout.bounds[0] = layout.bounds[0].min(point[0]);
            layout.bounds[1] = layout.bounds[1].min(point[1]);
            layout.bounds[2] = layout.bounds[2].max(point[0]);
            layout.bounds[3] = layout.bounds[3].max(point[1]);
        }
    }
}

struct PathCluster<'a> {
    glyphs: Vec<ShapedGlyph>,
    range: TextRange,
    style: &'a CharStyle,
    ascent: f32,
    descent: f32,
    advance: f64,
}

impl PathCluster<'_> {
    fn step(&self) -> f64 {
        (self.advance + f64::from(self.style.tracking) * f64::from(self.style.size_pt) / 1000.0).max(0.0)
    }
}

fn shape_path_field<'a>(
    lib: &FontLibrary, text: &str, start: usize, style_at: impl Fn(usize) -> &'a CharStyle,
) -> (Vec<PathCluster<'a>>, Option<usize>) {
    let boundaries: Vec<_> = text.char_indices().map(|(index, _)| index).chain(std::iter::once(text.len())).collect();
    let count = boundaries.len() - 1;
    let mut clusters = Vec::new();
    let mut begin = 0;
    while begin < count {
        let style = style_at(start + begin);
        let mut stop = begin + 1;
        while stop < count && style_at(start + stop) == style { stop += 1; }
        let piece = &text[boundaries[begin]..boundaries[stop]];
        if !style.size_pt.is_finite() || style.size_pt <= 0.0 || !style.tracking.is_finite() || !style.baseline_pt.is_finite() {
            return (clusters, Some(start + begin));
        }
        let shaped = shape_run_with(lib, piece, &style.family, &style.style, style.size_pt, &ShapeOptions::from_style(style));
        if shaped.glyphs.is_empty() {
            return (clusters, Some(start + begin));
        }
        let mut cluster_starts: Vec<_> = shaped.glyphs.iter().map(|glyph| glyph.cluster).chain(std::iter::once(piece.len())).collect();
        cluster_starts.sort_unstable();
        cluster_starts.dedup();
        for glyphs in shaped.glyphs.chunk_by(|a, b| a.cluster == b.cluster) {
            let byte = glyphs[0].cluster;
            let end = cluster_starts[cluster_starts.partition_point(|next| *next <= byte).min(cluster_starts.len() - 1)];
            let (Ok(first), Ok(last)) = (
                boundaries.binary_search(&(boundaries[begin] + byte)),
                boundaries.binary_search(&(boundaries[begin] + end)),
            ) else { return (clusters, Some(start + begin)); };
            if first >= last || last > stop {
                return (clusters, Some(start + begin));
            }
            let range = TextRange::new(start + first, start + last);
            if glyphs.iter().any(|glyph| !glyph.advance.is_finite() || glyph.offset.iter().any(|value| !value.is_finite())) {
                return (clusters, Some(range.start));
            }
            let advance = glyphs.iter().map(|glyph| f64::from(glyph.advance)).sum::<f64>();
            if advance < 0.0 {
                return (clusters, Some(range.start));
            }
            clusters.push(PathCluster { glyphs: glyphs.to_vec(), range, style, ascent: shaped.ascent, descent: shaped.descent, advance });
        }
        begin = stop;
    }
    (clusters, None)
}

fn path_field_width(clusters: &[PathCluster<'_>], text: &str, start: usize, decimal: Option<char>) -> f32 {
    let decimal = decimal.and_then(|decimal| text.chars().position(|ch| ch == decimal)).map(|index| start + index);
    let mut pen = 0.0;
    let mut end = 0.0;
    for cluster in clusters {
        if decimal.is_some_and(|index| cluster.range.start <= index && index < cluster.range.end) {
            return pen as f32;
        }
        end = pen + cluster.advance;
        pen += cluster.step();
    }
    end as f32
}

struct PathPlacement<'a> {
    measure: &'a PathMeasure,
    flow: &'a PathFlow,
    origin: f64,
    direction: f64,
    capacity: f64,
    wraps: bool,
}

impl PathPlacement<'_> {
    fn fits(&self, start: f64, end: f64) -> bool {
        end <= self.capacity && (self.wraps || [start, end].into_iter().all(|cursor| {
            let distance = self.origin + self.direction * cursor;
            (0.0..=self.measure.length).contains(&distance)
        }))
    }

    fn point(&self, cursor: f64) -> ([f32; 2], f32) {
        let distance = self.origin + self.direction * cursor;
        let distance = if self.wraps { distance.rem_euclid(self.measure.length) } else { distance };
        let (point, tangent) = self.measure.point(distance);
        (point, tangent + if self.direction < 0.0 { std::f32::consts::PI } else { 0.0 })
    }

    fn offset_point(&self, cursor: f64, offset: f32) -> ([f32; 2], f32) {
        let (point, angle) = self.point(cursor);
        (self.local_point(point, angle, 0.0, offset), angle)
    }

    fn local_point(&self, point: [f32; 2], angle: f32, x: f32, y: f32) -> [f32; 2] {
        let (sin, cos) = angle.sin_cos();
        let y = if self.flow.flipped { -y } else { y };
        [point[0] + x * cos - y * sin, point[1] + x * sin + y * cos]
    }

    fn place_cluster(&self, layout: &mut PathTextLayout, cluster: PathCluster<'_>, cursor: f64) -> bool {
        let baseline = self.flow.baseline_pct * cluster.style.size_pt + cluster.style.baseline_pt;
        if !baseline.is_finite() || !self.fits(cursor, cursor + cluster.advance) {
            return false;
        }
        let (point, angle) = self.point(cursor + cluster.advance * 0.5);
        let mut offset = -(cluster.advance as f32) * 0.5;
        let mut placed = Vec::with_capacity(cluster.glyphs.len());
        for glyph in cluster.glyphs {
            let position = self.local_point(point, angle, offset + glyph.offset[0], glyph.offset[1] + baseline);
            if position.iter().any(|value| !value.is_finite()) {
                return false;
            }
            offset += glyph.advance;
            placed.push(PlacedGlyph { glyph, position, angle, cluster: cluster.range, flipped: self.flow.flipped });
        }
        for glyph in placed {
            include_bounds(layout, glyph.position, angle, glyph.glyph.advance, cluster.ascent, cluster.descent, glyph.flipped);
            layout.glyphs.push(glyph);
        }
        true
    }

    fn place_tab(
        &self, layout: &mut PathTextLayout, source: TabSource<'_>, style: &CharStyle,
        start: f64, gap: f32, index: usize, leader: Leader,
    ) -> bool {
        let end = start + f64::from(gap);
        let baseline = self.flow.baseline_pct * style.size_pt + style.baseline_pt;
        if !baseline.is_finite() || !self.fits(start, end) {
            return false;
        }
        let range = TextRange::new(index, index + 1);
        let (ascent, descent) = (style.size_pt * 0.8, style.size_pt * 0.2);
        let mut leaders = Vec::new();
        if let Leader::Character(ch) = leader {
            append_tab_char(&mut leaders, source, ch, gap, index - source.paragraph_start);
        }
        // Keep tab carets at both arc endpoints, even if the gap has no visible glyphs.
        let spacer = || ShapedGlyph { font: UNMAPPED_FONT, gid: 0, cluster: index, advance: 0.0, offset: [0.0, 0.0] };
        let mut pen = start;
        let glyphs = std::iter::once((start, spacer()))
            .chain(leaders.into_iter().map(|glyph| {
                let at = pen;
                pen += f64::from(glyph.advance);
                (at, glyph)
            }))
            .chain(std::iter::once((end, spacer())));
        for (at, glyph) in glyphs {
            let advance = f64::from(glyph.advance);
            let (point, angle) = self.point((at + advance * 0.5).min(end));
            let position = self.local_point(point, angle, glyph.offset[0] - glyph.advance * 0.5, baseline + glyph.offset[1]);
            include_bounds(layout, position, angle, glyph.advance, ascent, descent, self.flow.flipped);
            layout.glyphs.push(PlacedGlyph { glyph, position, angle, cluster: range, flipped: self.flow.flipped });
        }
        if gap > 0.0 && matches!(leader, Leader::Underline | Leader::Strikeout) {
            let metrics = crate::decorations::line_metrics(source.lib, style);
            let (offset, thickness) = if leader == Leader::Underline {
                (metrics.underline_offset, metrics.underline_thickness)
            } else {
                (metrics.strike_offset, metrics.strike_thickness)
            };
            let slices = (gap / (style.size_pt * 0.25).max(0.01)).ceil().clamp(1.0, 1024.0) as usize;
            let points = |edge: f32, index: usize| {
                let cursor = start + f64::from(gap) * index as f64 / slices as f64;
                self.offset_point(cursor, baseline - offset + edge * thickness * 0.5).0
            };
            let contour = (0..=slices).map(|index| points(-1.0, index))
                .chain((0..=slices).rev().map(|index| points(1.0, index))).collect();
            layout.tab_contours.push(contour);
        }
        true
    }
}

pub fn layout_path_text(lib: &FontLibrary, data: &PathTextData, story: &Story, path: &[[[f32; 2]; 4]]) -> PathTextLayout {
    let mut layout = PathTextLayout::default();
    let total = story.text.chars().count();
    if total == 0 { return layout; }
    let measure = PathMeasure::new(path);
    if measure.length <= 0.0 || !measure.length.is_finite() {
        overset(&mut layout, 0, total);
        return layout;
    }
    let default_flow = PathFlow::default();
    let flows = if data.flows.is_empty() { std::slice::from_ref(&default_flow) } else { &data.flows };
    let fallback = CharStyle::default();
    let closed = path.first().zip(path.last()).is_some_and(|(first, last)| first[0] == last[3]);
    let mut scalar_start = 0;
    for (line_index, line) in story.text.split('\n').enumerate() {
        let count = line.chars().count();
        if count == 0 { scalar_start += 1; continue; }
        let Some(flow) = flows.get(line_index) else {
            overset(&mut layout, scalar_start, total);
            return layout;
        };
        if !flow.start_pct.is_finite() || !flow.end_pct.is_finite() || !flow.baseline_pct.is_finite() {
            overset(&mut layout, scalar_start, total);
            return layout;
        }
        let start = f64::from(flow.start_pct) * measure.length;
        let end = f64::from(flow.end_pct) * measure.length;
        let direction = (if end >= start { 1.0 } else { -1.0 }) * if flow.reversed { -1.0 } else { 1.0 };
        let origin = if flow.reversed { end } else { start };
        let capacity = (end - start).abs();
        let wraps = closed && flow.end_pct > 1.0 && flow.start_pct >= 0.0 && flow.start_pct <= 1.0
            && flow.end_pct - flow.start_pct <= 1.0 + f32::EPSILON;
        let placement = PathPlacement { measure: &measure, flow, origin, direction, capacity, wraps };
        let style_at = |index: usize| story.chars.iter().rev()
            .find(|run| run.range.start <= index && index < run.range.end).map(|run| &run.style).unwrap_or(&fallback);
        let tabs = line.contains('\t').then(|| path_tabs(story, scalar_start));
        let source = TabSource { lib, story, paragraph_start: scalar_start };
        let mut cursor = 0.0f64;
        let mut field_start = scalar_start;
        for (field_index, field) in line.split('\t').enumerate() {
            let (clusters, invalid) = shape_path_field(lib, field, field_start, style_at);
            if let Some(tabs) = tabs.as_ref().filter(|_| field_index != 0) {
                let (gap, leader) = tab_gap(&tabs.tabs, cursor as f32, capacity as f32, tabs.default_tab_interval,
                    |decimal| path_field_width(&clusters, field, field_start, decimal));
                let tab = field_start - 1;
                if !placement.place_tab(&mut layout, source, style_at(tab), cursor, gap, tab, leader) {
                    overset(&mut layout, tab, total);
                    return layout;
                }
                cursor += f64::from(gap);
            }
            for cluster in clusters {
                let step = cluster.step();
                let start = cluster.range.start;
                if !placement.place_cluster(&mut layout, cluster, cursor) {
                    overset(&mut layout, start, total);
                    return layout;
                }
                cursor += step;
            }
            if let Some(invalid) = invalid {
                overset(&mut layout, invalid, total);
                return layout;
            }
            field_start += field.chars().count() + 1;
        }
        scalar_start += count + 1;
    }
    layout
}

fn path_segments(text: &dtool_core::Object, path_object: &dtool_core::Object) -> Vec<[[f32; 2]; 4]> {
    let mut segments = Vec::new();
    for (mut polyline, closed) in dtool_core::outline::path_text_layout_contours(text, path_object) {
        if closed {
            if let Some(first) = polyline.first().copied() {
                polyline.push(first);
            }
        }
        for pair in polyline.windows(2) {
            if pair[0] != pair[1] {
                segments.push(line_segment(pair[0], pair[1]));
            }
        }
    }
    segments
}

pub fn path_text_layout_in_doc(lib: &FontLibrary, doc: &dtool_core::document::Document, object: &dtool_core::object::Object) -> Option<(Story, PathTextLayout)> {
    let ObjectKind::PathText(owned) = &object.kind else { return None; };
    let core_story = doc.stories.iter().find(|story| story.id == owned.story)?;
    let path_object = doc.get_object(owned.path)?;
    let story = crate::bridge::story_in_document(core_story, doc);
    let data = PathTextData::from(owned);
    let layout = layout_path_text(lib, &data, &story, &path_segments(object, path_object));
    Some((story, layout))
}

pub fn path_text_insertions(lib: &FontLibrary, doc: &dtool_core::Document, object: &dtool_core::Object) -> Vec<(usize, [[f32; 2]; 2])> {
    let ObjectKind::PathText(data) = &object.kind else { return Vec::new(); };
    let Some((story, layout)) = path_text_layout_in_doc(lib, doc, object) else { return Vec::new(); };
    let matrix = dtool_core::geometry::text_matrix(object);
    let insertion = |pos: [f32; 2], angle: f32, size: f32, flipped: bool| {
        let (sin, cos) = angle.sin_cos();
        let direction = if flipped { -1.0 } else { 1.0 };
        [-0.8, 0.2].map(|height| {
            let offset = size * height * direction;
            dtool_core::geometry::apply_matrix(&matrix, [pos[0] - sin * offset, pos[1] + cos * offset])
        })
    };
    let mut carets = Vec::new();
    for cluster in layout.glyphs.chunk_by(|a, b| a.cluster == b.cluster) {
        let first = &cluster[0];
        let last = &cluster[cluster.len() - 1];
        let size = story.chars.iter().rev().find(|run| run.range.start <= first.cluster.start && first.cluster.start < run.range.end)
            .map_or(dtool_core::text_cmd::DEFAULT_SIZE_PT, |run| run.style.size_pt);
        let (sin, cos) = last.angle.sin_cos();
        let end = [last.position[0] + cos * last.glyph.advance, last.position[1] + sin * last.glyph.advance];
        let span = first.cluster.end - first.cluster.start;
        for offset in 0..=span {
            let fraction = offset as f32 / span.max(1) as f32;
            let pos = [first.position[0] + (end[0] - first.position[0]) * fraction, first.position[1] + (end[1] - first.position[1]) * fraction];
            let angle = if offset == span { last.angle } else { first.angle };
            carets.push((first.cluster.start + offset, insertion(pos, angle, size, first.flipped)));
        }
    }
    if !carets.is_empty() { return carets; }
    let Some(path) = doc.get_object(data.path) else { return carets; };
    let measure = PathMeasure::new(&path_segments(object, path));
    if measure.length <= 0.0 { return carets; }
    let flow = data.flows.first().copied().unwrap_or_default();
    let fraction = if flow.reversed { flow.end_pct } else { flow.start_pct };
    let fraction = if flow.end_pct > 1.0 && flow.end_pct - flow.start_pct <= 1.0 + f32::EPSILON {
        fraction.rem_euclid(1.0)
    } else { fraction };
    let (mut pos, mut angle) = measure.point(f64::from(fraction) * measure.length);
    if (flow.end_pct < flow.start_pct) != flow.reversed { angle += std::f32::consts::PI; }
    let size = doc.stories.iter().find(|story| story.id == data.story)
        .map_or(dtool_core::text_cmd::DEFAULT_SIZE_PT, |story| dtool_core::text_cmd::size_pt_of(story, dtool_core::text_cmd::DEFAULT_SIZE_PT))
        * dtool_core::units::units_per_point(doc.units, doc.dpi);
    let baseline = flow.baseline_pct * size * if flow.flipped { -1.0 } else { 1.0 };
    let (sin, cos) = angle.sin_cos();
    pos[0] -= sin * baseline;
    pos[1] += cos * baseline;
    carets.push((0, insertion(pos, angle, size, flow.flipped)));
    carets
}

pub fn path_text_contours(lib: &FontLibrary, doc: &dtool_core::document::Document, object: &dtool_core::object::Object) -> Vec<Vec<[f32; 2]>> {
    let Some((story, layout)) = path_text_layout_in_doc(lib, doc, object) else {
        return Vec::new();
    };
    let mut contours = Vec::new();
    visit_path_contours(lib, &story, layout, object, None, |_, contour| contours.push(contour));
    contours
}

fn visit_path_contours(
    lib: &FontLibrary, story: &Story, layout: PathTextLayout, object: &dtool_core::Object, tolerance: Option<f64>,
    mut add: impl FnMut(Option<&CharStyle>, Vec<[f32; 2]>),
) {
    let matrix = dtool_core::geometry::text_matrix(object);
    let world = |point| dtool_core::geometry::apply_matrix(&matrix, point);
    for tab in layout.tab_contours {
        add(None, tab.into_iter().map(world).collect());
    }
    let fallback = CharStyle::default();
    for placed in layout.glyphs {
        if placed.glyph.font == UNMAPPED_FONT { continue; }
        let style = story.chars.iter().rev().find(|run| run.range.start <= placed.cluster.start && placed.cluster.start < run.range.end).map(|run| &run.style).unwrap_or(&fallback);
        let (sin, cos) = placed.angle.sin_cos();
        let side = if placed.flipped { -1.0 } else { 1.0 };
        for contour in lib.glyph_contours_with_tolerance(placed.glyph.font, placed.glyph.gid, style.size_pt, tolerance).iter() {
            add(Some(style), contour.iter().map(|point| {
                let x = point[0];
                let y = -side * point[1];
                world([placed.position[0] + x * cos - y * sin, placed.position[1] + x * sin + y * cos])
            }).collect());
        }
    }
}

pub fn path_text_ink(lib: &FontLibrary, doc: &dtool_core::Document, object: &dtool_core::Object) -> Option<Vec<dtool_core::text_lines::TextInk>> {
    path_text_ink_with_tolerance(lib, doc, object, None)
}

pub(crate) fn path_text_ink_with_tolerance(lib: &FontLibrary, doc: &dtool_core::Document, object: &dtool_core::Object,
    tolerance: Option<f64>,
) -> Option<Vec<dtool_core::text_lines::TextInk>> {
    let core = doc.stories.iter().find(|core| Some(core.id) == object.story_id())?;
    let paint = path_text_paint(doc, object);
    if tolerance.is_none() && !crate::canvas_text::has_local_text_fill(core, &paint) {
        return None;
    }
    let (story, layout) = path_text_layout_in_doc(lib, doc, object)?;
    if tolerance.is_none() && !story.chars.iter().any(|run| run.style.fill.as_ref().is_some_and(|fill| fill != &paint)) {
        return None;
    }
    let mut ink = Vec::new();
    visit_path_contours(lib, &story, layout, object, tolerance, |style, contour| {
        let fill = style.and_then(|style| style.fill.as_ref()).unwrap_or(&paint);
        dtool_core::text_lines::TextInk::append(&mut ink, fill, contour);
    });
    Some(ink)
}

/// Return contours when tabs or a group transform require the full path layout.
pub fn export_path_text_contours(lib: &FontLibrary, doc: &dtool_core::document::Document, object: &dtool_core::Object) -> Option<Vec<Vec<[f32; 2]>>> {
    let ObjectKind::PathText(data) = &object.kind else { return None; };
    let core = doc.stories.iter().find(|candidate| candidate.id == data.story)?;
    (core.text.contains('\t') || object.path_text_linear.is_some()).then(|| path_text_contours(lib, doc, object))
}

pub fn path_text_paint(doc: &dtool_core::Document, object: &dtool_core::Object) -> dtool_core::paint::Paint {
    if object.fill != dtool_core::paint::Paint::None {
        return object.fill.clone();
    }
    object.story_id().and_then(|id| doc.stories.iter().find(|story| story.id == id))
        .map(dtool_core::text_lines::story_paint)
        .unwrap_or_else(|| dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(0, 0, 0, 255) })
}
