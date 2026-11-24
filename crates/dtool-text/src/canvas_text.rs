use crate::story::{Line, TabRule};
use crate::FontLibrary;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::{register_overset, register_text_lines, story_paint, TextLine};
use dtool_core::Document;
use std::borrow::Cow;
use std::rc::Rc;

fn vertical_upright(ch: char) -> bool {
    matches!(unicode_vo::char_orientation(ch),
        unicode_vo::Orientation::Upright | unicode_vo::Orientation::TransformedOrUpright)
}

/// Keep each upright grapheme together and preserve contiguous sideways text as a run.
pub fn vertical_composition_runs(text: &str) -> Vec<(&str, bool)> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut runs = Vec::new();
    let mut sideways = None;
    for (offset, cluster) in text.grapheme_indices(true) {
        if cluster.chars().next().is_some_and(vertical_upright) {
            if let Some(start) = sideways.take() {
                runs.push((&text[start..offset], false));
            }
            runs.push((cluster, true));
        } else {
            sideways.get_or_insert(offset);
        }
    }
    if let Some(start) = sideways {
        runs.push((&text[start..], false));
    }
    runs
}

fn previewed(doc: &Document, story: dtool_core::StoryId) -> Cow<'_, Document> {
    crate::font_preview::preview_document(doc, story).map_or(Cow::Borrowed(doc), Cow::Owned)
}

fn to_text_lines(doc: &Document, core: &dtool_core::Story, lines: &[Line], pos: [f32; 2], id: ObjectId) -> Vec<TextLine> {
    let chars: Vec<char> = core.text.chars().collect();
    let size = dtool_core::text_cmd::size_pt_of(core, dtool_core::text_cmd::DEFAULT_SIZE_PT) * dtool_core::units::units_per_point(doc.units, doc.dpi);
    let paint = story_paint(core);
    lines
        .iter()
        .map(|line| {
            let start = line.range.start.min(chars.len());
            let end = line.range.end.min(chars.len()).max(start);
            let text: String = chars[start..end].iter().filter(|c| **c != '\n').collect();
            let x = line.body_glyphs().first().map(|g| g.offset[0]).unwrap_or(0.0);
            TextLine {
                object: id,
                origin: [pos[0] + x, pos[1] + line.baseline],
                text,
                size,
                width: line.width,
                para_end: chars.get(end) == Some(&'\n'),
                paint: paint.clone(),
            }
        })
        .collect()
}

pub fn text_lines_with(lib: &FontLibrary, doc: &Document, id: ObjectId) -> Option<Vec<TextLine>> {
    let object = doc.get_object(id)?;
    match &object.kind {
        ObjectKind::FrameText { story } => {
            let view = previewed(doc, *story);
            let doc = view.as_ref();
            let core = doc.stories.iter().find(|s| s.id == *story)?;
            let flow = crate::frame::shared_doc_flow(lib, doc, *story)?;
            let index = flow.frames.iter().position(|f| *f == id)?;
            let lines = flow.layout.per_frame.get(index)?;
            Some(to_text_lines(doc, core, lines, object.pos, id))
        }
        ObjectKind::ArtText { story } => {
            let view = previewed(doc, *story);
            let core = view.stories.iter().find(|s| s.id == *story)?;
            let layout = crate::frame::shared_artistic_layout(lib, &view, *story)?;
            Some(to_text_lines(&view, core, &layout.1, object.pos, id))
        }
        _ => None,
    }
}

fn break_marks_from(doc: &Document, core: &dtool_core::Story, lines: &[Line], pos: [f32; 2]) -> Vec<dtool_core::text_lines::BreakMark> {
    use dtool_core::document::{BreakKind, Special};
    let size = dtool_core::text_cmd::size_pt_of(core, dtool_core::text_cmd::DEFAULT_SIZE_PT) * dtool_core::units::units_per_point(doc.units, doc.dpi);
    let mut out = Vec::new();
    for (at, mark) in &core.specials {
        let Special::Break(kind) = mark else { continue };
        if *kind == BreakKind::Line {
            continue;
        }
        let Some(line) = lines.iter().rev().find(|line| line.range.start < *at || line.range.start == 0).filter(|line| *at <= line.range.end) else { continue };
        let x = line.body_glyphs().first().map(|g| g.offset[0]).unwrap_or(0.0);
        out.push(dtool_core::text_lines::BreakMark { kind: *kind, pos: *at, at: [pos[0] + x + line.width, pos[1] + line.baseline], size });
    }
    out
}

pub fn break_marks_with(lib: &FontLibrary, doc: &Document, id: ObjectId) -> Option<Vec<dtool_core::text_lines::BreakMark>> {
    let object = doc.get_object(id)?;
    match &object.kind {
        ObjectKind::FrameText { story } => {
            let core = doc.stories.iter().find(|s| s.id == *story)?;
            let flow = crate::frame::shared_doc_flow(lib, doc, *story)?;
            let index = flow.frames.iter().position(|f| *f == id)?;
            Some(break_marks_from(doc, core, flow.layout.per_frame.get(index)?, object.pos))
        }
        ObjectKind::ArtText { story } => {
            let core = doc.stories.iter().find(|s| s.id == *story)?;
            let layout = crate::frame::shared_artistic_layout(lib, doc, *story)?;
            Some(break_marks_from(doc, core, &layout.1, object.pos))
        }
        _ => None,
    }
}

fn break_marks_shared(doc: &Document, id: ObjectId) -> Option<Vec<dtool_core::text_lines::BreakMark>> {
    break_marks_with(&FontLibrary::shared(), doc, id)
}

fn text_lines_shared(doc: &Document, id: ObjectId) -> Option<Vec<TextLine>> {
    text_lines_with(&FontLibrary::shared(), doc, id)
}

pub fn overset_with(lib: &FontLibrary, doc: &Document, id: ObjectId) -> Option<Option<(usize, usize)>> {
    let object = doc.get_object(id)?;
    match &object.kind {
        ObjectKind::FrameText { story } => {
            let flow = crate::frame::shared_doc_flow(lib, previewed(doc, *story).as_ref(), *story)?;
            if !flow.frames.contains(&id) {
                return None;
            }
            Some(flow.layout.overset.map(|range| (range.start, range.end)))
        }
        ObjectKind::PathText(_) => {
            let (_, layout) = crate::path_text::path_text_layout_in_doc(lib, doc, object)?;
            Some(layout.overset.map(|range| (range.start, range.end)))
        }
        ObjectKind::ArtText { .. } => Some(None),
        _ => None,
    }
}

fn overset_shared(doc: &Document, id: ObjectId) -> Option<Option<(usize, usize)>> {
    overset_with(&FontLibrary::shared(), doc, id)
}

pub(crate) fn line_outlines(lib: &FontLibrary, story: &crate::story::Story, lines: &[Line], pos: [f32; 2], include: impl Fn(usize, usize, &Line) -> bool) -> Vec<Vec<[f32; 2]>> {
    let mut out = Vec::new();
    visit_outline_contours(lib, story, lines, pos, false, include, |_, contour| out.push(contour));
    out
}

fn visit_outline_contours(
    lib: &FontLibrary, story: &crate::story::Story, lines: &[Line], pos: [f32; 2],
    vertical: bool,
    include: impl Fn(usize, usize, &Line) -> bool,
    mut add: impl FnMut(&crate::story::CharStyle, Vec<[f32; 2]>),
) {
    let fallback = crate::story::CharStyle::default();
    let chars: Vec<char> = if vertical { story.text.chars().collect() } else { Vec::new() };
    for line in lines {
        let mut pen = 0.0;
        let mut rules = line.tab_rules.iter().peekable();
        let mut cluster_pivot = None;
        for (index, glyph) in line.glyphs.iter().enumerate() {
            let rule = rules.next_if(|(at, _)| *at + line.list_marker_len == index);
            let start = pen;
            pen += glyph.advance;
            if !include(glyph.cluster, index, line) {
                continue;
            }
            let style = story.chars.iter().rev().find(|run| run.range.start <= glyph.cluster && glyph.cluster < run.range.end).map(|run| &run.style).unwrap_or(&fallback);
            let origin = [pos[0] + start + glyph.offset[0], pos[1] + line.baseline + glyph.offset[1]];
            if let Some((_, rule)) = rule {
                let metrics = crate::decorations::line_metrics(lib, style);
                let (offset, thickness) = match rule {
                    TabRule::Underline => (metrics.underline_offset, metrics.underline_thickness),
                    TabRule::Strikeout => (metrics.strike_offset, metrics.strike_thickness),
                };
                let left = origin[0];
                let right = left + glyph.advance;
                let top = origin[1] - offset - thickness * 0.5;
                let bottom = top + thickness;
                add(style, vec![[left, top], [right, top], [right, bottom], [left, bottom]]);
                continue;
            }
            let [a, _, c, d, _, _] = crate::GlyphTransform::from_style(style).matrix();
            let ch = chars.get(glyph.cluster).copied();
            let alternate = ch.filter(|ch| matches!(unicode_vo::char_orientation(*ch),
                unicode_vo::Orientation::TransformedOrUpright | unicode_vo::Orientation::TransformedOrRotated))
                .and_then(|_| lib.vertical_alternate(glyph.font, glyph.gid));
            let upright = alternate.is_some() || ch.is_some_and(vertical_upright);
            let pivot = if upright {
                let centre = cluster_pivot.filter(|(cluster, _)| *cluster == glyph.cluster).map(|(_, centre)| centre).unwrap_or([
                    pos[0] + start + glyph.advance * 0.5,
                    pos[1] + line.baseline - style.size_pt * 0.35,
                ]);
                cluster_pivot = Some((glyph.cluster, centre));
                Some(centre)
            } else {
                None
            };
            for contour in lib.shared_glyph_contours(glyph.font, alternate.unwrap_or(glyph.gid), style.size_pt).iter() {
                add(style, contour.iter().map(|p| {
                    let [x, y] = [origin[0] + a * p[0] + c * p[1], origin[1] - d * p[1]];
                    pivot.map_or([x, y], |[cx, cy]| [cx + (y - cy), cy - (x - cx)])
                }).collect());
            }
        }
    }
}

fn line_ink(
    lib: &FontLibrary, story: &crate::story::Story, lines: &[Line], pos: [f32; 2],
    default: &dtool_core::paint::Paint, vertical: bool, include: impl Fn(usize, usize, &Line) -> bool,
) -> Option<Vec<dtool_core::text_lines::TextInk>> {
    use dtool_core::text_lines::TextInk;
    if !vertical && !story.chars.iter().any(|run| run.style.fill.as_ref().is_some_and(|paint| paint != default)) {
        return None;
    }
    let mut ink: Vec<TextInk> = Vec::new();
    visit_outline_contours(lib, story, lines, pos, vertical, include, |style, contour| {
        let paint = style.fill.as_ref().unwrap_or(default);
        TextInk::append(&mut ink, paint, contour);
    });
    Some(ink)
}

fn straight_text_layout(lib: &FontLibrary, doc: &Document, object: &dtool_core::Object) -> Option<Rc<(crate::story::Story, Vec<Line>)>> {
    match &object.kind {
        ObjectKind::ArtText { story } => {
            let view = previewed(doc, *story);
            crate::frame::shared_artistic_layout(lib, &view, *story)
        }
        ObjectKind::FrameText { story } => {
            let view = previewed(doc, *story);
            let doc = view.as_ref();
            let core = doc.stories.iter().find(|s| s.id == *story)?;
            let flow = crate::frame::shared_doc_flow(lib, doc, *story)?;
            let index = flow.frames.iter().position(|f| *f == object.id)?;
            let lines = flow.layout.per_frame.get(index)?.clone();
            Some(Rc::new((crate::bridge::story_in_document(core, doc), lines)))
        }
        _ => None,
    }
}

/// Baselines for formatting marks, using the same insertion positions as the text tool.
pub fn invisible_mark_positions(
    lib: &FontLibrary, doc: &Document, object: &dtool_core::Object, glyph: impl Fn(char) -> Option<char>,
) -> Option<Vec<(usize, char, [f32; 2], f32)>> {
    let layout = straight_text_layout(lib, doc, object)?;
    let (story, lines) = layout.as_ref();
    let chars: Vec<char> = story.text.chars().collect();
    let default_size = dtool_core::text_cmd::DEFAULT_SIZE_PT * dtool_core::units::units_per_point(doc.units, doc.dpi);
    let mut marks = Vec::new();
    for line in lines {
        let [x, y] = object.pos;
        let start = line.range.start.min(chars.len());
        let end = line.range.end.min(chars.len()).max(start);
        let mut add = |offset, mark| {
            let size = story.chars.iter().rev().find(|run| run.range.start <= offset && offset < run.range.end)
                .map_or(default_size, |run| run.style.size_pt);
            marks.push((offset, mark, [x + crate::frame::caret_x(line, offset), y + line.baseline], size));
        };
        for (offset, &ch) in (start..end).zip(&chars[start..end]) {
            if let Some(mark) = glyph(ch) {
                add(offset, mark);
            }
        }
        if chars.get(end) == Some(&'\n') {
            add(end, '\u{00B6}');
        }
    }
    Some(marks)
}

/// Formatting marks at visible insertion points on path text, after its object transform.
pub fn path_invisible_mark_positions(
    lib: &FontLibrary, doc: &Document, object: &dtool_core::Object, glyph: impl Fn(char) -> Option<char>,
) -> Vec<(char, [[f32; 2]; 2])> {
    let Some(story) = object.story_id().and_then(|id| doc.stories.iter().find(|story| story.id == id)) else {
        return Vec::new();
    };
    let chars: Vec<_> = story.text.chars().collect();
    let mut marks = std::collections::BTreeMap::new();
    for (offset, ends) in crate::path_text::path_text_insertions(lib, doc, object) {
        let Some(&ch) = chars.get(offset) else { continue; };
        let mark = if ch == '\n' { Some('\u{00B6}') } else { glyph(ch) };
        if let Some(mark) = mark {
            // At a shared boundary the following cluster supplies the mark's position.
            marks.insert(offset, (mark, ends));
        }
    }
    let mut marks: Vec<_> = marks.into_values().collect();
    let points: Vec<_> = marks.iter().flat_map(|(_, ends)| *ends).collect();
    if let Some(warped) = dtool_core::commands::warp::live_warp_points(doc, object.id, &points) {
        for ((_, ends), mapped) in marks.iter_mut().zip(warped.chunks_exact(2)) {
            ends.copy_from_slice(mapped);
        }
    }
    marks
}

/// Document-space quadrilaterals for contiguous visible fragments of a text range.
pub fn text_range_quads(lib: &FontLibrary, doc: &Document, object: &dtool_core::Object, range: std::ops::Range<usize>) -> Vec<[[f32; 2]; 4]> {
    let mut quads = Vec::new();
    if object.is_path_text() {
        let points = crate::path_text::path_text_insertions(lib, doc, object);
        for edge in points.windows(2) {
            let [(start, [a, b]), (end, [c, d])] = [edge[0], edge[1]];
            if end == start + 1 && range.contains(&start) {
                quads.push([a, c, d, b]);
            }
        }
        return quads;
    }
    let Some(layout) = straight_text_layout(lib, doc, object) else { return quads };
    let (story, lines) = layout.as_ref();
    let matrix = dtool_core::geometry::text_matrix(object);
    let at = |x, y| dtool_core::geometry::apply_matrix(&matrix, [object.pos[0] + x, object.pos[1] + y]);
    let default_size = dtool_core::text_cmd::DEFAULT_SIZE_PT * dtool_core::units::units_per_point(doc.units, doc.dpi);
    for line in lines.iter().filter(|line| range.start < line.range.end && line.range.start < range.end) {
        let mut stops: Vec<_> = line.body_glyphs().iter().map(|glyph| glyph.cluster).collect();
        stops.push(line.range.end);
        stops.sort_unstable();
        stops.dedup();
        let mut pen = 0.0;
        let mut fragment: Option<[f32; 4]> = None;
        let mut flush = |fragment: [f32; 4]| {
            let [left, top, right, bottom] = fragment;
            quads.push([at(left, top), at(right, top), at(right, bottom), at(left, bottom)]);
        };
        for cluster in line.body_glyphs().chunk_by(|a, b| a.cluster == b.cluster) {
            let start = cluster[0].cluster;
            let advance = cluster.iter().map(|glyph| glyph.advance).sum::<f32>();
            let x = pen + cluster[0].offset[0];
            pen += advance;
            let end = stops.get(stops.partition_point(|stop| *stop <= start)).copied().unwrap_or(start);
            let low = range.start.max(start);
            let high = range.end.min(end);
            if low >= high || advance.abs() <= f32::EPSILON {
                if low >= high && let Some(bounds) = fragment.take() { flush(bounds); }
                continue;
            }
            let step = advance / (end - start) as f32;
            let a = x + (low - start) as f32 * step;
            let b = x + (high - start) as f32 * step;
            let size = story.chars.iter().rev().find(|run| run.range.start <= start && start < run.range.end)
                .map_or(default_size, |run| run.style.size_pt);
            let bounds = [a.min(b), line.baseline - size * 0.85, a.max(b), line.baseline + size * 0.25];
            if let Some(current) = &mut fragment {
                current[0] = current[0].min(bounds[0]);
                current[1] = current[1].min(bounds[1]);
                current[2] = current[2].max(bounds[2]);
                current[3] = current[3].max(bounds[3]);
            } else {
                fragment = Some(bounds);
            }
        }
        if let Some(bounds) = fragment { flush(bounds); }
    }
    if let Some(points) = dtool_core::commands::warp::live_warp_points(doc, object.id, &quads.iter().flatten().copied().collect::<Vec<_>>()) {
        for (quad, mapped) in quads.iter_mut().zip(points.chunks_exact(4)) {
            quad.copy_from_slice(mapped);
        }
    }
    quads
}

/// Top of the insertion caret at a story offset on this text object.
pub fn text_anchor_point(lib: &FontLibrary, doc: &Document, object: &dtool_core::Object, offset: usize) -> Option<[f32; 2]> {
    let point = if object.is_path_text() {
        crate::path_text::path_text_insertions(lib, doc, object).into_iter()
            .find_map(|(index, [top, _])| (index == offset).then_some(top))?
    } else {
        let layout = straight_text_layout(lib, doc, object)?;
        let (story, lines) = layout.as_ref();
        let line = lines.iter().rev().find(|line| line.range.start <= offset && offset <= line.range.end)?;
        let size = story.chars.iter().rev().find(|run| run.range.start <= offset && offset < run.range.end)
            .map_or_else(|| dtool_core::text_cmd::DEFAULT_SIZE_PT * dtool_core::units::units_per_point(doc.units, doc.dpi), |run| run.style.size_pt);
        let location = [object.pos[0] + crate::frame::caret_x(line, offset), object.pos[1] + line.baseline - size * 0.85];
        dtool_core::geometry::apply_matrix(&dtool_core::geometry::text_matrix(object), location)
    };
    Some(dtool_core::commands::warp::live_warp_points(doc, object.id, &[point])
        .and_then(|points| points.first().copied()).unwrap_or(point))
}

pub fn text_outlines_with(lib: &FontLibrary, doc: &Document, id: ObjectId) -> Option<Vec<Vec<[f32; 2]>>> {
    let object = doc.get_object(id)?;
    if matches!(object.kind, ObjectKind::PathText(_)) {
        return Some(crate::path_text::path_text_contours(lib, doc, object));
    }
    let layout = straight_text_layout(lib, doc, object)?;
    let (story, lines) = layout.as_ref();
    let mut out = Vec::new();
    visit_outline_contours(lib, story, lines, object.pos, object.is_vertical_art_text(), |_, _, _| true, |_, contour| out.push(contour));
    let matrix = dtool_core::geometry::text_matrix(object);
    if matrix != dtool_core::geometry::IDENTITY {
        for point in out.iter_mut().flatten() {
            *point = dtool_core::geometry::apply_matrix(&matrix, *point);
        }
    }
    Some(out)
}

pub struct ExportTextGlyph {
    pub id: u16,
    pub origin: [f32; 2],
    pub advance: f32,
    pub text_range: std::ops::Range<usize>,
}

pub struct ExportTextSpan {
    pub glyphs: Vec<ExportTextGlyph>,
    pub origin: [f32; 2],
    pub text: String,
    pub style: crate::story::CharStyle,
    pub font: crate::fonts::FontId,
    pub width: f32,
}

pub struct TextExportLayout {
    pub spans: Vec<ExportTextSpan>,
    pub contours: Vec<Vec<[f32; 2]>>,
    pub outline_ink: Option<Vec<dtool_core::text_lines::TextInk>>,
    pub paint: dtool_core::paint::Paint,
    pub matrix: [f32; 6],
}

/// Export the lines owned by this layer in layout coordinates, with their document transform.
pub fn text_export_layout(lib: &FontLibrary, doc: &Document, object: &dtool_core::Object, outlined: bool, tabbed_only: bool) -> Option<TextExportLayout> {
    if matches!(object.kind, ObjectKind::PathText(_)) {
        let outline_ink = crate::path_text::path_text_ink(lib, doc, object);
        let contours = if outline_ink.is_some() {
            Vec::new()
        } else {
            crate::path_text::export_path_text_contours(lib, doc, object)?
        };
        return Some(TextExportLayout {
            spans: Vec::new(),
            contours,
            outline_ink,
            paint: crate::path_text::path_text_paint(doc, object),
            matrix: dtool_core::geometry::IDENTITY,
        });
    }
    let story_id = match object.kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => story,
        _ => return None,
    };
    let core = doc.stories.iter().find(|story| story.id == story_id)?;
    let paint = story_paint(core);
    let vertical = object.is_vertical_art_text();
    let outlined = outlined || vertical;
    if tabbed_only && !vertical && !core.text.contains('\t') && !has_local_text_fill(core, &paint)
        && !core.paras.iter().any(|range| range["paragraph_decorations"].as_array().is_some_and(|items| !items.is_empty()))
    {
        return None;
    }
    let layout = straight_text_layout(lib, doc, object)?;
    let (story, lines) = layout.as_ref();
    let chars: Vec<char> = story.text.chars().collect();
    let include = |cluster, index, line: &Line| {
        outlined || index < line.list_marker_len || chars.get(cluster) == Some(&'\t')
    };
    let outline_ink = line_ink(lib, story, lines, object.pos, &paint, vertical, include);
    let contours = if outline_ink.is_none() { line_outlines(lib, story, lines, object.pos, include) } else { Vec::new() };
    let spans = if outlined { Vec::new() } else { export_text_spans(story, &chars, lines, object.pos) };
    Some(TextExportLayout { spans, contours, outline_ink, paint, matrix: dtool_core::geometry::text_matrix(object) })
}

fn export_text_spans(story: &crate::story::Story, chars: &[char], lines: &[Line], pos: [f32; 2]) -> Vec<ExportTextSpan> {
    let mut spans = Vec::new();
    for line in lines {
        let start = line.range.start.min(chars.len());
        let end = line.range.end.min(chars.len()).max(start);
        let mut positions = vec![None; end - start];
        let mut cluster_glyphs = vec![Vec::new(); end - start];
        let mut pen = 0.0;
        for (index, glyph) in line.glyphs.iter().enumerate() {
            if index >= line.list_marker_len && (start..end).contains(&glyph.cluster) {
                let slot = &mut positions[glyph.cluster - start];
                let x = pos[0] + pen + glyph.offset[0];
                let y = pos[1] + line.baseline + glyph.offset[1];
                let (_, _, right, _) = slot.get_or_insert((x, y, x, glyph.font));
                *right = right.max(x + glyph.advance);
                cluster_glyphs[glyph.cluster - start].push((index, glyph, [x, y]));
            }
            pen += glyph.advance;
        }
        let mut offset = start;
        for piece in chars[start..end].split(|character| matches!(character, '\t' | '\n')) {
            let piece_start = offset;
            offset += piece.len() + 1;
            if piece.is_empty() {
                continue;
            }
            let mut span_start = piece_start;
            let piece_end = piece_start + piece.len();
            while span_start < piece_end {
                let run = story.chars.iter().rev().find(|run| (run.range.start..run.range.end).contains(&span_start));
                let next_run = story.chars.iter().filter(|run| run.range.start > span_start).map(|run| run.range.start).min().unwrap_or(piece_end);
                let style_end = next_run.min(run.map_or(piece_end, |run| run.range.end)).min(piece_end);
                let positions_for_style = &positions[span_start - start..style_end - start];
                let Some(font) = positions_for_style.iter().flatten().next().map(|glyph| glyph.3) else {
                    span_start = style_end;
                    continue;
                };
                let span_length = positions_for_style.iter()
                    .position(|glyph| glyph.is_some_and(|glyph| glyph.3 != font))
                    .unwrap_or(positions_for_style.len());
                let span_end = span_start + span_length;
                let glyphs = positions_for_style[..span_length].iter().flatten();
                if let Some(&(x, y, _, _)) = glyphs.clone().min_by(|a, b| a.0.total_cmp(&b.0)) {
                    let right = glyphs.map(|glyph| glyph.2).fold(x, f32::max);
                    let style = run.map(|run| run.style.clone()).unwrap_or_default();
                    let text: String = chars[span_start..span_end].iter().collect();
                    let clusters = &cluster_glyphs[span_start - start..span_end - start];
                    let glyphs = export_span_glyphs(&text, clusters, font);
                    spans.push(ExportTextSpan { origin: [x, y], text, style, font, width: right - x, glyphs });
                }
                span_start = span_end;
            }
        }
    }
    spans
}

fn export_span_glyphs(text: &str, clusters: &[Vec<(usize, &crate::ShapedGlyph, [f32; 2])>], font: crate::FontId) -> Vec<ExportTextGlyph> {
    let mut placed = Vec::new();
    let mut end = text.len();
    for ((start, _), cluster) in text.char_indices().rev().zip(clusters.iter().rev()) {
        if cluster.is_empty() {
            continue;
        }
        for &(index, glyph, origin) in cluster {
            if glyph.font != font {
                return Vec::new();
            }
            placed.push((index, ExportTextGlyph { id: glyph.gid, origin, advance: glyph.advance, text_range: start..end }));
        }
        end = start;
    }
    placed.sort_unstable_by_key(|(index, _)| *index);
    placed.into_iter().map(|(_, glyph)| glyph).collect()
}

fn text_outlines_shared(doc: &Document, id: ObjectId) -> Option<Vec<Vec<[f32; 2]>>> {
    text_outlines_with(&FontLibrary::shared(), doc, id)
}

pub(crate) fn has_local_text_fill(core: &dtool_core::Story, default: &dtool_core::paint::Paint) -> bool {
    core.ranges.iter().any(|range| range.get("fill").is_some_and(|fill| {
        dtool_core::serde_json::from_value::<dtool_core::paint::Paint>(fill.clone()).is_ok_and(|fill| &fill != default)
    })) || core.paras.iter().any(|range| range["initial_words"]["on"] == true)
}

fn text_ink_shared(doc: &Document, id: ObjectId) -> Option<Vec<dtool_core::text_lines::TextInk>> {
    let object = doc.get_object(id)?;
    let core = doc.stories.iter().find(|core| Some(core.id) == object.story_id())?;
    if object.is_path_text() {
        return crate::path_text::path_text_ink(&FontLibrary::shared(), doc, object);
    }
    let paint = story_paint(core);
    let vertical = object.is_vertical_art_text();
    if !vertical && !has_local_text_fill(core, &paint) {
        return None;
    }
    let lib = FontLibrary::shared();
    let layout = straight_text_layout(&lib, doc, object)?;
    let (story, lines) = layout.as_ref();
    let mut ink = line_ink(&lib, story, lines, object.pos, &paint, vertical, |_, _, _| true)?;
    let matrix = dtool_core::geometry::text_matrix(object);
    for point in ink.iter_mut().flat_map(|run| run.contours.iter_mut().flatten()) {
        *point = dtool_core::geometry::apply_matrix(&matrix, *point);
    }
    Some(ink)
}

pub fn install() -> bool {
    let lines = register_text_lines(text_lines_shared);
    let outlines = dtool_core::text_lines::register_text_outlines(text_outlines_shared);
    let ink = dtool_core::text_lines::register_text_ink(text_ink_shared);
    let tables = dtool_core::query::register_table_overflow(crate::table_layout::table_overflow_shared);
    let table_lines = dtool_core::table::query::register_table_lines(crate::table_layout::table_lines_shared);
    let table_fit = dtool_core::table::fit::register_measure(crate::table_layout::measure_for_fit);
    let decorations = dtool_core::text_lines::register_text_decorations(crate::decorations::text_decorations_shared);
    let faces = crate::text_faces::install();
    let breaks = dtool_core::text_lines::register_text_break_marks(break_marks_shared);
    register_overset(overset_shared) && lines && tables && table_lines && table_fit && outlines && ink && decorations && faces && breaks
}
