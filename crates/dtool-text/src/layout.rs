use crate::char_style::{OpticalAlign, compute_optical_hang};
use crate::fonts::{FontId, FontLibrary, UNMAPPED_FONT};
use crate::metrics::FontMetrics;
use crate::hyphen::Hyphenator;
use crate::linebreak::{can_break_between, can_end_line, can_start_line};
use crate::para_style::{BreakKind, DropCap, Justification, Leader, ListKind, ListProps, NumberFormat, SpaceBeforeRule, Special, TabAlign, TabStop};
use crate::shape::{shape_styled, ShapedGlyph};
use crate::story::{Align, CharStyle, Line, ParaStyle, Story, TabRule, TextRange};

const DEFAULT_TAB_INTERVAL: f32 = 36.0;

fn style_at(story: &Story, index: usize) -> CharStyle {
    story
        .chars
        .iter()
        .rev()
        .find(|run| run.range.start <= index && index < run.range.end)
        .map(|run| run.style.clone())
        .unwrap_or_default()
}

pub(crate) fn para_run_at(story: &Story, index: usize) -> Option<&ParaStyle> {
    story
        .paras
        .iter()
        .rev()
        .find(|run| run.range.start <= index && (index < run.range.end || run.range.start == index && run.range.end == index))
        .map(|run| &run.style)
}

fn fixture_advance(size_pt: f32) -> f32 {
    FontMetrics::approximate(size_pt).x_height_pt
}

fn fixture_glyphs(text: &str, size_pt: f32) -> (Vec<ShapedGlyph>, f32) {
    let advance = fixture_advance(size_pt);
    let mut glyphs = Vec::new();
    let mut width = 0.0f32;
    let mut cluster = 0;
    for ch in text.chars() {
        if ch != '\n' {
            glyphs.push(ShapedGlyph {
                font: FontId(0),
                gid: 0,
                cluster,
                advance,
                offset: [0.0, 0.0],
            });
            width += advance;
        }
        cluster += 1;
    }
    (glyphs, width)
}

fn shaped_or_fixture(lib: &FontLibrary, text: &str, style: &CharStyle) -> (Vec<ShapedGlyph>, f32, f32, f32) {
    let metrics = FontMetrics::approximate(style.size_pt);
    let ascent = metrics.ascent_pt;
    let descent = metrics.descent_pt;
    if !style.size_pt.is_finite() || style.size_pt <= 0.0 {
        return (Vec::new(), 0.0, ascent, descent);
    }
    if text.chars().all(|ch| ch == '\n') {
        let (ascent, descent) = font_metrics(lib, style, ascent, descent);
        let (glyphs, width) = fixture_glyphs(text, style.size_pt);
        return (glyphs, width, ascent, descent);
    }
    if !style.family.trim().is_empty() {
        let shaped = shape_styled(lib, text, style);
        if !shaped.glyphs.is_empty() && shaped.width.is_finite() && shaped.width > 0.0 {
            let width = shaped.transformed_width();
            let used_ascent = if shaped.transformed_ascent() > 0.0 { shaped.transformed_ascent() } else { ascent };
            let used_descent = if shaped.transformed_descent() > 0.0 { shaped.transformed_descent() } else { descent };
            let xf = shaped.transform;
            let mut glyphs = shaped.glyphs;
            let mut scalar_of = vec![0usize; text.len() + 1];
            let mut count = 0usize;
            for (byte, _) in text.char_indices() {
                scalar_of[byte] = count;
                count += 1;
            }
            scalar_of[text.len()] = count;
            for glyph in &mut glyphs {
                glyph.cluster = scalar_of.get(glyph.cluster).copied().unwrap_or(count);
            }
            if !xf.is_identity() {
                for glyph in &mut glyphs {
                    glyph.advance = xf.advance(glyph.advance);
                    glyph.offset = xf.apply_point(glyph.offset);
                }
            }
            return (glyphs, width, used_ascent, used_descent);
        }
    }
    let (mut glyphs, width) = fixture_glyphs(text, style.size_pt);
    let xf = crate::GlyphTransform::from_style(style);
    if !xf.is_identity() {
        for glyph in &mut glyphs {
            glyph.advance = xf.advance(glyph.advance);
            glyph.offset = xf.apply_point(glyph.offset);
        }
    }
    (glyphs, xf.advance(width), ascent, descent)
}

fn shaped_runs(lib: &FontLibrary, story: &Story, base: usize, text: &str, style: &CharStyle) -> (Vec<ShapedGlyph>, f32, f32, f32) {
    let chars: Vec<char> = text.chars().collect();
    let mut segments: Vec<(usize, usize, CharStyle)> = Vec::new();
    for index in 0..chars.len() {
        let at = style_at(story, base + index);
        match segments.last_mut() {
            Some(last) if last.2 == at => last.1 = index + 1,
            _ => segments.push((index, index + 1, at)),
        }
    }
    if segments.len() < 2 {
        return shaped_or_fixture(lib, text, style);
    }
    let mut glyphs = Vec::new();
    let mut width = 0.0f32;
    let mut ascent = 0.0f32;
    let mut descent = 0.0f32;
    for (start, end, run_style) in &segments {
        let piece: String = chars[*start..*end].iter().collect();
        let (run, run_width, run_ascent, run_descent) = shaped_or_fixture(lib, &piece, run_style);
        glyphs.extend(run.into_iter().map(|mut glyph| {
            glyph.cluster += start;
            glyph
        }));
        width += run_width;
        ascent = ascent.max(run_ascent);
        descent = descent.max(run_descent);
    }
    (glyphs, width, ascent, descent)
}

fn font_metrics(lib: &FontLibrary, style: &CharStyle, ascent: f32, descent: f32) -> (f32, f32) {
    if style.family.trim().is_empty() {
        return (ascent, descent);
    }
    let shaped = shape_styled(lib, "x", style);
    if shaped.glyphs.is_empty() || !shaped.width.is_finite() || shaped.width <= 0.0 {
        return (ascent, descent);
    }
    let used_ascent = if shaped.transformed_ascent() > 0.0 { shaped.transformed_ascent() } else { ascent };
    let used_descent = if shaped.transformed_descent() > 0.0 { shaped.transformed_descent() } else { descent };
    (used_ascent, used_descent)
}

fn cluster_end(text: &str, glyphs: &[ShapedGlyph], start_cluster: usize) -> usize {
    let last = glyphs.last().map(|g| g.cluster).unwrap_or(start_cluster);
    let mut n = 0;
    for ch in text.chars() {
        if n == last {
            return if ch == '\n' { n } else { n + 1 };
        }
        n += 1;
    }
    n
}

fn char_at(text: &str, cluster: usize) -> Option<char> {
    text.chars().nth(cluster)
}

fn line_height(ascent: f32, descent: f32, size_pt: f32, leading: Option<f32>, units_per_point: f32) -> f32 {
    let natural = ascent + descent;
    let Some(leading) = leading else { return natural; };
    if !leading.is_finite() || leading <= 0.0 {
        return natural;
    }
    // Existing styles encode values below four as a multiple of the font size.
    if leading < 4.0 {
        if size_pt.is_finite() && size_pt > 0.0 {
            size_pt * leading
        } else {
            natural * leading
        }
    } else {
        leading * units_per_point
    }
}

fn alpha_marker(mut n: u32, upper: bool) -> String {
    if n == 0 {
        n = 1;
    }
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        let c = (n % 26) as u8;
        let base = if upper { b'A' } else { b'a' };
        s.insert(0, (base + c) as char);
        n /= 26;
    }
    s
}

fn roman_marker(mut n: u32) -> String {
    if n == 0 {
        n = 1;
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, sym) in table {
        while n >= v {
            s.push_str(sym);
            n -= v;
        }
    }
    s
}

fn format_number(fmt: &NumberFormat, n: u32) -> String {
    let n = n.max(1);
    match fmt {
        NumberFormat::Arabic => n.to_string(),
        NumberFormat::Arabic2 => format!("{:02}", n),
        NumberFormat::Arabic3 => format!("{:03}", n),
        NumberFormat::Arabic4 => format!("{:04}", n),
        NumberFormat::AlphaLower => alpha_marker(n, false),
        NumberFormat::AlphaUpper => alpha_marker(n, true),
        NumberFormat::RomanLower => roman_marker(n).to_lowercase(),
        NumberFormat::RomanUpper => roman_marker(n),
    }
}

fn list_marker(state: &mut Vec<(u32, u32)>, list: &ListProps) -> Option<String> {
    match &list.kind {
        ListKind::None => None,
        ListKind::Bullet(text) => Some(text.clone()),
        ListKind::Number(fmt) => {
            let level = list.level.max(1);
            state.retain(|(l, _)| *l <= level);
            let start = list.start_at.unwrap_or(1).max(1);
            let n = if list.restart {
                if let Some(pos) = state.iter().position(|(l, _)| *l == level) {
                    state[pos].1 = start.saturating_add(1);
                } else {
                    state.push((level, start.saturating_add(1)));
                }
                start
            } else if let Some(pos) = state.iter().position(|(l, _)| *l == level) {
                let cur = if list.start_at.is_some() { start } else { state[pos].1 };
                state[pos].1 = cur.saturating_add(1);
                cur
            } else {
                state.push((level, start.saturating_add(1)));
                start
            };
            Some(format!("{}.", format_number(fmt, n)))
        }
    }
}

fn marker_glyphs(lib: &FontLibrary, marker: &str, style: &CharStyle) -> Vec<ShapedGlyph> {
    let (mut glyphs, _, _, _) = shaped_or_fixture(lib, marker, style);
    for glyph in &mut glyphs {
        glyph.cluster = 0;
    }
    if !glyphs.is_empty() {
        glyphs.push(ShapedGlyph {
            font: UNMAPPED_FONT,
            gid: 0,
            cluster: 0,
            advance: fixture_advance(style.size_pt),
            offset: [0.0, 0.0],
        });
    }
    glyphs
}

struct ListItemLayout {
    marker: Vec<ShapedGlyph>,
    indent: f32,
    hang: f32,
}

#[derive(Default)]
/// Number and measure markers against the full story before splitting it into frames.
pub(crate) struct ListLayout(Vec<(usize, Option<ListItemLayout>)>);

impl ListLayout {
    pub(crate) fn new(lib: &FontLibrary, story: &Story) -> Self {
        if !story.paras.iter().any(|run| !matches!(run.style.list.kind, ListKind::None)) {
            return Self::default();
        }
        let scale = story.layout_units_per_point.unwrap_or(1.0);
        let last_char = story.text.chars().count().saturating_sub(1);
        let mut counters = Vec::new();
        let mut columns = std::collections::HashMap::new();
        let mut column_widths = Vec::<f32>::new();
        let mut start = 0;
        let mut items = Vec::new();
        for part in story.text.split(['\n', '\u{2029}']) {
            let item = para_run_at(story, start).and_then(|style| {
                let list = &style.list;
                let text = list_marker(&mut counters, list)?;
                let marker = marker_glyphs(lib, &text, &style_at(story, start.min(last_char)));
                let width: f32 = marker.iter().map(|glyph| glyph.advance).sum();
                let step = list.tab_step() * scale;
                let hang = step.max(width);
                let indent = step * list.level.saturating_sub(1) as f32 + hang;
                let numbered = matches!(list.kind, ListKind::Number(_));
                // Nested items keep their own columns without interrupting their parent list.
                let column = *columns.entry((list.level, step.to_bits(), numbered)).or_insert_with(|| {
                    column_widths.push(0.0);
                    column_widths.len() - 1
                });
                column_widths[column] = column_widths[column].max(hang);
                Some((ListItemLayout { marker, indent, hang }, column, list.level, step))
            });
            if item.is_none() {
                columns.clear();
            }
            items.push((start, item));
            start += part.chars().count() + 1;
        }
        let mut ancestors = Vec::<(u32, f32)>::new();
        Self(items.into_iter().map(|(start, item)| {
            let Some((mut item, column, level, step)) = item else {
                ancestors.clear();
                return (start, None);
            };
            while ancestors.last().is_some_and(|&(parent_level, _)| parent_level >= level) {
                ancestors.pop();
            }
            let mut marker_origin = item.indent - item.hang;
            if let Some(&(parent_level, parent_body)) = ancestors.last() {
                let skipped = level.saturating_sub(parent_level).saturating_sub(1);
                marker_origin = marker_origin.max(parent_body + skipped as f32 * step);
            }
            item.hang = column_widths[column];
            item.indent = marker_origin + item.hang;
            ancestors.push((level, item.indent));
            (start, Some(item))
        }).collect())
    }

    fn item_at(&self, position: usize) -> Option<(bool, &ListItemLayout)> {
        let index = self.0.partition_point(|(start, _)| *start <= position).checked_sub(1)?;
        let (start, item) = &self.0[index];
        Some((*start == position, item.as_ref()?))
    }
}

impl ListItemLayout {
    fn attach_marker(&self, line: &mut Line) {
        let body_x = line.glyphs.first().map_or(0.0, |glyph| glyph.offset[0]);
        let mut x = body_x - self.hang;
        let marker = self.marker.iter().cloned().map(|mut glyph| {
            glyph.cluster = line.range.start;
            glyph.offset[0] += x;
            x += glyph.advance;
            glyph.advance = 0.0;
            glyph
        });
        line.list_marker_len = self.marker.len();
        line.glyphs.splice(0..0, marker);
    }
}

fn tab_position(tab: &TabStop, frame_width: f32) -> f32 {
    if tab.from_right {
        frame_width - tab.pos
    } else {
        tab.pos
    }
}

fn tab_interval_of(para: &ParaStyle) -> f32 {
    let v = para.default_tab_interval;
    if v.is_finite() && v > 0.0 {
        v
    } else {
        DEFAULT_TAB_INTERVAL
    }
}

fn paragraph_layout_units(mut style: ParaStyle, scale: f32) -> ParaStyle {
    style.indent_left *= scale;
    style.indent_right *= scale;
    style.indent_first *= scale;
    style.space_before *= scale;
    style.space_after *= scale;
    style.drop_cap.distance *= scale;
    style.default_tab_interval = tab_interval_of(&style) * scale;
    for tab in &mut style.tabs {
        tab.pos *= scale;
    }
    style
}

fn next_left_tab(tabs: &[TabStop], x: f32, frame_width: f32, interval: f32) -> f32 {
    let mut stops: Vec<f32> = tabs
        .iter()
        .map(|t| tab_position(t, frame_width))
        .filter(|p| p.is_finite())
        .collect();
    stops.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(pos) = stops.iter().copied().find(|p| *p > x + 0.001) {
        return pos;
    }
    let interval = if interval.is_finite() && interval > 0.0 { interval } else { DEFAULT_TAB_INTERVAL };
    if !interval.is_finite() || interval <= 0.0 {
        return x;
    }
    let k = (x / interval).floor() + 1.0;
    k * interval
}

fn find_tab_stop(tabs: &[TabStop], x: f32, frame_width: f32) -> Option<TabStop> {
    let mut best: Option<(f32, TabStop)> = None;
    for tab in tabs {
        let pos = tab_position(tab, frame_width);
        if pos.is_finite() && pos > x + 0.001 {
            if best.as_ref().map(|(p, _)| pos < *p).unwrap_or(true) {
                best = Some((pos, tab.clone()));
            }
        }
    }
    best.map(|(_, t)| t)
}

fn segment_end(glyphs: &[ShapedGlyph], text: &str, start: usize) -> usize {
    let mut i = start;
    while i < glyphs.len() {
        if let Some('\t') = char_at(text, glyphs[i].cluster) {
            break;
        }
        i += 1;
    }
    i
}

fn width_to_decimal(glyphs: &[ShapedGlyph], text: &str, start: usize, end: usize, decimal: char) -> f32 {
    let mut w = 0.0f32;
    for g in &glyphs[start..end] {
        if let Some(ch) = char_at(text, g.cluster) {
            if ch == decimal {
                return w;
            }
        }
        w += g.advance;
    }
    w
}

#[derive(Clone, Copy)]
pub(crate) struct TabSource<'a> {
    pub(crate) lib: &'a FontLibrary,
    pub(crate) story: &'a Story,
    pub(crate) paragraph_start: usize,
}

pub(crate) fn append_tab_char(glyphs: &mut Vec<ShapedGlyph>, source: TabSource<'_>, ch: char, gap: f32, cluster: usize) -> f32 {
    if gap <= 0.0 || !gap.is_finite() || ch.is_whitespace() || ch.is_control() {
        return gap;
    }
    let mut style = style_at(source.story, source.paragraph_start + cluster);
    if style.family.is_empty() {
        style.family = "sans-serif".into();
    }
    let (mut copies, _, _, _) = shaped_or_fixture(source.lib, &ch.to_string(), &style);
    let advance: f32 = copies.iter().map(|glyph| glyph.advance).sum();
    if !advance.is_finite() || advance <= 0.0 || copies.iter().any(|glyph| glyph.gid == 0 || glyph.font == UNMAPPED_FONT) {
        return gap;
    }
    let repeat = (gap / advance).floor().min(4096.0) as usize;
    for glyph in &mut copies {
        glyph.cluster = cluster;
    }
    for _ in 0..repeat {
        glyphs.extend(copies.iter().cloned());
    }
    (gap - repeat as f32 * advance).max(0.0)
}

pub(crate) fn path_tabs(story: &Story, start: usize) -> ParaStyle {
    let scale = story.layout_units_per_point.unwrap_or(1.0);
    paragraph_layout_units(para_run_at(story, start).cloned().unwrap_or_default(), scale)
}

pub(crate) fn tab_gap(
    tabs: &[TabStop],
    x: f32,
    frame_width: f32,
    interval: f32,
    width: impl FnOnce(Option<char>) -> f32,
) -> (f32, Leader) {
    let Some(stop) = find_tab_stop(tabs, x, frame_width) else {
        return ((next_left_tab(tabs, x, frame_width, interval) - x).max(0.0), Leader::None);
    };
    let pos = tab_position(&stop, frame_width);
    let aligned = match stop.align {
        TabAlign::Left => pos,
        TabAlign::Center => pos - width(None) * 0.5,
        TabAlign::Right => pos - width(None),
        TabAlign::Decimal => pos - width(Some(stop.decimal)),
    };
    ((aligned - x).max(0.0), stop.leader)
}

fn apply_tabs(glyphs: &mut Vec<ShapedGlyph>, text: &str, tabs: &[TabStop], frame_width: f32, origin: f32, interval: f32, source: TabSource<'_>) -> Vec<(usize, TabRule)> {
    if glyphs.is_empty() || !text.contains('\t') {
        return Vec::new();
    }
    let mut rules = Vec::new();
    let mut result: Vec<ShapedGlyph> = Vec::with_capacity(glyphs.len());
    let mut x = origin;
    let mut i = 0;
    while i < glyphs.len() {
        let is_tab = matches!(char_at(text, glyphs[i].cluster), Some('\t'));
        if is_tab {
            let seg_from = i + 1;
            let seg_to = segment_end(glyphs, text, seg_from);
            let (gap, leader) = tab_gap(tabs, x, frame_width, interval, |decimal| {
                if let Some(decimal) = decimal {
                    width_to_decimal(glyphs, text, seg_from, seg_to, decimal)
                } else {
                    glyphs[seg_from..seg_to].iter().map(|g| g.advance).sum()
                }
            });
            let cluster = glyphs[i].cluster;
            let advance = match leader {
                Leader::Character(ch) => append_tab_char(&mut result, source, ch, gap, cluster),
                Leader::Underline if gap > 0.0 => { rules.push((result.len(), TabRule::Underline)); gap }
                Leader::Strikeout if gap > 0.0 => { rules.push((result.len(), TabRule::Strikeout)); gap }
                _ => gap,
            };
            result.push(ShapedGlyph { font: UNMAPPED_FONT, gid: 0, cluster, advance, offset: [0.0, 0.0] });
            x += gap;
            i += 1;
            continue;
        }
        result.push(glyphs[i].clone());
        x += glyphs[i].advance;
        i += 1;
    }
    *glyphs = result;
    rules
}

fn shift_glyphs(glyphs: &mut [ShapedGlyph], dx: f32) {
    if dx == 0.0 {
        return;
    }
    for g in glyphs {
        g.offset[0] += dx;
    }
}

fn letter_gap(chars: &[char], left: &ShapedGlyph, right: &ShapedGlyph) -> bool {
    left.cluster != right.cluster && [left, right].into_iter().all(|glyph| {
        chars.get(glyph.cluster).is_some_and(|ch| !is_unbreakable(*ch) && !is_wrap_space(*ch) && !ch.is_control())
    })
}

fn preferred_spacing(glyphs: &mut [ShapedGlyph], text: &str, options: Justification, em: f32) {
    let chars: Vec<_> = text.chars().collect();
    for index in 0..glyphs.len().saturating_sub(1) {
        if chars.get(glyphs[index].cluster) == Some(&' ') {
            glyphs[index].advance *= options.word_ideal / 100.0;
        } else if letter_gap(&chars, &glyphs[index], &glyphs[index + 1]) {
            glyphs[index].advance = (glyphs[index].advance + options.letter_ideal * em / 100.0).max(0.0);
        }
    }
}

fn distribute_spacing(
    glyphs: &mut [ShapedGlyph], gaps: &[usize], extra: f32, limit: impl Fn(f32) -> f32,
) -> f32 {
    let capacity: f32 = gaps.iter().map(|&i| limit(glyphs[i].advance)).sum();
    if capacity <= 0.0 {
        return 0.0;
    }
    let amount = extra.abs().min(capacity).copysign(extra);
    let fraction = amount / capacity;
    for &index in gaps {
        glyphs[index].advance += limit(glyphs[index].advance) * fraction;
    }
    amount
}

fn justify_line(line: &mut Line, chars: &[char], avail: f32, custom: Option<Justification>, em: f32) -> bool {
    let glyphs = &mut line.glyphs;
    let character = |glyph: &ShapedGlyph| chars.get(glyph.cluster).copied();
    let first = glyphs.iter().rposition(|glyph| character(glyph) == Some('\t'))
        .map_or(0, |index| index + 1);
    let Some(first_ink) = glyphs[first..].iter().position(|glyph| {
        character(glyph).is_some_and(|ch| !is_wrap_space(ch) && !ch.is_control())
    }).map(|index| first + index) else { return false };
    let Some(last_ink) = glyphs.iter().rposition(|glyph| {
        character(glyph).is_some_and(|ch| !is_wrap_space(ch) && !ch.is_control())
    }) else { return false };

    let mut word_gaps = Vec::new();
    let mut cluster_gaps = Vec::new();
    for index in first_ink..last_ink {
        let glyph = &glyphs[index];
        if glyph.cluster == glyphs[index + 1].cluster {
            continue;
        }
        if character(glyph) == Some(' ') && glyph.advance > 0.0 {
            word_gaps.push(index);
        } else if letter_gap(chars, glyph, &glyphs[index + 1]) {
            cluster_gaps.push(index);
        }
    }
    if word_gaps.is_empty() && cluster_gaps.is_empty() {
        return false;
    }
    let trailing = glyphs[last_ink + 1..].iter().map(|glyph| glyph.advance).sum::<f32>();
    let extra = avail - (line.width - trailing);
    if !extra.is_finite() || (extra <= 0.0 && custom.is_none()) {
        return false;
    }
    for glyph in &mut glyphs[last_ink + 1..] {
        glyph.advance = 0.0;
    }
    if let Some(options) = custom {
        let word_limit = if extra >= 0.0 { options.word_max } else { options.word_min };
        let word_change = if extra >= 0.0 {
            (word_limit / options.word_ideal.max(1.0) - 1.0).max(0.0)
        } else {
            (1.0 - word_limit / options.word_ideal.max(1.0)).max(0.0)
        };
        let used = distribute_spacing(glyphs, &word_gaps, extra, |width| {
            let limit = width * word_change;
            if extra < 0.0 { limit.min(width) } else { limit }
        });
        let letter_limit = if extra >= 0.0 { options.letter_max } else { options.letter_min };
        let letter_change = if extra >= 0.0 { letter_limit - options.letter_ideal } else { options.letter_ideal - letter_limit };
        let letter_change = letter_change.max(0.0) * em / 100.0;
        let letters = distribute_spacing(glyphs, &cluster_gaps, extra - used, |width| {
            if extra < 0.0 { letter_change.min(width) } else { letter_change }
        });
        line.width += used + letters - trailing;
    } else {
        let gaps = if word_gaps.is_empty() { cluster_gaps } else { word_gaps };
        let each = extra / gaps.len() as f32;
        for index in gaps {
            glyphs[index].advance += each;
        }
        line.width = avail;
    }
    true
}

fn apply_align(line: &mut Line, chars: &[char], align: Align, left: f32, avail: f32, last_of_para: bool, custom: Option<Justification>, em: f32) {
    let extra = if avail.is_finite() { (avail - line.width).max(0.0) } else { 0.0 };
    let should_justify = match align {
        Align::JustifyAll => true,
        Align::JustifyLeft | Align::JustifyCenter | Align::JustifyRight => {
            !last_of_para
        }
        _ => false,
    };
    let fit = align.is_justified() && custom.is_some() && line.width > avail;
    let justify = (should_justify || fit) && justify_line(line, chars, avail, custom, em);
    let shift = if justify {
        0.0
    } else {
        match align {
            Align::Left | Align::TowardsSpine | Align::JustifyLeft => 0.0,
            Align::Center | Align::JustifyCenter => extra * 0.5,
            Align::Right | Align::AwayFromSpine | Align::JustifyRight | Align::JustifyAll => extra,
        }
    };
    let offset = left + shift;
    if line.glyphs.is_empty() && offset != 0.0 {
        // Keep the caret origin even when the paragraph has no drawable glyphs.
        line.glyphs.push(ShapedGlyph {
            font: UNMAPPED_FONT,
            gid: 0,
            cluster: line.range.start,
            advance: 0.0,
            offset: [offset, 0.0],
        });
    } else {
        shift_glyphs(&mut line.glyphs, offset);
    }
}

fn make_line(
    glyphs: Vec<ShapedGlyph>,
    text: &str,
    range_start: usize,
    baseline: f32,
    tab_rules: Vec<(usize, TabRule)>,
) -> Line {
    let width = glyphs.iter().map(|g| g.advance).sum();
    let start_cluster = glyphs.first().map(|g| g.cluster).unwrap_or(0);
    let end = if glyphs.is_empty() {
        range_start
    } else {
        range_start + cluster_end(text, &glyphs, start_cluster)
    };
    let start = if glyphs.is_empty() {
        range_start
    } else {
        range_start + glyphs[0].cluster
    };
    Line {
        range: TextRange::new(start, end),
        glyphs,
        list_marker_len: 0,
        tab_rules,
        baseline,
        width,
        break_after: None,
    }
}


fn special_space_advance(ch: char, size_pt: f32, fallback: f32) -> Option<f32> {
    let em = if size_pt.is_finite() && size_pt > 0.0 { size_pt } else { fallback };
    match ch {
        '\u{00AD}' | '\u{200B}' | '\u{200C}' | '\u{2028}' | '\u{2029}' => Some(0.0),
        '\u{2003}' => Some(em),
        '\u{2002}' => Some(em * 0.5),
        '\u{2009}' => Some(em * 0.2),
        '\u{200A}' => Some(em * 0.1),
        '\u{2007}' => Some(if fallback > 0.0 { fallback } else { em * 0.5 }),
        '\u{2008}' => Some(em * 0.25),
        '\u{00A0}' => Some(if fallback > 0.0 { fallback } else { em * 0.25 }),
        _ => None,
    }
}

fn apply_special_space_widths(glyphs: &mut [ShapedGlyph], text: &str, size_pt: f32) {
    if text.is_ascii() {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    for g in glyphs.iter_mut() {
        if let Some(ch) = chars.get(g.cluster).copied() {
            if let Some(adv) = special_space_advance(ch, size_pt, g.advance) {
                g.advance = adv;
            }
        }
    }
}

fn is_unbreakable(ch: char) -> bool {
    matches!(ch, '\u{00A0}' | '\u{2011}' | '\u{202F}' | '\u{2060}')
}

fn is_wrap_space(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t' | '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{200A}' | '\u{2007}' | '\u{2008}' | '\u{200B}'
    )
}

fn is_forced_line_break_char(ch: char) -> bool {
    ch == '\u{2028}'
}

fn special_at(specials: &[(usize, Special)], abs_index: usize) -> Option<Special> {
    specials.iter().find(|(pos, _)| *pos == abs_index).map(|(_, s)| s.clone())
}

fn hyphen_glyph(source: TabSource<'_>, cluster: usize, size_pt: f32) -> ShapedGlyph {
    let mut style = style_at(source.story, source.paragraph_start + cluster);
    if style.family.is_empty() {
        style.family = "sans-serif".into();
    }
    let (glyphs, _, _, _) = shaped_or_fixture(source.lib, "-", &style);
    if let Some(mut glyph) = glyphs.into_iter().find(|glyph| {
        glyph.gid != 0 && glyph.font != UNMAPPED_FONT && glyph.advance.is_finite() && glyph.advance > 0.0
    }) {
        glyph.cluster = cluster;
        return glyph;
    }
    ShapedGlyph {
        font: FontId(0),
        gid: 0,
        cluster,
        advance: fixture_advance(size_pt),
        offset: [0.0, 0.0],
    }
}

fn letter_count_in_range(text: &str, glyphs: &[ShapedGlyph], from: usize, to: usize) -> usize {
    let mut n = 0;
    for g in &glyphs[from..to] {
        if let Some(ch) = char_at(text, g.cluster) {
            if ch == '\u{00AD}' {
                continue;
            }
            if ch.is_alphabetic() || ch == '\'' || ch == '\u{2019}' || ch == '-' || ch == '\u{2011}' || ch == '\u{2010}' {
                n += 1;
            }
        }
    }
    n
}

fn word_text(chars: &[char], glyphs: &[ShapedGlyph], from: usize, to: usize) -> String {
    let mut s = String::new();
    let mut last = None;
    for g in &glyphs[from..to] {
        if last == Some(g.cluster) {
            continue;
        }
        last = Some(g.cluster);
        if let Some(ch) = chars.get(g.cluster).copied() {
            s.push(ch);
        }
    }
    s
}

fn glyph_index_after_letters(chars: &[char], glyphs: &[ShapedGlyph], from: usize, to: usize, letters: usize) -> Option<usize> {
    let mut n = 0;
    for i in from..to {
        if let Some(ch) = chars.get(glyphs[i].cluster).copied() {
            if ch == '\u{00AD}' {
                if n == letters {
                    return Some(i + 1);
                }
                continue;
            }
            if ch.is_alphabetic() || ch == '\'' || ch == '\u{2019}' || ch == '-' || ch == '\u{2011}' || ch == '\u{2010}' {
                n += 1;
                if n == letters {
                    let mut end = i + 1;
                    while end < to {
                        if let Some('\u{00AD}') = chars.get(glyphs[end].cluster).copied() {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    return Some(end);
                }
            }
        }
    }
    None
}

fn split_paragraphs(text: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\n' || chars[i] == '\u{2029}' {
            out.push((start, chars[start..i].iter().collect()));
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push((start, chars[start..].iter().collect()));
    out
}

pub fn insert_special(story: &mut Story, at: usize, special: Special) {
    story.insert_special(at, special);
}

pub fn get_specials(story: &Story) -> &[(usize, Special)] {
    &story.specials
}


fn prev_text_char(chars: &[char], glyphs: &[ShapedGlyph], index: usize) -> Option<char> {
    glyphs[..index].iter().rev().find_map(|glyph| chars.get(glyph.cluster).copied())
}

fn kinsoku_cut_ok(chars: &[char], glyphs: &[ShapedGlyph], cut: usize) -> bool {
    if cut < glyphs.len() {
        if let Some(c) = chars.get(glyphs[cut].cluster).copied() {
            if !can_start_line(c) {
                return false;
            }
        }
    }
    if let Some(prev) = prev_text_char(chars, glyphs, cut) {
        if !can_end_line(prev) {
            return false;
        }
    }
    true
}

fn wrap_para(
    glyphs: &[ShapedGlyph],
    text: &str,
    range_start: usize,
    first_max: f32,
    rest_max: f32,
    line_h: f32,
    mut baseline: f32,
    tabs: &[TabStop],
    frame_width: f32,
    first_left: f32,
    rest_left: f32,
    size_pt: f32,
    para: &ParaStyle,
    language: &str,
    specials: &[(usize, Special)],
    right_indent: f32,
    tab_source: TabSource<'_>,
) -> (Vec<Line>, f32) {
    let mut lines = Vec::new();
    let mut work = glyphs.to_vec();
    // Glyph clusters index Unicode scalar values. Index once instead of
    // rescanning the UTF-8 paragraph at each candidate line break.
    let chars: Vec<char> = text.chars().collect();
    if work.is_empty() {
        lines.push(make_line(Vec::new(), text, range_start, baseline, Vec::new()));
        return (lines, baseline + line_h);
    }
    let interval = tab_interval_of(para);
    let hyphenator = Hyphenator::from_settings(&para.hyphenation);
    let mut consec = 0u32;
    let mut used_indent = rest_left;
    let mut start = 0usize;
    let mut acc = 0.0f32;
    let mut i = 0usize;
    let mut first = true;
    let mut last_opp: Option<usize> = None;
    let mut word_start = 0;
    let mut broken_at: Option<usize> = None;
    while i < work.len() {
        let origin = if first { first_left } else { used_indent };
        let max_w = if first { first_max } else { rest_max + (rest_left - used_indent) };
        let abs_at = range_start + work[i].cluster;
        let special = special_at(specials, abs_at);
        if let Some(Special::IndentToHere) = special {
            used_indent = origin + acc;
        }
        let ch = chars.get(work[i].cluster).copied();
        let forced = if broken_at == Some(i) { None } else { match special {
            Some(Special::Break(ref kind)) if !matches!(kind, BreakKind::Line) => Some(kind.clone()),
            Some(Special::Break(BreakKind::Line)) => Some(BreakKind::Line),
            _ => {
                if let Some(c) = ch {
                    if is_forced_line_break_char(c) { Some(BreakKind::Line) } else { None }
                } else { None }
            }
        } };
        if let Some(kind) = forced {
            let end = if matches!(special, Some(Special::Break(_))) { i } else { i + 1 };
            let mut line_glyphs = work[start..end].to_vec();
            let rules = apply_tabs(&mut line_glyphs, text, tabs, frame_width, origin, interval, tab_source);
            let mut line = make_line(line_glyphs, text, range_start, baseline, rules);
            line.break_after = Some(kind);
            lines.push(line);
            baseline += line_h;
            start = end;
            i = end;
            broken_at = Some(end);
            acc = 0.0;
            first = false;
            last_opp = None;
            word_start = end;
            consec = 0;
            continue;
        }
        if let Some(Special::RightIndentTab) = special {
            let target = (frame_width - right_indent).max(origin);
            let gap = (target - (origin + acc)).max(0.0);
            work[i].advance = gap;
            acc += gap;
            i += 1;
            last_opp = Some(i);
            word_start = i;
            continue;
        }
        let is_tab = matches!(ch, Some('\t'));
        let adv = if is_tab {
            let x = origin + acc;
            (next_left_tab(tabs, x, frame_width, interval) - x).max(0.0)
        } else {
            work[i].advance
        };
        let unbreakable = ch.map(is_unbreakable).unwrap_or(false);
        if !unbreakable {
            if let Some(c) = ch {
                if is_wrap_space(c) || c == '\t' {
                    let next_ok = if i + 1 < work.len() {
                        chars.get(work[i + 1].cluster).copied().map(can_start_line).unwrap_or(true)
                    } else {
                        true
                    };
                    if next_ok {
                        last_opp = Some(i + 1);
                    }
                    word_start = i + 1;
                }
            }
        }
        if i > start {
            if let Some(curr) = ch {
                if let Some(prev) = prev_text_char(&chars, &work, i) {
                    if can_break_between(prev, curr) {
                        last_opp = Some(i);
                    }
                }
            }
        }
        if ch.map(|c| c.is_alphabetic() || c == '\u{00AD}').unwrap_or(false) {
            if last_opp.map(|o| o <= i).unwrap_or(true) && (i == start || last_opp == Some(i)) {
                if i == start || last_opp == Some(i) {
                    word_start = i;
                }
            }
        }
        let mut overflow = acc + adv > max_w && i > start && max_w.is_finite() && max_w > 0.0;
        if overflow && para.align.is_justified() && para.justification.enabled && !is_tab {
            let options = para.justification;
            let shrink_word = (1.0 - options.word_min / options.word_ideal.max(1.0)).max(0.0);
            let shrink_letter = (options.letter_ideal - options.letter_min).max(0.0) * size_pt / 100.0;
            let after_tab = work[start..i].iter().rposition(|g| chars.get(g.cluster) == Some(&'\t'))
                .map_or(start, |index| start + index + 1);
            let reclaim: f32 = work[after_tab..=i].windows(2).map(|pair| {
                if chars.get(pair[0].cluster) == Some(&' ') {
                    pair[0].advance * shrink_word
                } else if letter_gap(&chars, &pair[0], &pair[1]) {
                    pair[0].advance.min(shrink_letter)
                } else {
                    0.0
                }
            }).sum();
            overflow = acc + adv - reclaim > max_w;
        }
        if overflow {
            let mut cut = last_opp.filter(|o| *o > start && *o <= i && kinsoku_cut_ok(&chars, &work, *o));
            let mut hyphen = None;
            let hyphen_ok = para.hyphenation.on
                && (para.hyphenation.max_consecutive == 0 || consec < para.hyphenation.max_consecutive);
            let zone = para.hyphenation.zone;
            let zone_w = if zone >= 1.0 { zone } else { max_w * zone.max(0.0) };
            if hyphen_ok && word_start >= start && word_start < i {
                let following = work[i..].iter().take_while(|glyph| {
                    chars.get(glyph.cluster).is_some_and(|c| {
                        c.is_alphabetic() || matches!(*c, '\u{00AD}' | '\'' | '\u{2019}' | '-' | '\u{2011}' | '\u{2010}')
                    })
                }).count();
                let wend = i + following;
                let before_word: f32 = work[word_start..i].iter().map(|g| g.advance).sum();
                let word_width: f32 = work[word_start..wend].iter().map(|g| g.advance).sum();
                let leftover = (max_w - (acc - before_word)).max(0.0);
                if following > 0 && (leftover <= zone_w + 0.001 || word_width > max_w) {
                    let wt = word_text(&chars, &work, word_start, wend);
                    let breaks = hyphenator.breaks_lang(&wt, language);
                    for b in breaks.into_iter().rev() {
                        if let Some(gi) = glyph_index_after_letters(&chars, &work, word_start, wend, b) {
                            if gi <= cut.unwrap_or(start) || gi > i {
                                continue;
                            }
                            let prefix: f32 = work[start..gi].iter().map(|g| g.advance).sum();
                            let glyph = hyphen_glyph(tab_source, work[gi - 1].cluster, size_pt);
                            if prefix + glyph.advance <= max_w + 0.001 {
                                cut = Some(gi);
                                hyphen = Some(glyph);
                                break;
                            }
                        }
                    }
                }
            }
            if cut.is_none() {
                let mut linked = true;
                for g in &work[start..i] {
                    if let Some(c) = chars.get(g.cluster).copied() {
                        if !is_unbreakable(c) && is_wrap_space(c) {
                            linked = false;
                            break;
                        }
                    }
                }
                let prev_unbreakable = prev_text_char(&chars, &work, i).map(is_unbreakable).unwrap_or(false);
                if linked && (ch.map(is_unbreakable).unwrap_or(false) || prev_unbreakable) {
                    acc += adv;
                    i += 1;
                    continue;
                }
                if ch.map(|c| !can_start_line(c)).unwrap_or(false) {
                    acc += adv;
                    i += 1;
                    continue;
                }
                if let Some(prev) = prev_text_char(&chars, &work, i) {
                    if !can_end_line(prev) {
                        acc += adv;
                        i += 1;
                        continue;
                    }
                }
                cut = Some(i);
            }
            let cut = cut.unwrap();
            let mut line_glyphs = work[start..cut].to_vec();
            let add_hyphen = hyphen.is_some();
            if let Some(glyph) = hyphen {
                line_glyphs.push(glyph);
                consec = consec.saturating_add(1);
            } else {
                consec = 0;
            }
            let rules = apply_tabs(&mut line_glyphs, text, tabs, frame_width, origin, interval, tab_source);
            lines.push(make_line(line_glyphs, text, range_start, baseline, rules));
            baseline += line_h;
            start = cut;
            i = cut;
            acc = 0.0;
            first = false;
            last_opp = None;
            if add_hyphen {
                word_start = cut;
            }
            continue;
        }
        acc += adv;
        i += 1;
    }
    if start < work.len() {
        let origin = if first { first_left } else { used_indent };
        let mut line_glyphs = work[start..].to_vec();
        let rules = apply_tabs(&mut line_glyphs, text, tabs, frame_width, origin, interval, tab_source);
        lines.push(make_line(line_glyphs, text, range_start, baseline, rules));
        baseline += line_h;
    }
    (lines, baseline)
}

fn drop_cap_glyphs(
    lib: &FontLibrary,
    part: &str,
    style: &CharStyle,
    drop_cap: &DropCap,
    line_h: f32,
    ascent: f32,
    descent: f32,
) -> (Vec<ShapedGlyph>, f32) {
    if !drop_cap.on || drop_cap.lines == 0 || drop_cap.chars == 0 {
        return (Vec::new(), 0.0);
    }

    let cap_text: String = part.chars().take(drop_cap.chars as usize).collect();
    if cap_text.is_empty() {
        return (Vec::new(), 0.0);
    }

    let target_height = line_h * drop_cap.lines as f32;
    let natural_height = ascent + descent;
    
    if natural_height <= 0.0 || !natural_height.is_finite() {
        return (Vec::new(), 0.0);
    }

    let scale_factor = target_height / natural_height;
    let cap_size_pt = style.size_pt * scale_factor;

    let mut cap_style = style.clone();
    cap_style.size_pt = cap_size_pt;

    let (glyphs, width, cap_ascent, cap_descent) = shaped_or_fixture(lib, &cap_text, &cap_style);

    let cap_width = glyphs.iter().map(|g| g.advance).sum::<f32>();
    
    (glyphs, cap_width)
}

pub fn layout_story(lib: &FontLibrary, story: &Story, width: Option<f32>) -> Vec<Line> {
    layout_story_at(lib, story, width, 0, &ListLayout::new(lib, story))
}

pub(crate) fn intrinsic_line_width(story: &Story, line: &Line) -> f32 {
    let mut pen = 0.0;
    let mut right = line.width;
    for glyph in &line.glyphs {
        pen += glyph.advance;
        right = right.max(pen + glyph.offset[0]);
    }
    let indent = para_run_at(story, line.range.start).map_or(0.0, |style| style.indent_right);
    right + indent * story.layout_units_per_point.unwrap_or(1.0)
}

pub(crate) fn layout_story_at(lib: &FontLibrary, story: &Story, width: Option<f32>, from: usize, lists: &ListLayout) -> Vec<Line> {
    let mut out = Vec::new();
    if story.text.is_empty() {
        return out;
    }
    let mut scalar = 0;
    let mut baseline = 0.0f32;
    let parts = split_paragraphs(&story.text);
    let last_char = story.text.chars().count() - 1;
    let frame_width = width.filter(|w| w.is_finite() && *w > 0.0);
    let units_per_point = story.layout_units_per_point.unwrap_or(1.0);

    for (index, (part_start, part_owned)) in parts.iter().enumerate() {
        scalar = *part_start;
        let tab_source = TabSource { lib, story, paragraph_start: scalar };
        let part = part_owned.as_str();
        let run = para_run_at(story, scalar);
        let has_para = run.is_some();
        let para = paragraph_layout_units(run.cloned().unwrap_or_default(), units_per_point);
        // A trailing empty paragraph uses the same metrics before and after typing.
        let style = style_at(story, scalar.min(last_char));
        let justification = (frame_width.is_some() && para.align.is_justified() && para.justification.enabled)
            .then_some(para.justification);
        let (mut glyphs, line_width, ascent, descent) = shaped_runs(lib, story, scalar, part, &style);
        apply_special_space_widths(&mut glyphs, part, style.size_pt);

        let letter_spacing = crate::shape::letter_spacing_pt(&style);
        crate::shape::apply_letter_spacing(&mut glyphs, letter_spacing);
        if let Some(options) = justification {
            preferred_spacing(&mut glyphs, part, options, style.size_pt);
        }

        let line_h = line_height(ascent, descent, style.size_pt, run.map(|style| style.leading), units_per_point);

        let item = lists.item_at(from + scalar);
        let continued_item = matches!(item, Some((false, _)));
        let list_pad = item.map_or(0.0, |(_, item)| item.indent);

        let left = if has_para || list_pad != 0.0 {
            para.indent_left + list_pad
        } else {
            0.0
        };

        let right = if has_para { para.indent_right } else { 0.0 };

        let first_extra = if has_para && !continued_item { para.indent_first } else { 0.0 };
        let first_left = left + first_extra;

        let space_before = if has_para && !continued_item && para.space_before.is_finite() && para.space_before > 0.0 {
            match para.space_rule {
                SpaceBeforeRule::Always => para.space_before,
                SpaceBeforeRule::OnlyBetweenParagraphs => {
                    if index == 0 { 0.0 } else { para.space_before }
                }
                SpaceBeforeRule::OnlyAtColumnTop => {
                    if baseline == 0.0 { para.space_before } else { 0.0 }
                }
            }
        } else {
            0.0
        };

        let space_after = if has_para && para.space_after.is_finite() && para.space_after > 0.0 {
            para.space_after
        } else {
            0.0
        };

        baseline += space_before;

        let para_start = out.len();

        let (drop_cap_glyphs, drop_cap_width) = if has_para && !continued_item && para.drop_cap.on {
            drop_cap_glyphs(lib, part, &style, &para.drop_cap, line_h, ascent, descent)
        } else {
            (Vec::new(), 0.0)
        };

        let has_drop_cap = !drop_cap_glyphs.is_empty();
        let drop_cap_lines_count = if has_drop_cap { para.drop_cap.lines.max(1) } else { 0 };
        
        let effective_first_left = if has_drop_cap {
            first_left + drop_cap_width + para.drop_cap.distance
        } else {
            first_left
        };

        match frame_width {
            Some(max_width) => {
                if has_drop_cap {
                    let cap_max_width = (max_width - first_left - right).max(0.0);
                    
                    let mut remaining_text: String = part.chars().skip(para.drop_cap.chars as usize).collect();
                    let remaining_glyphs_start = para.drop_cap.chars as usize;
                    let remaining_tabs = TabSource { paragraph_start: scalar + remaining_glyphs_start, ..tab_source };
                    
                    let (mut remaining_glyphs, _, rem_ascent, rem_descent) = shaped_runs(lib, story, scalar + para.drop_cap.chars as usize, &remaining_text, &style);
                    apply_special_space_widths(&mut remaining_glyphs, &remaining_text, style.size_pt);
                    
                    crate::shape::apply_letter_spacing(&mut remaining_glyphs, letter_spacing);
                    if let Some(options) = justification {
                        preferred_spacing(&mut remaining_glyphs, &remaining_text, options, style.size_pt);
                    }

                    let cap_baseline = baseline + ascent;
                    
                    out.push(Line {
                        range: TextRange::new(scalar, scalar + para.drop_cap.chars as usize),
                        glyphs: drop_cap_glyphs.clone(),
                        list_marker_len: 0,
                        tab_rules: Vec::new(),
                        baseline: cap_baseline,
                        width: drop_cap_width,
                        break_after: None,
                    });

                    let mut current_baseline = baseline + line_h;
                    let mut text_offset = 0usize;
                    let mut glyph_idx = 0usize;
                    
                    for line_num in 1..=drop_cap_lines_count {
                        let is_first_visual_line = line_num == 1;
                        let line_left = if is_first_visual_line { effective_first_left } else { left };
                        let avail_width = (max_width - line_left - right).max(0.0);
                        
                        let mut line_glyphs: Vec<ShapedGlyph> = Vec::new();
                        let mut acc_width = 0.0f32;
                        let mut start_idx = glyph_idx;
                        
                        while glyph_idx < remaining_glyphs.len() {
                            let adv = remaining_glyphs[glyph_idx].advance;
                            if acc_width + adv > avail_width && !line_glyphs.is_empty() {
                                break;
                            }
                            line_glyphs.push(remaining_glyphs[glyph_idx].clone());
                            acc_width += adv;
                            glyph_idx += 1;
                        }

                        if !line_glyphs.is_empty() || (glyph_idx == start_idx && remaining_text.len() > text_offset) {
                            let cluster_start = remaining_glyphs.get(start_idx).map(|g| g.cluster).unwrap_or(0);
                            let cluster_end_val = remaining_glyphs.get(glyph_idx.saturating_sub(1)).map(|g| g.cluster).unwrap_or(cluster_start);
                            
                            let range_start = scalar + para.drop_cap.chars as usize + text_offset;
                            let chars_consumed = if glyph_idx > start_idx {
                                cluster_end_val - cluster_start + 1
                            } else {
                                0
                            };
                            let range_end = range_start + chars_consumed;
                            
                            let tab_rules = apply_tabs(&mut line_glyphs, &remaining_text, &para.tabs, max_width, line_left, tab_interval_of(&para), remaining_tabs);
                            
                            out.push(Line {
                                range: TextRange::new(range_start, range_end),
                                glyphs: line_glyphs,
                                list_marker_len: 0,
                                tab_rules,
                                baseline: current_baseline + ascent,
                                width: acc_width,
                                break_after: None,
                            });
                            
                            text_offset += chars_consumed;
                        }

                        current_baseline += line_h;
                    }

                    while glyph_idx < remaining_glyphs.len() {
                        let line_left = left;
                        let avail_width = (max_width - line_left - right).max(0.0);
                        
                        let mut line_glyphs: Vec<ShapedGlyph> = Vec::new();
                        let mut acc_width = 0.0f32;
                        let start_idx = glyph_idx;
                        
                        while glyph_idx < remaining_glyphs.len() {
                            let adv = remaining_glyphs[glyph_idx].advance;
                            if acc_width + adv > avail_width && !line_glyphs.is_empty() {
                                break;
                            }
                            line_glyphs.push(remaining_glyphs[glyph_idx].clone());
                            acc_width += adv;
                            glyph_idx += 1;
                        }

                        let cluster_start = remaining_glyphs.get(start_idx).map(|g| g.cluster).unwrap_or(0);
                        let cluster_end_val = remaining_glyphs.get(glyph_idx.saturating_sub(1)).map(|g| g.cluster).unwrap_or(cluster_start);
                        
                        let range_start = scalar + para.drop_cap.chars as usize + text_offset;
                        let chars_consumed = if glyph_idx > start_idx {
                            cluster_end_val - cluster_start + 1
                        } else {
                            0
                        };
                        let range_end = range_start + chars_consumed;

                        let tab_rules = apply_tabs(&mut line_glyphs, &remaining_text, &para.tabs, max_width, line_left, tab_interval_of(&para), remaining_tabs);

                        out.push(Line {
                            range: TextRange::new(range_start, range_end),
                            glyphs: line_glyphs,
                            list_marker_len: 0,
                            tab_rules,
                            baseline: current_baseline + ascent,
                            width: acc_width,
                            break_after: None,
                        });

                        text_offset += chars_consumed;
                        current_baseline += line_h;
                    }

                    baseline = current_baseline - line_h;
                } else {
                    let first_max = (max_width - first_left - right).max(0.0);
                    let rest_max = (max_width - left - right).max(0.0);

                    let optical_align = if has_para { para.optical_align.clone() } else { OpticalAlign::None };
                    let (opt_left_hang, opt_right_hang) = compute_optical_hang(part, style.size_pt, optical_align);

                    let adj_first_left = first_left - opt_left_hang;
                    let adj_rest_left = left - opt_left_hang;
                    let adj_first_max = (max_width - adj_first_left - right + opt_right_hang).max(0.0);
                    let adj_rest_max = (max_width - adj_rest_left - right + opt_right_hang).max(0.0);

                    let (mut lines, next) = wrap_para(
                        &glyphs,
                        part,
                        scalar,
                        adj_first_max,
                        adj_rest_max,
                        line_h,
                        baseline + ascent,
                        &para.tabs,
                        max_width,
                        adj_first_left,
                        adj_rest_left,
                        style.size_pt,
                        &para,
                        &style.language,
                        &story.specials,
                        right,
                        tab_source,
                    );

                    out.append(&mut lines);
                    baseline = next - ascent;
                }
            }
            _ => {
                if has_drop_cap {
                    let cap_baseline = baseline + ascent;
                    out.push(Line {
                        range: TextRange::new(scalar, scalar + para.drop_cap.chars as usize),
                        glyphs: drop_cap_glyphs.clone(),
                        list_marker_len: 0,
                        tab_rules: Vec::new(),
                        baseline: cap_baseline,
                        width: drop_cap_width,
                        break_after: None,
                    });

                    let remaining_text: String = part.chars().skip(para.drop_cap.chars as usize).collect();
                    let (mut remaining_glyphs, _, rem_ascent, rem_descent) = shaped_runs(lib, story, scalar + para.drop_cap.chars as usize, &remaining_text, &style);
                    apply_special_space_widths(&mut remaining_glyphs, &remaining_text, style.size_pt);
                    
                    crate::shape::apply_letter_spacing(&mut remaining_glyphs, letter_spacing);

                    let mut current_baseline = baseline + line_h;
                    
                    for line_num in 1..=drop_cap_lines_count {
                        let is_first_visual_line = line_num == 1;
                        let origin = if is_first_visual_line { effective_first_left } else { left };
                        
                        let remaining_len = remaining_glyphs.len();
                        if remaining_len > 0 {
                            out.push(Line {
                                range: TextRange::new(scalar + para.drop_cap.chars as usize, scalar + part.chars().count()),
                                glyphs: remaining_glyphs.clone(),
                                list_marker_len: 0,
                                tab_rules: Vec::new(),
                                baseline: current_baseline + ascent,
                                width: remaining_glyphs.iter().map(|g| g.advance).sum::<f32>(),
                                break_after: None,
                            });
                        } else {
                            out.push(Line {
                                range: TextRange::new(scalar + para.drop_cap.chars as usize, scalar + part.chars().count()),
                                glyphs: Vec::new(),
                                list_marker_len: 0,
                                tab_rules: Vec::new(),
                                baseline: current_baseline + ascent,
                                width: 0.0,
                                break_after: None,
                            });
                        }
                        
                        current_baseline += line_h;
                    }

                    baseline = current_baseline - line_h;
                } else {
                    let origin = first_left;
                    let mut line_glyphs = glyphs;

                    let fw = if line_width > 0.0 { line_width * 4.0 } else { 1000.0 };
                    let tab_rules = apply_tabs(&mut line_glyphs, part, &para.tabs, fw, origin, tab_interval_of(&para), tab_source);

                    let end = scalar + part.chars().count();
                    let width_now = line_glyphs.iter().map(|g| g.advance).sum();

                    out.push(Line {
                        range: TextRange::new(scalar, end),
                        glyphs: line_glyphs,
                        list_marker_len: 0,
                        tab_rules,
                        baseline: baseline + ascent,
                        width: width_now,
                        break_after: None,
                    });

                    baseline += line_h;
                }
            }
        }

        let last = out.len().saturating_sub(1);
        let chars: Vec<_> = if para.align.is_justified() {
            part.chars().collect()
        } else {
            Vec::new()
        };
        for (n, line) in out[para_start..].iter_mut().enumerate() {
            let is_last = para_start + n == last;
            let is_first = n == 0;
            
            let optical_align = if has_para { para.optical_align.clone() } else { OpticalAlign::None };
            let (opt_left_hang, opt_right_hang) = compute_optical_hang(part, style.size_pt, optical_align);

            let line_left = if has_drop_cap {
                if n == 0 {
                    first_left
                } else if n <= drop_cap_lines_count as usize {
                    effective_first_left
                } else {
                    left
                }
            } else {
                if is_first { first_left - opt_left_hang } else { left - opt_left_hang }
            };

            let avail = match frame_width {
                Some(max_width) => (max_width - line_left - right + opt_right_hang).max(0.0),
                None => line.width,
            };

            if !is_last && !(has_drop_cap && n == 0) {
                crate::shape::trim_line_end_spacing(line, letter_spacing);
            }
            let cap_body_start = if has_drop_cap && n > 0 { para.drop_cap.chars as usize } else { 0 };
            let align = if has_drop_cap && is_first { Align::Left } else { para.align };
            apply_align(line, chars.get(cap_body_start..).unwrap_or_default(), align, line_left, avail, is_last, justification, style.size_pt);
            let cluster_base = scalar + cap_body_start;
            for glyph in &mut line.glyphs {
                // The spacer for an empty aligned line already uses its absolute range.
                glyph.cluster = if line.range.start == line.range.end { line.range.start } else { glyph.cluster + cluster_base };
            }
        }
        if let (Some((true, item)), Some(line)) = (item, out.get_mut(para_start)) {
            item.attach_marker(line);
        }

        baseline += space_after;

    }

    out
}
