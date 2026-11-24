use crate::char_style::{background_paint, strikethrough_offset, strikethrough_paint, strikethrough_weight, underline_offset, underline_paint, underline_weight};
use crate::fonts::{FontEngine, FontLibrary};
use crate::story::{CharStyle, Line, Story, TextRange};
use dtool_core::paint::Paint;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub underline_offset: f32,
    pub underline_thickness: f32,
    pub strike_offset: f32,
    pub strike_thickness: f32,
    pub actual: bool,
}

impl LineMetrics {
    pub fn approximate(size_pt: f32) -> Self {
        Self {
            ascent: size_pt * 0.8,
            descent: size_pt * 0.2,
            underline_offset: -size_pt * 0.1,
            underline_thickness: size_pt * 0.05,
            strike_offset: size_pt * 0.25,
            strike_thickness: size_pt * 0.05,
            actual: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationKind {
    Background,
    Underline,
    Strike,
    ParagraphRule,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecorationRect {
    pub kind: DecorationKind,
    pub range: TextRange,
    pub rect: [f32; 4],
    pub paint: Paint,
}

pub fn line_metrics(lib: &FontLibrary, style: &CharStyle) -> LineMetrics {
    use skrifa::instance::{LocationRef, Size};
    use skrifa::MetadataProvider;
    let size_pt = style.size_pt;
    if !size_pt.is_finite() || size_pt <= 0.0 {
        return LineMetrics::approximate(12.0);
    }
    let fallback = LineMetrics::approximate(size_pt);
    let Some(face) = lib.resolve(&style.family, &style.style).or_else(|| lib.resolve(&style.family, "Regular")).and_then(|id| lib.face(id)) else {
        return fallback;
    };
    let blob = {
        let mut engine = lib.engine.lock().unwrap_or_else(|e| e.into_inner());
        let FontEngine { fonts, .. } = &mut *engine;
        let Some(family) = fonts.collection.family_by_name(&face.family) else {
            return fallback;
        };
        let mut found = None;
        for info in family.fonts() {
            if info.index() != face.index {
                continue;
            }
            let same_path = match info.source().kind() {
                parley::fontique::SourceKind::Path(path) => path.as_ref() == face.path.as_path(),
                _ => face.path.as_os_str().is_empty(),
            };
            if same_path {
                found = info.load(Some(&mut fonts.source_cache));
                break;
            }
        }
        match found {
            Some(blob) => blob,
            None => return fallback,
        }
    };
    let Ok(font) = skrifa::FontRef::from_index(blob.data(), face.index) else {
        return fallback;
    };
    let metrics = font.metrics(Size::new(size_pt), LocationRef::default());
    let finite = |value: f32| value.is_finite();
    let underline = metrics.underline.filter(|d| finite(d.offset) && finite(d.thickness) && d.thickness > 0.0);
    let strike = metrics.strikeout.filter(|d| finite(d.offset) && finite(d.thickness) && d.thickness > 0.0);
    LineMetrics {
        ascent: if finite(metrics.ascent) && metrics.ascent > 0.0 { metrics.ascent } else { fallback.ascent },
        descent: if finite(metrics.descent) && metrics.descent != 0.0 { metrics.descent.abs() } else { fallback.descent },
        underline_offset: underline.map(|d| d.offset).unwrap_or(fallback.underline_offset),
        underline_thickness: underline.map(|d| d.thickness).unwrap_or(fallback.underline_thickness),
        strike_offset: strike.map(|d| d.offset).unwrap_or(fallback.strike_offset),
        strike_thickness: strike.map(|d| d.thickness).unwrap_or(fallback.strike_thickness),
        actual: underline.is_some() && strike.is_some(),
    }
}

fn glyph_spans(line: &Line) -> Vec<(usize, f32, f32)> {
    let glyphs = line.body_glyphs();
    let mut pen = 0.0f32;
    let mut spans = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        let x = pen + glyph.offset[0];
        spans.push((glyph.cluster, x, x + glyph.advance));
        pen += glyph.advance;
    }
    spans
}

fn segment_x(spans: &[(usize, f32, f32)], range: TextRange) -> Option<(f32, f32)> {
    let mut extent: Option<(f32, f32)> = None;
    for (index, x0, x1) in spans {
        if *index < range.start || *index >= range.end {
            continue;
        }
        let (a, b) = if x0 <= x1 { (*x0, *x1) } else { (*x1, *x0) };
        extent = Some(match extent {
            Some((left, right)) => (left.min(a), right.max(b)),
            None => (a, b),
        });
    }
    extent.filter(|(left, right)| right > left)
}

fn paragraph_decorations(lib: &FontLibrary, story: &Story, lines: &[Line]) -> Vec<DecorationRect> {
    if !story.paras.iter().any(|run| !run.style.paragraph_decorations.is_empty()) {
        return Vec::new();
    }
    let starts: Vec<_> = std::iter::once(0).chain(story.text.chars().enumerate()
        .filter(|(_, ch)| matches!(ch, '\n' | '\u{2029}')).map(|(index, _)| index + 1)).collect();
    let mut paragraphs = std::collections::BTreeMap::<usize, [f32; 4]>::new();
    for line in lines {
        let start = starts[starts.partition_point(|&start| start <= line.range.start).saturating_sub(1)];
        if !crate::layout::para_run_at(story, start).is_some_and(|style| !style.paragraph_decorations.is_empty()) {
            continue;
        }
        let Some((left, right)) = segment_x(&glyph_spans(line), line.range) else { continue };
        let fallback = CharStyle::default();
        let style = story.chars.iter().filter(|run| run.range.start < line.range.end && line.range.start < run.range.end)
            .max_by(|a, b| a.style.size_pt.total_cmp(&b.style.size_pt))
            .map_or(&fallback, |run| &run.style);
        let metrics = line_metrics(lib, style);
        let top = line.baseline - metrics.ascent;
        let bottom = line.baseline + metrics.descent;
        paragraphs.entry(start).and_modify(|rect| {
            *rect = [rect[0].min(left), rect[1].min(top), rect[2].max(right), rect[3].max(bottom)];
        }).or_insert([left, top, right, bottom]);
    }
    let mut result = Vec::new();
    let scale = story.layout_units_per_point.unwrap_or(1.0);
    for (start, [left, top, right, bottom]) in paragraphs {
        let Some(para) = crate::layout::para_run_at(story, start) else { continue };
        for decoration in &para.paragraph_decorations {
            let pad = decoration.padding.max(0.0) * scale;
            let width = decoration.width.max(0.0) * scale;
            let [x, y, w, h] = [left - pad, top - pad, right - left + 2.0 * pad, bottom - top + 2.0 * pad];
            let range = TextRange::new(start, start);
            if let Some(colour) = decoration.fill {
                result.push(DecorationRect { kind: DecorationKind::Background, range, rect: [x, y, w, h], paint: Paint::Solid { colour } });
            }
            if width > 0.0 {
                let vertical = width.min(w);
                let horizontal = width.min(h);
                let [_, top_on, _, bottom_on] = decoration.sides;
                let y0 = y + if top_on { horizontal } else { 0.0 };
                let inner_h = (h - (y0 - y) - if bottom_on { horizontal } else { 0.0 }).max(0.0);
                for (on, rect) in decoration.sides.into_iter().zip([
                    [x, y0, vertical, inner_h], [x, y, w, horizontal],
                    [x + w - vertical, y0, vertical, inner_h], [x, y + h - horizontal, w, horizontal],
                ]) {
                    if on && rect[2] > 0.0 && rect[3] > 0.0 {
                        result.push(DecorationRect { kind: DecorationKind::ParagraphRule, range, rect, paint: Paint::Solid { colour: decoration.stroke } });
                    }
                }
            }
        }
    }
    result
}

pub fn line_decorations(lib: &FontLibrary, story: &Story, lines: &[Line], text_fill: &Paint) -> Vec<DecorationRect> {
    let mut backgrounds = Vec::new();
    let mut strokes = Vec::new();
    for line in lines {
        let spans = glyph_spans(line);
        if spans.is_empty() {
            continue;
        }
        for run in &story.chars {
            let start = run.range.start.max(line.range.start);
            let end = run.range.end.min(line.range.end);
            if start >= end {
                continue;
            }
            let style = &run.style;
            let has_background = background_paint(style).is_some();
            if !has_background && !style.underline.on && !style.strike.on {
                continue;
            }
            let range = TextRange::new(start, end);
            let Some((x0, x1)) = segment_x(&spans, range) else { continue };
            let metrics = line_metrics(lib, style);
            let fill = style.fill.clone().unwrap_or_else(|| text_fill.clone());
            if let Some(paint) = background_paint(style) {
                backgrounds.push(DecorationRect {
                    kind: DecorationKind::Background,
                    range,
                    rect: [x0, line.baseline - metrics.ascent, x1 - x0, metrics.ascent + metrics.descent],
                    paint,
                });
            }
            if style.underline.on {
                let thickness = underline_weight(&style.underline, metrics.underline_thickness).max(0.0);
                let offset = underline_offset(&style.underline, metrics.underline_offset);
                strokes.push(DecorationRect {
                    kind: DecorationKind::Underline,
                    range,
                    rect: [x0, line.baseline - offset - thickness * 0.5, x1 - x0, thickness],
                    paint: underline_paint(&style.underline, &fill),
                });
            }
            if style.strike.on {
                let thickness = strikethrough_weight(&style.strike, metrics.strike_thickness).max(0.0);
                let offset = strikethrough_offset(&style.strike, metrics.strike_offset);
                strokes.push(DecorationRect {
                    kind: DecorationKind::Strike,
                    range,
                    rect: [x0, line.baseline - offset - thickness * 0.5, x1 - x0, thickness],
                    paint: strikethrough_paint(&style.strike, &fill),
                });
            }
        }
    }
    let mut paragraph = paragraph_decorations(lib, story, lines);
    paragraph.extend(backgrounds);
    paragraph.extend(strokes);
    paragraph
}

pub fn story_decorations(lib: &FontLibrary, story: &Story, width: Option<f32>, text_fill: &Paint) -> Vec<DecorationRect> {
    let lines = crate::layout::layout_story(lib, story, width);
    line_decorations(lib, story, &lines, text_fill)
}


pub fn object_decorations_with(lib: &FontLibrary, doc: &dtool_core::Document, id: dtool_core::object::ObjectId) -> Vec<DecorationRect> {
    use dtool_core::object::ObjectKind;
    let Some(object) = doc.get_object(id) else { return Vec::new() };
    let story_id = match &object.kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => *story,
        ObjectKind::PathText(data) => data.story,
        _ => return Vec::new(),
    };
    let Some(core) = doc.stories.iter().find(|s| s.id == story_id) else { return Vec::new() };
    let decorated = core.ranges.iter().any(|range| {
        let on = |key: &str| range.get(key).and_then(|value| value.get("on")).and_then(|value| value.as_bool()).unwrap_or(false);
        on("underline") || on("strike") || range.get("background").is_some_and(|value| !value.is_null())
    });
    let paragraph = core.paras.iter().any(|range| range["paragraph_decorations"].as_array().is_some_and(|items| !items.is_empty()));
    if !decorated && !paragraph {
        return Vec::new();
    }
    if matches!(object.kind, ObjectKind::PathText(_)) {
        let fill = if matches!(object.fill, Paint::None) { dtool_core::text_lines::story_paint(core) } else { object.fill.clone() };
        return path_decorations(lib, doc, object, &fill);
    }
    let story = crate::bridge::story_in_document(core, doc);
    let fill = dtool_core::text_lines::story_paint(core);
    let mut rects = if matches!(object.kind, ObjectKind::FrameText { .. }) {
        let Some(flow) = crate::frame::shared_doc_flow(lib, doc, story_id) else { return Vec::new() };
        let Some(lines) = flow.frames.iter().position(|f| *f == id).and_then(|index| flow.layout.per_frame.get(index)) else { return Vec::new() };
        line_decorations(lib, &story, lines, &fill)
    } else {
        story_decorations(lib, &story, None, &fill)
    };
    for rect in &mut rects {
        rect.rect[0] += object.pos[0];
        rect.rect[1] += object.pos[1];
    }
    rects
}

fn along_glyph(out: &mut Vec<DecorationRect>, placed: &crate::path_text::PlacedGlyph, centre: f32, thickness: f32, kind: DecorationKind, paint: &Paint) {
    let advance = placed.glyph.advance.abs();
    if !(advance > 0.0) || !(thickness > 0.0) || !centre.is_finite() {
        return;
    }
    let (sin, cos) = placed.angle.sin_cos();
    let side = if placed.flipped { -1.0 } else { 1.0 };
    let local_y = -side * centre;
    let pieces = ((advance / thickness.max(0.5)).ceil() as usize).clamp(1, 64);
    let piece = advance / pieces as f32;
    let w = piece * cos.abs() + thickness * sin.abs();
    let h = piece * sin.abs() + thickness * cos.abs();
    for index in 0..pieces {
        let x = (index as f32 + 0.5) * piece;
        let at = [placed.position[0] + x * cos - local_y * sin, placed.position[1] + x * sin + local_y * cos];
        out.push(DecorationRect { kind, range: placed.cluster, rect: [at[0] - w * 0.5, at[1] - h * 0.5, w, h], paint: paint.clone() });
    }
}

fn path_decorations(lib: &FontLibrary, doc: &dtool_core::Document, object: &dtool_core::object::Object, text_fill: &Paint) -> Vec<DecorationRect> {
    let Some((story, layout)) = crate::path_text::path_text_layout_in_doc(lib, doc, object) else { return Vec::new() };
    let mut backgrounds = Vec::new();
    let mut strokes = Vec::new();
    for placed in &layout.glyphs {
        let Some(run) = story.chars.iter().rev().find(|run| run.range.start <= placed.cluster.start && placed.cluster.start < run.range.end) else { continue };
        let style = &run.style;
        let background = background_paint(style);
        if background.is_none() && !style.underline.on && !style.strike.on {
            continue;
        }
        let metrics = line_metrics(lib, style);
        let fill = style.fill.clone().unwrap_or_else(|| text_fill.clone());
        if let Some(paint) = background {
            along_glyph(&mut backgrounds, placed, (metrics.ascent - metrics.descent) * 0.5, metrics.ascent + metrics.descent, DecorationKind::Background, &paint);
        }
        if style.underline.on {
            let thickness = underline_weight(&style.underline, metrics.underline_thickness).max(0.0);
            along_glyph(&mut strokes, placed, underline_offset(&style.underline, metrics.underline_offset), thickness, DecorationKind::Underline, &underline_paint(&style.underline, &fill));
        }
        if style.strike.on {
            let thickness = strikethrough_weight(&style.strike, metrics.strike_thickness).max(0.0);
            along_glyph(&mut strokes, placed, strikethrough_offset(&style.strike, metrics.strike_offset), thickness, DecorationKind::Strike, &strikethrough_paint(&style.strike, &fill));
        }
    }
    backgrounds.extend(strokes);
    backgrounds
}

pub fn object_decorations(doc: &dtool_core::Document, id: dtool_core::object::ObjectId) -> Vec<DecorationRect> {
    object_decorations_with(&FontLibrary::shared(), doc, id)
}

pub fn text_decorations_shared(doc: &dtool_core::Document, id: dtool_core::object::ObjectId) -> Option<Vec<dtool_core::text_lines::TextDecoration>> {
    let rects = object_decorations(doc, id);
    Some(rects.into_iter().map(|d| dtool_core::text_lines::TextDecoration { rect: d.rect, paint: d.paint, under_text: d.kind == DecorationKind::Background }).collect())
}
