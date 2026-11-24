use crate::char_style::{position_scale, Caps, Kerning, Position};
use crate::fonts::{FontEngine, FontId, FontLibrary, UNMAPPED_FONT};
use crate::story::CharStyle;
use parley::{FontFamily, FontFeatures, FontStyle, FontWeight, FontWidth, Language, Layout, StyleProperty};

#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub font: FontId,
    pub gid: u16,
    pub cluster: usize,
    pub advance: f32,
    pub offset: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphTransform {
    pub h_scale: f32,
    pub v_scale: f32,
    pub shear_deg: f32,
    pub baseline_shift: f32,
}

impl Default for GlyphTransform {
    fn default() -> Self {
        Self { h_scale: 1.0, v_scale: 1.0, shear_deg: 0.0, baseline_shift: 0.0 }
    }
}

impl GlyphTransform {
    pub fn from_style(style: &CharStyle) -> Self {
        let (position_factor, position_shift) = if style.size_pt.is_finite() && style.size_pt > 0.0 {
            position_scale(style)
        } else {
            (1.0, 0.0)
        };
        let factor = if position_factor.is_finite() && position_factor > 0.0 { position_factor } else { 1.0 };
        let h_scale = if style.h_scale.is_finite() && style.h_scale > 0.0 { style.h_scale } else { 1.0 };
        let v_scale = if style.v_scale.is_finite() && style.v_scale > 0.0 { style.v_scale } else { 1.0 };
        let mut shear_deg = if style.shear_deg.is_finite() { style.shear_deg.clamp(-89.0, 89.0) } else { 0.0 };
        if style.italic && shear_deg == 0.0 && !style.style.to_lowercase().contains("italic") && !style.style.to_lowercase().contains("oblique") {
            shear_deg = SYNTHETIC_ITALIC_DEG;
        }
        let baseline = if style.baseline_pt.is_finite() { style.baseline_pt } else { 0.0 };
        Self {
            h_scale: h_scale * factor,
            v_scale: v_scale * factor,
            shear_deg,
            baseline_shift: baseline + position_shift,
        }
    }

    pub fn is_identity(&self) -> bool {
        self.h_scale == 1.0 && self.v_scale == 1.0 && self.shear_deg == 0.0 && self.baseline_shift == 0.0
    }

    pub fn shear_factor(&self) -> f32 {
        self.shear_deg.to_radians().tan()
    }

    pub fn matrix(&self) -> [f32; 6] {
        let shear = self.shear_factor();
        [self.h_scale, 0.0, shear * self.v_scale, self.v_scale, 0.0, -self.baseline_shift]
    }

    pub fn glyph_matrix(&self, origin: [f32; 2]) -> [f32; 6] {
        let m = self.matrix();
        [m[0], m[1], m[2], m[3], origin[0] + m[4], origin[1] + m[5]]
    }

    pub fn apply_point(&self, point: [f32; 2]) -> [f32; 2] {
        let m = self.matrix();
        [m[0] * point[0] + m[2] * point[1] + m[4], m[1] * point[0] + m[3] * point[1] + m[5]]
    }

    pub fn advance(&self, advance: f32) -> f32 {
        advance * self.h_scale
    }
}

pub const SYNTHETIC_ITALIC_DEG: f32 = 12.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeOptions {
    pub language: Option<String>,
    pub features: Vec<(String, u32)>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub transform: GlyphTransform,
}

fn valid_tag(tag: &str) -> bool {
    tag.len() == 4 && tag.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
}

fn push_feature(list: &mut Vec<(String, u32)>, tag: &str, value: u32) {
    if !valid_tag(tag) {
        return;
    }
    if let Some(slot) = list.iter_mut().find(|(t, _)| t == tag) {
        slot.1 = value;
    } else {
        list.push((tag.to_string(), value));
    }
}

impl ShapeOptions {
    pub fn from_style(style: &CharStyle) -> Self {
        let mut features: Vec<(String, u32)> = Vec::new();
        match style.ligatures {
            Some(true) => {
                push_feature(&mut features, "liga", 1);
                push_feature(&mut features, "clig", 1);
            }
            Some(false) => {
                push_feature(&mut features, "liga", 0);
                push_feature(&mut features, "clig", 0);
                push_feature(&mut features, "dlig", 0);
            }
            None => {}
        }
        match style.caps {
            Caps::None | Caps::AllCaps => {}
            Caps::SmallCaps => push_feature(&mut features, "smcp", 1),
            Caps::AllSmallCaps => {
                push_feature(&mut features, "smcp", 1);
                push_feature(&mut features, "c2sc", 1);
            }
            Caps::PetiteCaps => push_feature(&mut features, "pcap", 1),
        }
        match style.kerning {
            Kerning::Auto | Kerning::Optical => push_feature(&mut features, "kern", 1),
            Kerning::Manual(_) => push_feature(&mut features, "kern", 0),
        }
        match style.position {
            Position::Superscript if crate::char_style::has_opentype_sups(style) => push_feature(&mut features, "sups", 1),
            Position::Subscript if crate::char_style::has_opentype_subs(style) => push_feature(&mut features, "subs", 1),
            _ => {}
        }
        for (tag, value) in &style.opentype {
            push_feature(&mut features, tag, *value);
        }
        let language = style.language.trim();
        Self {
            language: if language.is_empty() { None } else { Some(language.to_string()) },
            features,
            weight: Some(style.weight),
            italic: Some(style.italic),
            transform: GlyphTransform::from_style(style),
        }
    }

    pub fn feature_source(&self) -> String {
        self.features
            .iter()
            .filter(|(tag, _)| valid_tag(tag))
            .map(|(tag, value)| format!("\"{}\" {}", tag, value))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn feature(&self, tag: &str) -> Option<u32> {
        self.features.iter().find(|(t, _)| t == tag).map(|(_, v)| *v)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub language: Option<String>,
    pub features: Vec<(String, u32)>,
    pub transform: GlyphTransform,
}

impl ShapedRun {
    pub fn transformed_width(&self) -> f32 {
        self.transform.advance(self.width)
    }

    pub fn transformed_ascent(&self) -> f32 {
        self.ascent * self.transform.v_scale + self.transform.baseline_shift.max(0.0)
    }

    pub fn transformed_descent(&self) -> f32 {
        self.descent * self.transform.v_scale + (-self.transform.baseline_shift).max(0.0)
    }

    pub fn glyph_matrices(&self, origin: [f32; 2]) -> Vec<[f32; 6]> {
        let mut pen = 0.0f32;
        let mut out = Vec::with_capacity(self.glyphs.len());
        for glyph in &self.glyphs {
            let local = [pen + glyph.offset[0], glyph.offset[1]];
            let placed = self.transform.apply_point(local);
            let m = self.transform.matrix();
            out.push([m[0], m[1], m[2], m[3], origin[0] + placed[0], origin[1] + placed[1]]);
            pen += glyph.advance;
        }
        out
    }
}

pub fn letter_spacing_pt(style: &CharStyle) -> f32 {
    if !style.size_pt.is_finite() || style.size_pt <= 0.0 {
        return 0.0;
    }
    let tracking = if style.tracking.is_finite() { style.tracking } else { 0.0 };
    let manual = match style.kerning {
        Kerning::Manual(value) if value.is_finite() => value,
        _ => 0.0,
    };
    (tracking + manual) / 1000.0 * style.size_pt
}

pub fn apply_letter_spacing(glyphs: &mut [ShapedGlyph], spacing: f32) -> f32 {
    if spacing == 0.0 || !spacing.is_finite() || glyphs.len() < 2 {
        return 0.0;
    }
    let mut added = 0.0;
    for i in 0..glyphs.len() - 1 {
        if glyphs[i + 1].cluster != glyphs[i].cluster {
            glyphs[i].advance += spacing;
            added += spacing;
        }
    }
    added
}

pub fn trim_line_end_spacing(line: &mut crate::story::Line, spacing: f32) -> bool {
    if spacing == 0.0 || !spacing.is_finite() {
        return false;
    }
    let count = line.glyphs.len();
    let Some(last) = line.glyphs.last() else { return false };
    let synthetic_tail = count >= 2 && line.glyphs[count - 2].cluster == last.cluster && last.font == FontId(0) && last.gid == 0;
    if synthetic_tail {
        return false;
    }
    line.glyphs[count - 1].advance -= spacing;
    line.width -= spacing;
    true
}
pub fn shape_run(lib: &FontLibrary, text: &str, family: &str, style: &str, size_pt: f32) -> ShapedRun {
    shape_run_with(lib, text, family, style, size_pt, &ShapeOptions::default())
}

pub fn shape_styled(lib: &FontLibrary, text: &str, style: &CharStyle) -> ShapedRun {
    if style.caps != Caps::AllCaps {
        return shape_run_with(lib, text, &style.family, &style.style, style.size_pt, &ShapeOptions::from_style(style));
    }
    let (upper, origin) = upper_with_origin(text);
    let mut run = shape_run_with(lib, &upper, &style.family, &style.style, style.size_pt, &ShapeOptions::from_style(style));
    for glyph in &mut run.glyphs {
        glyph.cluster = origin.get(glyph.cluster).copied().unwrap_or(text.len());
    }
    run
}

fn upper_with_origin(text: &str) -> (String, Vec<usize>) {
    let mut upper = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len() + 1);
    for (byte, ch) in text.char_indices() {
        let mut it = ch.to_uppercase();
        let mapped = match (it.next(), it.next()) {
            (Some(u), None) => u,
            _ => ch,
        };
        upper.push(mapped);
        origin.resize(upper.len(), byte);
    }
    origin.push(text.len());
    (upper, origin)
}

pub fn shape_run_with(lib: &FontLibrary, text: &str, family: &str, style: &str, size_pt: f32, options: &ShapeOptions) -> ShapedRun {
    if text.is_empty() || !size_pt.is_finite() || size_pt <= 0.0 {
        return ShapedRun {
            language: options.language.clone(),
            features: options.features.clone(),
            transform: options.transform,
            ..ShapedRun::default()
        };
    }

    let primary_id = lib.resolve(family, style).or_else(|| {
        let family = family.trim();
        if !family.is_empty() && !family.eq_ignore_ascii_case("sans-serif") { return None; }
        let (weight, italic) = crate::fonts::parse_style(style).unwrap_or((400, false));
        let preferred = ["Arial", "DejaVu Sans", "Segoe UI", "Malgun Gothic"];
        preferred.iter().copied().chain(lib.families()).find_map(|name| {
            lib.resolve_nearest(name, options.weight.unwrap_or(weight), options.italic.unwrap_or(italic))
        })
    });
    let primary_stack = primary_id.map(|pid| crate::fonts::family_stack_for(lib, pid, text));
    let mut engine = lib.engine.lock().unwrap_or_else(|e| e.into_inner());
    let FontEngine { fonts, layout, ids } = &mut *engine;

    let mut builder = layout.ranged_builder(fonts, text, 1.0, true);
    builder.push_default(StyleProperty::FontSize(size_pt));
    
    if let Some(pid) = primary_id {
        if let Some(face) = lib.face(pid) {
            let stack = primary_stack.unwrap_or_else(|| crate::fonts::family_stack(&face.family));
            builder.push_default(StyleProperty::FontFamily(FontFamily::Source(std::borrow::Cow::Owned(stack))));
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(face.weight as f32)));
            builder.push_default(StyleProperty::FontStyle(if face.italic { FontStyle::Italic } else { FontStyle::Normal }));
            builder.push_default(StyleProperty::FontWidth(FontWidth::from_ratio(face.stretch)));
        } else if !family.trim().is_empty() {
            builder.push_default(StyleProperty::FontFamily(FontFamily::Source(std::borrow::Cow::Owned(crate::fonts::family_stack(family)))));
        }
    } else if !family.trim().is_empty() {
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(std::borrow::Cow::Owned(crate::fonts::family_stack(family)))));
        if let Some(weight) = options.weight {
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight as f32)));
        }
        if let Some(italic) = options.italic {
            builder.push_default(StyleProperty::FontStyle(if italic { FontStyle::Italic } else { FontStyle::Normal }));
        }
    }

    if let Some(language) = options.language.as_deref() {
        if let Some(parsed) = parse_language(language) {
            builder.push_default(StyleProperty::Locale(Some(parsed)));
        }
    }

    let feature_source = options.feature_source();
    if !feature_source.is_empty() {
        builder.push_default(StyleProperty::FontFeatures(FontFeatures::Source(std::borrow::Cow::Owned(feature_source))));
    }

    let mut shaped: Layout<()> = builder.build(text);
    shaped.break_all_lines(None);

    let mut result = ShapedRun {
        width: shaped.width(),
        language: options.language.clone(),
        features: options.features.clone(),
        transform: options.transform,
        ..Default::default()
    };

    for line in shaped.lines() {
        for run in line.runs() {
            let font = run.font();
            let id = ids.get(&(font.data.id(), font.index)).copied().unwrap_or(UNMAPPED_FONT);
            
            result.ascent = result.ascent.max(run.font_metrics().ascent);
            result.descent = result.descent.max(run.font_metrics().descent);

            for cluster in run.visual_clusters() {
                for glyph in cluster.glyphs() {
                    result.glyphs.push(ShapedGlyph {
                        font: id,
                        gid: glyph.id as u16,
                        cluster: cluster.text_range().start,
                        advance: glyph.advance,
                        offset: [glyph.x, glyph.y],
                    });
                }
            }
        }
    }

    result
}

fn scalar_index(text: &str, byte: usize) -> usize {
    text.char_indices().take_while(|(at, _)| *at < byte).count()
}

pub fn notdef_scalars(lib: &FontLibrary, text: &str, run: &ShapedRun) -> Vec<usize> {
    let mut out: Vec<usize> = run
        .glyphs
        .iter()
        .filter(|glyph| {
            if glyph.gid == 0 || glyph.font == UNMAPPED_FONT {
                return true;
            }
            match text.get(glyph.cluster..).and_then(|rest| rest.chars().next()) {
                Some(ch) if !ch.is_control() && !ch.is_whitespace() => !lib.has_glyph(glyph.font, ch),
                _ => false,
            }
        })
        .map(|glyph| scalar_index(text, glyph.cluster))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

pub fn parse_language(tag: &str) -> Option<Language> {
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }
    Language::parse(tag).ok()
}
