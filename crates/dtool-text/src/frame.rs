use dtool_core::ObjectId;
use crate::fonts::FontLibrary;
use crate::layout::{layout_story_at, para_run_at, ListLayout};
use crate::para_style::{BreakKind, FlowStart, Special};
use std::{borrow::Cow, cell::RefCell, collections::HashMap, rc::Rc};
use crate::story::{Line, Run, Story, TextRange};

pub type FrameProps = dtool_core::frame::FrameProps;
pub type VAlign = dtool_core::frame::VAlign;
pub fn width(props:&FrameProps)->f32 {props.width()}
pub fn height(props:&FrameProps)->f32 {props.height()}
pub fn from_rect(size:[f32;2])->FrameProps {FrameProps::from_rect(size)}
pub fn wrap_width(props:&FrameProps)->f32 {props.wrap_width()}

pub type FrameChain = dtool_core::frame::FrameChain;

#[derive(Debug, Clone)]
pub struct ChainLayout {
    pub per_frame: Vec<Vec<Line>>,
    pub overset: Option<TextRange>,
}

impl Default for ChainLayout {
    fn default() -> Self {
        Self {
            per_frame: Vec::new(),
            overset: None,
        }
    }
}

fn inset_left(insets: [f32; 4]) -> f32 { insets[0] }
fn inset_top(insets: [f32; 4]) -> f32 { insets[1] }
fn inset_right(insets: [f32; 4]) -> f32 { insets[2] }
fn inset_bottom(insets: [f32; 4]) -> f32 { insets[3] }

fn column_widths(props: &FrameProps, content_w: f32) -> Vec<f32> {
    let n = props.columns.max(1) as usize;
    let gutter = if props.gutter.is_finite() && props.gutter > 0.0 { props.gutter } else { 0.0 };
    if let Some(widths) = &props.column_widths {
        if !widths.is_empty() {
            let mut out: Vec<f32> = widths.iter().copied().take(n).collect();
            while out.len() < n {
                out.push(out.last().copied().unwrap_or(0.0));
            }
            return out;
        }
    }
    let gaps = (n.saturating_sub(1)) as f32 * gutter;
    let each = if content_w > gaps { (content_w - gaps) / n as f32 } else { 0.0 };
    vec![each.max(0.0); n]
}

fn remainder_story(story: &Story, from: usize) -> Story {
    let chars: Vec<char> = story.text.chars().collect();
    if from >= chars.len() {
        return Story {
            id: story.id,
            text: String::new(),
            chars: Vec::new(),
            paras: Vec::new(),
            specials: Vec::new(),
            styles: Vec::new(),
            layout_units_per_point: story.layout_units_per_point,
        };
    }
    let text: String = chars[from..].iter().collect();
    fn shift_runs<T: Clone>(runs: &[Run<T>], from: usize, keep_empty: bool) -> Vec<Run<T>> {
        let mut out = Vec::new();
        for run in runs {
            let empty = keep_empty && run.range.start == run.range.end;
            if run.range.end < from || (run.range.end == from && !empty) {
                continue;
            }
            let start = run.range.start.max(from) - from;
            let end = run.range.end - from;
            if start < end || empty {
                out.push(Run {
                    range: TextRange::new(start, end),
                    style: run.style.clone(),
                });
            }
        }
        out
    }
    let specials = story
        .specials
        .iter()
        .filter_map(|(pos, mark)| {
            if *pos < from || (*pos == from && matches!(mark, Special::Break(kind) if from > 0 || *kind != BreakKind::Line)) {
                None
            } else {
                Some((*pos - from, mark.clone()))
            }
        })
        .collect();
    Story {
        id: story.id,
        text,
        chars: shift_runs(&story.chars, from, false),
        paras: shift_runs(&story.paras, from, true),
        specials,
        styles: shift_runs(&story.styles, from, false),
        layout_units_per_point: story.layout_units_per_point,
    }
}

fn remap_lines(lines: &mut [Line], from: usize) {
    if from == 0 {
        return;
    }
    for line in lines {
        line.range.start += from;
        line.range.end += from;
        for g in &mut line.glyphs {
            g.cluster = g.cluster.saturating_add(from);
        }
    }
}

fn typical_gap(lines: &[Line]) -> f32 {
    if lines.len() >= 2 {
        let g = lines[1].baseline - lines[0].baseline;
        if g.is_finite() && g > 0.0 {
            return g;
        }
    }
    if let Some(first) = lines.first() {
        if first.baseline.is_finite() && first.baseline > 0.0 {
            return first.baseline;
        }
    }
    12.0
}

fn line_bottom(lines: &[Line], index: usize) -> f32 {
    let descent = typical_gap(lines) * 0.2;
    lines[index].baseline + descent
}

#[derive(Clone, Copy)]
struct FrameBaseline {
    start: f32,
    spacing: f32,
}

impl FrameBaseline {
    fn next(self, value: f32) -> f32 {
        let start = f64::from(self.start);
        let spacing = f64::from(self.spacing);
        let steps = ((f64::from(value) - start - 1e-5) / spacing).ceil().max(0.0);
        let aligned = (start + steps * spacing) as f32;
        if aligned.is_finite() { aligned } else { value }
    }

    fn for_object(doc: &dtool_core::Document, object: &dtool_core::Object, props: &FrameProps) -> Option<Self> {
        if props.ignore_baseline_grid || !doc.grid.baseline.is_some_and(|grid| grid.enabled) { return None; }
        let matrix = dtool_core::geometry::text_matrix(object);
        if matrix[1].abs() > 1e-5 || matrix[3] <= 0.0 { return None; }
        let page = dtool_core::data_merge::layout::object_page_index(doc, object.id);
        let top = dtool_core::geometry::apply_matrix(&matrix, object.pos)[1];
        let start = (dtool_core::grid::baseline::start_for_page(doc, page)? - top) / matrix[3];
        let spacing = doc.grid.baseline_spacing / matrix[3];
        (start.is_finite() && spacing.is_finite() && spacing > 0.0).then_some(Self { start, spacing })
    }

    fn align_lines(self, story: &Story, lines: &mut [Line], inset: f32) {
        let mut added = 0.0;
        for line in lines {
            let natural = line.baseline + added;
            let aligned = if uses_baseline(story, line) {
                self.next(natural + inset) - inset
            } else {
                natural
            };
            added += (aligned - natural).max(0.0);
            line.baseline = aligned;
        }
    }
}

fn uses_baseline(story: &Story, line: &Line) -> bool {
    para_run_at(story, line.range.start).is_none_or(|style| style.baseline_grid)
}

fn align_column(story: &Story, lines: &mut [Line], baseline: Option<FrameBaseline>, top: f32) -> (Option<FrameBaseline>, Option<f32>) {
    let baseline = baseline.filter(|_| lines.iter().any(|line| uses_baseline(story, line)));
    let descent = baseline.map(|grid| {
        let descent = typical_gap(lines) * 0.2;
        grid.align_lines(story, lines, top);
        descent
    });
    (baseline, descent)
}

fn apply_max_para_space(lines: &mut [Line], max_para_space: f32) {
    if lines.len() < 2 || !max_para_space.is_finite() || max_para_space <= 0.0 {
        return;
    }
    let gap = typical_gap(lines);
    let cap = gap + max_para_space;
    for i in 1..lines.len() {
        let delta = lines[i].baseline - lines[i - 1].baseline;
        if delta > cap {
            let shift = delta - cap;
            for line in lines.iter_mut().skip(i) {
                line.baseline -= shift;
            }
        }
    }
}

fn fitting_count(lines: &[Line], height: f32, auto_height: bool, baseline_descent: Option<f32>) -> usize {
    if auto_height || !height.is_finite() || (height <= 0.0 && baseline_descent.is_none()) {
        return lines.len();
    }
    if let Some(descent) = baseline_descent {
        return lines.iter().take_while(|line| line.baseline + descent <= height + 0.001).count();
    }
    // The first line is allowed in an undersized frame to keep text accessible.
    lines.iter().enumerate().take_while(|(index, line)| {
        *index == 0 || line.baseline + typical_gap(lines) * 0.2 <= height + 0.001
    }).count()
}

fn has_flow_options(style: &crate::story::ParaStyle) -> bool {
    style.keep_together || style.prevent_orphans || style.prevent_widows || style.keep_with_previous || style.keep_with_next > 0
}

fn paragraph_at(paragraphs: &[usize], offset: usize) -> usize {
    paragraphs.partition_point(|start| *start <= offset) - 1
}

fn paragraph_starts(story: &Story) -> Vec<usize> {
    if !story.paras.iter().any(|run| has_flow_options(&run.style)) {
        return Vec::new();
    }
    std::iter::once(0).chain(story.text.chars().enumerate().filter_map(|(index, ch)| {
        matches!(ch, '\n' | '\u{2029}').then_some(index + 1)
    })).collect()
}

fn respect_flow_options(story: &Story, paragraphs: &[usize], rows: &[usize], fitted: usize) -> (usize, bool) {
    if paragraphs.is_empty() || fitted == 0 || fitted >= rows.len() {
        return (fitted, false);
    }
    let paragraph = |offset: usize| paragraph_at(paragraphs, offset);
    let mut cut = fitted;
    loop {
        let left = paragraph(rows[cut - 1]);
        let right = paragraph(rows[cut]);
        let start = rows[..cut].partition_point(|offset| paragraph(*offset) < left);
        let end = cut + rows[cut..].partition_point(|offset| paragraph(*offset) == left);
        let left_count = cut - start;
        let mut next_cut = cut;
        if let Some(style) = para_run_at(story, paragraphs[left]) {
            if left == right {
                let starts_here = rows[start] == paragraphs[left];
                if style.keep_together || (style.prevent_orphans && starts_here && left_count == 1) {
                    next_cut = start;
                } else if style.prevent_widows && end - cut == 1 {
                    next_cut = cut - 1;
                }
            } else if style.keep_with_next > 0 {
                next_cut = cut - 1;
            }
        }
        if next_cut == cut && start > 0 && left == right {
            let previous = paragraph(rows[start - 1]);
            if let Some(style) = para_run_at(story, paragraphs[previous]) {
                let needed = (style.keep_with_next as usize).min(end - start);
                if left_count < needed {
                    next_cut = start - 1;
                }
            }
        }
        if left == right && let Some(next) = rows.get(end).copied().map(paragraph)
            && next == left + 1
            && para_run_at(story, paragraphs[next]).is_some_and(|style| style.keep_with_previous)
        {
            next_cut = next_cut.min(start);
        }
        if right > 0 && para_run_at(story, paragraphs[right]).is_some_and(|style| style.keep_with_previous) {
            let previous = right - 1;
            let boundary = if left == right { start } else { cut };
            if boundary > 0 && paragraph(rows[boundary - 1]) == previous {
                let previous_start = rows[..boundary].partition_point(|offset| paragraph(*offset) < previous);
                next_cut = next_cut.min(previous_start);
            }
        }
        // Keep the natural fit available if the next region cannot fit the group.
        if next_cut == 0 {
            return (fitted, true);
        }
        if next_cut == cut {
            return (cut, false);
        }
        cut = next_cut;
    }
}

fn shift_column(lines: &mut [Line], dx: f32, dy: f32) {
    for line in lines {
        line.baseline += dy;
        if dx != 0.0 {
            for g in &mut line.glyphs {
                g.offset[0] += dx;
            }
        }
    }
}

fn apply_valign(lines: &mut [Line], valign: VAlign, height: f32, auto_height: bool, baseline: Option<FrameBaseline>, descent: Option<f32>) {
    if lines.is_empty() || auto_height || !height.is_finite() || height <= 0.0 {
        return;
    }
    let bottom = descent.map_or_else(|| line_bottom(lines, lines.len() - 1), |descent| lines[lines.len() - 1].baseline + descent);
    let extra = height - bottom;
    if !extra.is_finite() || extra <= 0.0 {
        return;
    }
    let offset = |distance: f32| baseline.map_or(distance, |grid| (distance / grid.spacing).floor() * grid.spacing);
    match valign {
        VAlign::Top => {}
        VAlign::Center => {
            let dy = offset(extra * 0.5);
            for line in lines.iter_mut() {
                line.baseline += dy;
            }
        }
        VAlign::Bottom => {
            for line in lines.iter_mut() {
                line.baseline += offset(extra);
            }
        }
        VAlign::Justify => {
            if lines.len() < 2 {
                return;
            }
            let each = extra / (lines.len() - 1) as f32;
            for (i, line) in lines.iter_mut().enumerate() {
                line.baseline += offset(each * i as f32);
            }
        }
    }
}

fn skip_newline(story: &Story, mut at: usize) -> usize {
    let chars: Vec<char> = story.text.chars().collect();
    if at < chars.len() && matches!(chars[at], '\n' | '\u{2029}') {
        at += 1;
    }
    at
}

fn path_crossings(path: &[[f32; 2]], y: f32) -> Vec<f32> {
    let n = path.len();
    let mut xs = Vec::new();
    if n < 2 {
        return xs;
    }
    for i in 0..n {
        let a = path[i];
        let b = path[(i + 1) % n];
        let y0 = a[1];
        let y1 = b[1];
        let min_y = y0.min(y1);
        let max_y = y0.max(y1);
        if y < min_y || y >= max_y {
            continue;
        }
        let dy = y1 - y0;
        if dy.abs() <= f32::EPSILON {
            continue;
        }
        let t = (y - y0) / dy;
        xs.push(a[0] + t * (b[0] - a[0]));
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs
}

fn path_intervals(path: &[[f32; 2]], y: f32, clip_l: f32, clip_r: f32) -> Vec<(f32, f32)> {
    let xs = path_crossings(path, y);
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < xs.len() {
        let a = xs[i].max(clip_l);
        let b = xs[i + 1].min(clip_r);
        if b - a > 0.5 {
            out.push((a, b));
        }
        i += 2;
    }
    out
}

fn shape_band_spans(path: &[[f32; 2]], [left, right]: [f32; 2], [top, bottom]: [f32; 2]) -> Vec<[f32; 2]> {
    let epsilon = ((bottom - top) * 0.01).min(0.001);
    let mut available = path_intervals(path, top, left, right);
    // Between polygon vertices every edge varies linearly. Sample each side
    // of the vertices to keep concavities out of the entire visual row.
    let samples = std::iter::once(bottom - epsilon).chain(path.iter().flat_map(|point| {
        [point[1] - epsilon, point[1]].into_iter().filter(move |y| *y > top && *y < bottom)
    }));
    for y in samples {
        let sweep = path_intervals(path, y, left, right);
        available = available.into_iter().flat_map(|(a, b)| {
            sweep.iter().filter_map(move |&(l, r)| {
                let start = a.max(l);
                let end = b.min(r);
                (end - start > 0.5).then_some((start, end))
            })
        }).collect();
        if available.is_empty() {
            break;
        }
    }
    available.into_iter().map(|(a, b)| [a, b]).collect()
}

fn widest_interval(intervals: &[(f32, f32)]) -> Option<(f32, f32)> {
    intervals
        .iter()
        .copied()
        .max_by(|a, b| (a.1 - a.0).partial_cmp(&(b.1 - b.0)).unwrap_or(std::cmp::Ordering::Equal))
}

fn first_line_height(line: &Line) -> f32 {
    if line.baseline.is_finite() && line.baseline > 0.0 {
        line.baseline.max(8.0)
    } else {
        14.4
    }
}

struct FrameFlow {
    lines: Vec<Line>,
    next: usize,
    end_break: Option<BreakKind>,
    relaxed_keep: bool,
}

type FollowingRegion<'a> = &'a dyn Fn(usize, bool, usize) -> bool;

#[derive(Clone, Copy)]
struct FramePlacement<'a> {
    following: Option<FollowingRegion<'a>>,
    probe_column: Option<usize>,
}

impl FrameFlow {
    fn fits_group(&self, start: usize, at_least: usize) -> bool {
        !self.relaxed_keep && self.next > start && self.next >= at_least
    }
}

fn layout_one_frame_shaped(
    lib: &FontLibrary,
    story: &Story,
    lists: &ListLayout,
    paragraphs: &[usize],
    props: &FrameProps,
    path: &[[f32; 2]],
    mut consumed: usize,
    baseline: Option<FrameBaseline>,
    passed_start_break: bool,
    following: Option<FollowingRegion<'_>>,
) -> FrameFlow {
    let left = inset_left(props.insets);
    let top = inset_top(props.insets);
    let right = inset_right(props.insets);
    let bottom = inset_bottom(props.insets);
    let content_h = (props.size[1] - top - bottom).max(0.0);
    let clip_l = left;
    let clip_r = (props.size[0] - right).max(clip_l);
    let total = story.text.chars().count();
    let mut frame_lines = Vec::new();
    let mut rows = IrregularRows::default();
    let mut end_break = leading_break(story, consumed, total, passed_start_break);
    let mut y = top;
    let mut guard = 0u32;
    while end_break.is_none() && consumed < total && y < top + content_h && guard < 10_000 {
        guard += 1;
        let rem = remainder_story(story, consumed);
        if rem.text.is_empty() {
            break;
        }
        let probe_y = y + 6.0;
        if probe_y >= top + content_h {
            break;
        }
        let intervals = path_intervals(path, probe_y, clip_l, clip_r);
        let Some((x0, x1)) = widest_interval(&intervals) else {
            y += 2.0;
            continue;
        };
        let wrap = (x1 - x0).max(0.0);
        if wrap < 0.5 {
            y += 2.0;
            continue;
        }
        let mut lines = layout_story_at(lib, &rem, Some(wrap), consumed, lists);
        if lines.is_empty() {
            break;
        }
        remap_lines(&mut lines, consumed);
        let mut first = lines.remove(0);
        let height = first_line_height(&first);
        let natural = y + height * 0.8;
        let line_grid = baseline.filter(|_| uses_baseline(story, &first));
        let aligned = line_grid.map_or(natural, |grid| grid.next(natural));
        if aligned - natural > 1e-4 {
            y += aligned - natural;
            continue;
        }
        if !props.auto_height && y - top + height > content_h + 0.001 && (line_grid.is_some() || !frame_lines.is_empty()) {
            break;
        }
        first.baseline = aligned;
        for g in &mut first.glyphs {
            g.offset[0] += x0;
        }
        let stop = first.range.end;
        let forced = line_end_break(story, &first);
        if !paragraphs.is_empty() {
            rows.record(consumed, frame_lines.len(), &[(x0, x1)]);
        }
        consumed = skip_newline(story, stop);
        frame_lines.push(first);
        y += height;
        end_break = forced.or_else(|| hard_break_at(story, consumed));
    }
    let mut relaxed_keep = false;
    if end_break.is_none() {
        let (next, relaxed) = rows.trim(lib, story, lists, paragraphs, &mut frame_lines, consumed, baseline.is_some(), false);
        consumed = next;
        relaxed_keep = relaxed;
        if relaxed && let Some((start, _)) = rows.rows.first()
            && following.is_some_and(|check| check(*start, passed_start_break, consumed))
        {
            frame_lines.clear();
            consumed = *start;
            relaxed_keep = false;
        }
    }
    if !baseline.is_some_and(|_| frame_lines.iter().any(|line| uses_baseline(story, line))) {
        apply_max_para_space(&mut frame_lines, props.max_para_space);
        apply_valign(&mut frame_lines, props.valign, content_h, props.auto_height, None, None);
    }
    FrameFlow { lines: frame_lines, next: consumed, end_break, relaxed_keep }
}

fn layout_one_frame(lib: &FontLibrary, story: &Story, lists: &ListLayout, paragraphs: &[usize], props: &FrameProps, mut consumed: usize, baseline: Option<FrameBaseline>, passed_start_break: bool, placement: FramePlacement<'_>, spans: Option<LineSpans<'_>>) -> FrameFlow {
    let shape = props.shape_path.as_deref().filter(|path| path.len() >= 3);
    if let Some(path) = shape && spans.is_none() {
        return layout_one_frame_shaped(lib, story, lists, paragraphs, props, path, consumed, baseline, passed_start_break, placement.following);
    }
    let shape_wrap = |clip, band| {
        let (Some(path), Some(wrap)) = (shape, spans) else { return Vec::new() };
        shape_band_spans(path, clip, band).into_iter().flat_map(|piece| wrap(piece, band)).collect()
    };
    let active_spans: Option<LineSpans<'_>> = if shape.is_some() { Some(&shape_wrap) } else { spans };
    let left = inset_left(props.insets);
    let top = inset_top(props.insets);
    let right = inset_right(props.insets);
    let bottom = inset_bottom(props.insets);
    let content_w = (props.size[0] - left - right).max(0.0);
    let content_h = (props.size[1] - top - bottom).max(0.0);
    let widths = column_widths(props, content_w);
    let gutter = if props.gutter.is_finite() && props.gutter > 0.0 { props.gutter } else { 0.0 };
    let total = story.text.chars().count();
    let mut frame_lines = Vec::new();
    let mut end_break = None;
    let mut passed = passed_start_break.then_some(consumed);
    let mut relaxed_keep = false;
    let first = placement.probe_column.unwrap_or(0);
    let mut col_x = left + widths.iter().take(first).map(|width| width + gutter).sum::<f32>();
    let count = if placement.probe_column.is_some() { 1 } else { widths.len() };
    for (ci, &col_w) in widths.iter().enumerate().skip(first).take(count) {
        if consumed >= total {
            break;
        }
        if passed != Some(consumed) && let Some(kind) = hard_break_at(story, consumed) {
            passed = Some(consumed);
            end_break = Some(kind);
            if kind != BreakKind::Column {
                break;
            }
            col_x += col_w + gutter;
            continue;
        }
        let column_start = consumed;
        let auto = props.auto_height && ci + 1 == widths.len();
        let region = ColumnRegion { span: [col_x, col_x + col_w], top, height: content_h, auto, strict_shape: shape.is_some() };
        let wraps_column = active_spans.filter(|hook| {
            region.strict_shape || widths.len() == 1 || hook(region.span, [top, top + content_h]).as_slice() != [region.span]
        });
        let mut column = if let Some(hook) = wraps_column {
            layout_wrapped_column(lib, story, lists, paragraphs, props, hook, region, consumed, baseline)
        } else {
            layout_rect_column(lib, story, lists, paragraphs, props, region, consumed, baseline)
        };
        if column.relaxed_keep && placement.probe_column.is_none() {
            let already_passed = passed == Some(column_start);
            let can_move = if ci + 1 < widths.len() {
                let probe = FramePlacement { following: None, probe_column: Some(ci + 1) };
                layout_one_frame(lib, story, lists, paragraphs, props, column_start, baseline, already_passed, probe, spans)
                    .fits_group(column_start, column.next)
            } else {
                placement.following.is_some_and(|check| check(column_start, already_passed, column.next))
            };
            if can_move {
                col_x += col_w + gutter;
                continue;
            }
        }
        relaxed_keep |= column.relaxed_keep;
        consumed = column.next;
        frame_lines.append(&mut column.lines);
        if consumed != column_start {
            end_break = None;
            passed = None;
        }
        if let Some(kind) = column.end_break {
            end_break = Some(kind);
            passed = Some(consumed);
            if kind != BreakKind::Column {
                break;
            }
        }
        col_x += col_w + gutter;
    }
    FrameFlow { lines: frame_lines, next: consumed, end_break, relaxed_keep }
}

#[derive(Clone, Copy)]
struct ColumnRegion {
    span: [f32; 2],
    top: f32,
    height: f32,
    auto: bool,
    strict_shape: bool,
}

fn layout_rect_column(
    lib: &FontLibrary, story: &Story, lists: &ListLayout, paragraphs: &[usize],
    props: &FrameProps, region: ColumnRegion, start: usize, baseline: Option<FrameBaseline>,
) -> FrameFlow {
    let rem = remainder_story(story, start);
    let width = region.span[1] - region.span[0];
    let wrap = (width.is_finite() && width > 0.0).then_some(width);
    let mut lines = layout_story_at(lib, &rem, wrap, start, lists);
    remap_lines(&mut lines, start);
    apply_max_para_space(&mut lines, props.max_para_space);
    let explicit_break = lines.iter().enumerate().find_map(|(index, line)| {
        line_end_break(story, line).map(|kind| (index, kind))
    });
    if let Some((index, _)) = explicit_break {
        lines.truncate(index + 1);
    }
    let (baseline, descent) = align_column(story, &mut lines, baseline, region.top);
    let fitted = fitting_count(&lines, region.height, region.auto, descent);
    let (cut, relaxed_keep) = if paragraphs.is_empty() || fitted == lines.len() {
        (fitted, false)
    } else {
        let rows: Vec<_> = lines.iter().map(|line| line.range.start).collect();
        respect_flow_options(story, paragraphs, &rows, fitted)
    };
    let overflow = lines.get(cut).map(|line| line.range.start);
    lines.truncate(cut);
    apply_valign(&mut lines, props.valign, region.height, props.auto_height, baseline, descent);
    shift_column(&mut lines, region.span[0], region.top);
    let next = overflow.or_else(|| lines.last().map(|line| skip_newline(story, line.range.end))).unwrap_or(start);
    let end_break = explicit_break.filter(|_| overflow.is_none()).map(|(_, kind)| kind);
    FrameFlow { lines, next, end_break, relaxed_keep }
}

pub type LineSpans<'a> = &'a dyn Fn([f32; 2], [f32; 2]) -> Vec<[f32; 2]>;

fn hard_break(kind: Option<BreakKind>) -> Option<BreakKind> {
    kind.filter(|kind| *kind != BreakKind::Line)
}

fn hard_break_at(story: &Story, offset: usize) -> Option<BreakKind> {
    story.specials.iter().find_map(|(at, special)| match special {
        Special::Break(kind) if *at == offset => hard_break(Some(*kind)),
        _ => None,
    })
}

fn line_end_break(story: &Story, line: &Line) -> Option<BreakKind> {
    hard_break(line.break_after).or_else(|| {
        let end = line.range.end;
        if end > line.range.start && let Some(kind) = hard_break_at(story, end) {
            return Some(kind);
        }
        // A boundary at the next paragraph's start belongs to this region.
        hard_break_at(story, end.saturating_add(1)).filter(|_| {
            story.text.chars().nth(end).is_some_and(|ch| matches!(ch, '\n' | '\u{2029}'))
        })
    })
}

fn leading_break(story: &Story, offset: usize, total: usize, passed: bool) -> Option<BreakKind> {
    if passed || offset >= total { None } else { hard_break_at(story, offset) }
}

fn horizontal_extent(line: &Line) -> [f32; 2] {
    let mut pen = 0.0;
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for glyph in &line.glyphs {
        let x = pen + glyph.offset[0];
        pen += glyph.advance;
        left = left.min(x);
        right = right.max(x + glyph.advance);
    }
    [left, right]
}

enum BandFill {
    Taller(f32),
    Segments(Vec<(Line, f32)>, f32, usize, Option<BreakKind>),
    Empty,
}

fn fill_band(lib: &FontLibrary, story: &Story, lists: &ListLayout, pieces: &[(f32, f32)], band: f32, consumed: usize, strict: bool, baseline: bool, paragraphs: &[usize]) -> BandFill {
    let total = story.text.chars().count();
    let mut local = consumed;
    let mut segments = Vec::new();
    let mut tallest = 0.0f32;
    for &(x0, x1) in pieces {
        if local >= total {
            break;
        }
        let rem = remainder_story(story, local);
        if rem.text.is_empty() {
            break;
        }
        let mut lines = layout_story_at(lib, &rem, Some(x1 - x0), local, lists);
        if lines.is_empty() {
            break;
        }
        remap_lines(&mut lines, local);
        let first = lines.remove(0);
        if strict {
            let [left, right] = horizontal_extent(&first);
            if left < -0.5 || right > x1 - x0 + 0.5 {
                continue;
            }
        }
        // Keep fragments with differing baseline settings on separate visual rows.
        if baseline && segments.first().is_some_and(|(line, _)| uses_baseline(story, line) != uses_baseline(story, &first)) {
            break;
        }
        if !paragraphs.is_empty() && segments.last().is_some_and(|(previous, _)| {
            let before = paragraph_at(paragraphs, previous.range.start);
            let after = paragraph_at(paragraphs, first.range.start);
            before != after && [before, after].into_iter().any(|index| {
                para_run_at(story, paragraphs[index]).is_some_and(has_flow_options)
            })
        }) {
            break;
        }
        let height = first_line_height(&first);
        if height > band + 0.01 {
            return BandFill::Taller(height.max(tallest));
        }
        tallest = tallest.max(height);
        local = skip_newline(story, first.range.end);
        let stop = line_end_break(story, &first).or_else(|| hard_break_at(story, local));
        segments.push((first, x0));
        if stop.is_some() {
            return BandFill::Segments(segments, tallest, local, stop);
        }
    }
    if segments.is_empty() {
        BandFill::Empty
    } else {
        BandFill::Segments(segments, tallest, local, None)
    }
}

#[derive(Default)]
struct IrregularRows {
    rows: Vec<(usize, usize)>,
    spans: Vec<(f32, f32)>,
}

impl IrregularRows {
    fn record(&mut self, offset: usize, line_index: usize, spans: &[(f32, f32)]) {
        self.rows.push((offset, line_index));
        self.spans.clear();
        self.spans.extend_from_slice(spans);
    }

    fn trim(&self, lib: &FontLibrary, story: &Story, lists: &ListLayout, paragraphs: &[usize], lines: &mut Vec<Line>, consumed: usize, baseline: bool, strict_shape: bool) -> (usize, bool) {
        if self.rows.is_empty() {
            return (consumed, false);
        }
        let total = story.text.chars().count();
        if consumed >= total {
            return (consumed, false);
        }
        let mut starts: Vec<_> = self.rows.iter().map(|(offset, _)| *offset).collect();
        let fitted = starts.len();
        let mut at = consumed;
        // Inspect the continuation at the last usable geometry. A band of
        // fragments is one visual row, and nine rows cover all keep counts.
        for _ in 0..9 {
            if at >= total {
                break;
            }
            let mut fill = fill_band(lib, story, lists, &self.spans, f32::INFINITY, at, strict_shape || self.spans.len() > 1, baseline, paragraphs);
            if !strict_shape && matches!(fill, BandFill::Empty) && self.spans.len() > 1 {
                let widest = widest_interval(&self.spans).expect("recorded band has spans");
                fill = fill_band(lib, story, lists, &[widest], f32::INFINITY, at, false, baseline, paragraphs);
            }
            let BandFill::Segments(_, _, next, forced) = fill else { break };
            starts.push(at);
            if forced.is_some() || next <= at {
                break;
            }
            at = next;
        }
        let (cut, relaxed) = respect_flow_options(story, paragraphs, &starts, fitted);
        if cut == fitted {
            return (consumed, relaxed);
        }
        let (offset, line_index) = self.rows[cut];
        lines.truncate(line_index);
        (offset, false)
    }
}

fn layout_wrapped_column(
    lib: &FontLibrary, story: &Story, lists: &ListLayout, paragraphs: &[usize],
    props: &FrameProps, spans: LineSpans<'_>, region: ColumnRegion,
    mut consumed: usize, baseline: Option<FrameBaseline>,
) -> FrameFlow {
    let [clip_l, clip_r] = region.span;
    let top = region.top;
    let content_h = region.height;
    let total = story.text.chars().count();
    let mut frame_lines = Vec::new();
    let mut rows = IrregularRows::default();
    let adjust_geometry = region.strict_shape && (props.valign != VAlign::Top || props.max_para_space > 0.0);
    let mut heights = Vec::new();
    let mut end_break = None;
    let mut y = top;
    let mut band = 14.4f32;
    let mut guard = 0u32;
    while end_break.is_none() && consumed < total && y < top + content_h && guard < 10_000 {
        guard += 1;
        if remainder_story(story, consumed).text.is_empty() {
            break;
        }
        let band_end = if region.strict_shape { (y + band).min(top + content_h) } else { y + band };
        let mut pieces: Vec<(f32, f32)> = spans([clip_l, clip_r], [y, band_end]).into_iter().map(|s| (s[0].max(clip_l), s[1].min(clip_r))).filter(|(a, b)| b - a > 0.5).collect();
        pieces.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let Some(widest) = widest_interval(&pieces) else {
            y += 2.0;
            continue;
        };
        let mut pieces = pieces.as_slice();
        let fallback = [widest];
        let mut fill = fill_band(lib, story, lists, pieces, band, consumed, region.strict_shape || pieces.len() > 1, baseline.is_some(), paragraphs);
        if !region.strict_shape && matches!(fill, BandFill::Empty) && pieces.len() > 1 {
            pieces = &fallback;
            fill = fill_band(lib, story, lists, pieces, band, consumed, false, baseline.is_some(), paragraphs);
        }
        let (segments, height, next, stop) = match fill {
            BandFill::Taller(height) => {
                band = height;
                continue;
            }
            BandFill::Empty if region.strict_shape => {
                y += 2.0;
                continue;
            }
            BandFill::Empty => break,
            BandFill::Segments(segments, height, next, stop) => (segments, height, next, stop),
        };
        band = height;
        let natural = y + height * 0.8;
        let line_grid = baseline.filter(|_| uses_baseline(story, &segments[0].0));
        let aligned = line_grid.map_or(natural, |grid| grid.next(natural));
        if aligned - natural > 1e-4 {
            y += aligned - natural;
            continue;
        }
        if y - top + height > content_h + 0.001
            && (region.strict_shape || !region.auto && (line_grid.is_some() || !frame_lines.is_empty()))
        {
            break;
        }
        if !paragraphs.is_empty() {
            rows.record(consumed, frame_lines.len(), pieces);
        }
        for (mut line, x0) in segments {
            line.baseline = aligned;
            for g in &mut line.glyphs {
                g.offset[0] += x0;
            }
            frame_lines.push(line);
            if adjust_geometry { heights.push(height); }
        }
        consumed = next;
        y += height;
        end_break = stop;
    }
    let mut relaxed_keep = false;
    if end_break.is_none() {
        let (next, relaxed) = rows.trim(lib, story, lists, paragraphs, &mut frame_lines, consumed, baseline.is_some(), region.strict_shape);
        consumed = next;
        relaxed_keep = relaxed;
    }
    if !baseline.is_some_and(|_| frame_lines.iter().any(|line| uses_baseline(story, line))) {
        if !region.strict_shape {
            apply_max_para_space(&mut frame_lines, props.max_para_space);
            apply_valign(&mut frame_lines, props.valign, content_h, props.auto_height, None, None);
        } else if adjust_geometry {
            let mut adjusted = frame_lines.clone();
            apply_max_para_space(&mut adjusted, props.max_para_space);
            apply_valign(&mut adjusted, props.valign, content_h, props.auto_height, None, None);
            if adjusted.iter().zip(heights).all(|(line, height)| {
                let y = line.baseline - height * 0.8;
                let [left, right] = horizontal_extent(line);
                y >= top - 0.001 && y + height <= top + content_h + 0.001
                    && spans(region.span, [y.max(top), (y + height).min(top + content_h)])
                        .iter().any(|&[a, b]| left >= a - 0.5 && right <= b + 0.5)
            }) {
                frame_lines = adjusted;
            }
        }
    }
    FrameFlow { lines: frame_lines, next: consumed, end_break, relaxed_keep }
}

pub fn layout_chain(lib: &FontLibrary, story: &Story, frames: &[FrameProps]) -> ChainLayout {
    layout_chain_wrapped(lib, story, frames, &[])
}

pub fn layout_chain_wrapped(lib: &FontLibrary, story: &Story, frames: &[FrameProps], spans: &[Option<LineSpans>]) -> ChainLayout {
    layout_chain_with_baselines(lib, story, frames, spans, &[], &[])
}

fn paragraph_start_breaks(story: &Story) -> Cow<'_, Story> {
    if !story.paras.iter().any(|run| run.style.flow_start != FlowStart::Anywhere) {
        return Cow::Borrowed(story);
    }
    let mut breaks = Vec::new();
    let mut add = |start: usize, separator: usize| {
        let Some(style) = para_run_at(story, start) else { return };
        let kind = match style.flow_start {
            FlowStart::Anywhere => return,
            FlowStart::NextColumn => BreakKind::Column,
            FlowStart::NextFrame => BreakKind::Frame,
            FlowStart::NextPage => BreakKind::Page,
            FlowStart::NextOddPage => BreakKind::OddPage,
            FlowStart::NextEvenPage => BreakKind::EvenPage,
        };
        if hard_break_at(story, separator).is_none() && hard_break_at(story, start).is_none() {
            breaks.push((start, Special::Break(kind)));
        }
    };
    add(0, 0);
    let mut chars = story.text.chars().enumerate().peekable();
    while let Some((offset, ch)) = chars.next() {
        if matches!(ch, '\n' | '\u{2029}') && chars.peek().is_some() {
            add(offset + 1, offset);
        }
    }
    if breaks.is_empty() {
        return Cow::Borrowed(story);
    }
    // These boundaries affect frame routing without becoming editable story marks.
    let mut routed = story.clone();
    routed.specials.extend(breaks);
    routed.specials.sort_by_key(|(at, _)| *at);
    Cow::Owned(routed)
}

fn layout_chain_with_baselines(lib: &FontLibrary, story: &Story, frames: &[FrameProps], spans: &[Option<LineSpans>], baselines: &[Option<FrameBaseline>], pages: &[usize]) -> ChainLayout {
    let total = story.text.chars().count();
    let mut per_frame = Vec::with_capacity(frames.len());
    let mut consumed = 0usize;
    if frames.is_empty() {
        let overset = if total > 0 { Some(TextRange::new(0, total)) } else { None };
        return ChainLayout { per_frame, overset };
    }
    let routed = paragraph_start_breaks(story);
    let story = routed.as_ref();
    let lists = ListLayout::new(lib, story);
    let paragraphs = paragraph_starts(story);
    let layout_frame = |index: usize, start, passed, placement: FramePlacement<'_>| {
        let props = &frames[index];
        let baseline = baselines.get(index).copied().flatten();
        layout_one_frame(lib, story, &lists, &paragraphs, props, start, baseline, passed, placement,
            spans.get(index).copied().flatten())
    };
    let mut passed_break = false;
    let mut target = 0;
    for index in 0..frames.len() {
        if index < target {
            per_frame.push(Vec::new());
            continue;
        }
        let following = |start, passed, at_least| {
            index + 1 < frames.len() && layout_frame(index + 1, start, passed,
                FramePlacement { following: None, probe_column: Some(0) }).fits_group(start, at_least)
        };
        let placed = layout_frame(index, consumed, passed_break,
            FramePlacement { following: Some(&following), probe_column: None });
        passed_break = placed.end_break.is_some() || (passed_break && placed.next == consumed);
        consumed = placed.next;
        per_frame.push(placed.lines);
        if let (Some(origin), Some(kind @ (BreakKind::Page | BreakKind::OddPage | BreakKind::EvenPage))) =
            (pages.get(index).copied(), placed.end_break)
        {
            target = (index + 1..pages.len()).find(|&candidate| {
                let page = pages[candidate];
                page > origin && match kind {
                    BreakKind::OddPage => page % 2 == 0,
                    BreakKind::EvenPage => page % 2 == 1,
                    _ => true,
                }
            }).unwrap_or(frames.len());
        }
    }
    let overset = if consumed < total {
        Some(TextRange::new(consumed, total))
    } else {
        None
    };
    ChainLayout { per_frame, overset }
}

#[derive(Debug, Clone)]
pub struct DocFlow {
    pub frames: Vec<ObjectId>,
    pub layout: ChainLayout,
}

impl DocFlow {
    pub fn frame_range(&self, index: usize) -> Option<TextRange> {
        let lines = self.layout.per_frame.get(index)?;
        Some(TextRange::new(lines.first()?.range.start, lines.last()?.range.end))
    }

    pub fn last_frame(&self) -> Option<ObjectId> {
        self.frames.last().copied()
    }
}

struct DocLayoutReuse {
    document: *const dtool_core::Document,
    library: *const FontLibrary,
    font_generation: u64,
    layouts: HashMap<dtool_core::StoryId, Rc<DocFlow>>,
    artistic_layouts: HashMap<dtool_core::StoryId, Rc<(Story, Vec<Line>)>>,
}

impl DocLayoutReuse {
    fn matches(&self, document: &dtool_core::Document, library: &FontLibrary) -> bool {
        std::ptr::eq(self.document, document) && std::ptr::eq(self.library, library)
            && self.font_generation == dtool_core::text_lines::font_generation()
    }
}

thread_local! {
    static DOC_LAYOUT_REUSE: RefCell<Option<DocLayoutReuse>> = const { RefCell::new(None) };
}

/// Reuse text layouts for one immutable document and font library during a render.
/// Temporary documents, such as font hover previews, are laid out separately.
pub fn with_doc_layout_reuse<T>(document: &dtool_core::Document, library: &FontLibrary, render: impl FnOnce() -> T) -> T {
    if DOC_LAYOUT_REUSE.with(|slot| slot.borrow().as_ref().is_some_and(|cache| cache.matches(document, library))) {
        return render();
    }
    struct Restore(Option<DocLayoutReuse>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DOC_LAYOUT_REUSE.with(|slot| slot.replace(self.0.take()));
        }
    }
    let previous = DOC_LAYOUT_REUSE.with(|slot| slot.replace(Some(DocLayoutReuse {
        document,
        library,
        font_generation: dtool_core::text_lines::font_generation(),
        layouts: HashMap::new(),
        artistic_layouts: HashMap::new(),
    })));
    let _restore = Restore(previous);
    render()
}

pub(crate) fn shared_artistic_layout(lib: &FontLibrary, doc: &dtool_core::Document, id: dtool_core::StoryId) -> Option<Rc<(Story, Vec<Line>)>> {
    if let Some(layout) = DOC_LAYOUT_REUSE.with(|slot| {
        slot.borrow().as_ref().filter(|cache| cache.matches(doc, lib))?.artistic_layouts.get(&id).cloned()
    }) {
        return Some(layout);
    }
    let core = doc.stories.iter().find(|story| story.id == id)?;
    let story = crate::bridge::story_in_document(core, doc);
    let lines = crate::layout::layout_story(lib, &story, None);
    let layout = Rc::new((story, lines));
    DOC_LAYOUT_REUSE.with(|slot| {
        if let Some(cache) = slot.borrow_mut().as_mut().filter(|cache| cache.matches(doc, lib)) {
            cache.artistic_layouts.insert(id, layout.clone());
        }
    });
    Some(layout)
}

pub(crate) fn shared_doc_flow(lib: &FontLibrary, doc: &dtool_core::Document, story: dtool_core::StoryId) -> Option<Rc<DocFlow>> {
    if let Some(layout) = DOC_LAYOUT_REUSE.with(|slot| {
        slot.borrow().as_ref().filter(|cache| cache.matches(doc, lib))?.layouts.get(&story).cloned()
    }) {
        return Some(layout);
    }
    let layout = Rc::new(compute_doc_flow(lib, doc, story)?);
    DOC_LAYOUT_REUSE.with(|slot| {
        if let Some(cache) = slot.borrow_mut().as_mut().filter(|cache| cache.matches(doc, lib)) {
            cache.layouts.insert(story, layout.clone());
        }
    });
    Some(layout)
}

pub fn layout_doc_flow(lib: &FontLibrary, doc: &dtool_core::Document, story: dtool_core::StoryId) -> Option<DocFlow> {
    shared_doc_flow(lib, doc, story).map(Rc::unwrap_or_clone)
}

fn compute_doc_flow(lib: &FontLibrary, doc: &dtool_core::Document, story: dtool_core::StoryId) -> Option<DocFlow> {
    let core = doc.stories.iter().find(|s| s.id == story)?;
    let frames = dtool_core::frame::flow_frames(doc, story);
    let props: Vec<FrameProps> = frames.iter().filter_map(|id| doc.get_object(*id)?.frame_props()).collect();
    if props.len() != frames.len() {
        return None;
    }
    let wrappers: Vec<Vec<(dtool_core::text_wrap::TextWrap, [f32; 4])>> = frames
        .iter()
        .zip(&props)
        .map(|(id, props)| {
            let left = inset_left(props.insets);
            let top = inset_top(props.insets);
            let right = props.size[0] - inset_right(props.insets);
            let bottom = props.size[1] - inset_bottom(props.insets);
            let mut wrappers = dtool_core::commands::text_wrap_cmd::frame_wrappers(&doc.text_wraps, doc, *id);
            wrappers.retain(|(wrap, bounds)| wrap.exclusion(*bounds).is_some_and(|[x, y, w, h]| {
                x < right && x + w > left && y < bottom && y + h > top
            }));
            wrappers
        })
        .collect();
    let hooks: Vec<Box<dyn Fn([f32; 2], [f32; 2]) -> Vec<[f32; 2]> + '_>> = wrappers
        .iter()
        .map(|list| Box::new(move |span: [f32; 2], band: [f32; 2]| dtool_core::text_wrap::line_spans(span, band, list)) as Box<dyn Fn([f32; 2], [f32; 2]) -> Vec<[f32; 2]> + '_>)
        .collect();
    let spans: Vec<Option<LineSpans>> = wrappers.iter().zip(hooks.iter()).map(|(list, hook)| (!list.is_empty()).then_some(hook.as_ref() as LineSpans)).collect();
    let baselines: Vec<_> = frames.iter().zip(&props).map(|(id, props)| FrameBaseline::for_object(doc, doc.get_object(*id)?, props)).collect();
    let pages: Vec<_> = frames.iter().map(|id| dtool_core::data_merge::layout::object_page_index(doc, *id)).collect();
    let layout = layout_chain_with_baselines(lib, &crate::bridge::story_in_document(core, doc), &props, &spans, &baselines, &pages);
    Some(DocFlow { frames, layout })
}

pub fn overset_frames(lib: &FontLibrary, doc: &dtool_core::Document) -> Vec<ObjectId> {
    let mut stories: Vec<dtool_core::StoryId> = Vec::new();
    for object in &doc.objects {
        if let dtool_core::ObjectKind::FrameText { story } = object.kind {
            if !stories.contains(&story) {
                stories.push(story);
            }
        }
    }
    stories
        .into_iter()
        .filter_map(|story| shared_doc_flow(lib, doc, story))
        .filter(|flow| flow.layout.overset.is_some())
        .filter_map(|flow| flow.last_frame())
        .collect()
}

pub fn flow_ranges_with(lib: &FontLibrary, doc: &dtool_core::Document, story: dtool_core::StoryId) -> Option<Vec<dtool_core::longdoc::pages::FrameRange>> {
    let flow = layout_doc_flow(lib, doc, story)?;
    let mut out = Vec::with_capacity(flow.frames.len());
    let mut prev = 0usize;
    for (i, frame) in flow.frames.iter().enumerate() {
        let (start, end) = flow.frame_range(i).map(|r| (r.start, r.end)).unwrap_or((prev, prev));
        out.push(dtool_core::longdoc::pages::FrameRange { frame: *frame, start, end });
        prev = end;
    }
    Some(out)
}

fn flow_ranges_shared(doc: &dtool_core::Document, story: dtool_core::StoryId) -> Option<Vec<dtool_core::longdoc::pages::FrameRange>> {
    flow_ranges_with(&FontLibrary::shared(), doc, story)
}

pub fn caret_point_with(lib: &FontLibrary, doc: &dtool_core::Document, story_pos: usize) -> Option<dtool_core::longdoc::marks::CaretPoint> {
    let (story, at) = doc.story_at_pos(story_pos)?;
    let size = doc.stories.iter().find(|s| s.id == story).map(|s| dtool_core::text_cmd::size_pt_of(s, dtool_core::text_cmd::DEFAULT_SIZE_PT))?
        * dtool_core::units::units_per_point(doc.units, doc.dpi);
    let flow = layout_doc_flow(lib, doc, story)?;
    for (i, frame) in flow.frames.iter().enumerate() {
        let Some(lines) = flow.layout.per_frame.get(i) else { continue };
        let Some(line) = lines.iter().rev().find(|l| at >= l.range.start && at <= l.range.end) else { continue };
        let object = doc.get_object(*frame)?;
        let x = caret_x(line, at);
        let top = line.baseline - size * 0.8;
        return Some(dtool_core::longdoc::marks::CaretPoint { frame: *frame, point: [object.pos[0] + x, object.pos[1] + top], line_height: size * 1.2 });
    }
    None
}

fn caret_point_shared(doc: &dtool_core::Document, story_pos: usize) -> Option<dtool_core::longdoc::marks::CaretPoint> {
    caret_point_with(&FontLibrary::shared(), doc, story_pos)
}

pub fn install_flow_ranges() -> bool {
    let ranges = dtool_core::longdoc::pages::register_flow_ranges(flow_ranges_shared);
    let caret = dtool_core::longdoc::marks::register_caret_locator(caret_point_shared);
    ranges && caret
}

pub fn layout_in_frame(lib: &FontLibrary, story: &Story, size: [f32; 2]) -> ChainLayout {
    layout_chain(lib, story, &[FrameProps::from_rect(size)])
}

pub fn caret_x(line: &Line, at: usize) -> f32 {
    let mut pen = 0.0;
    for glyph in line.body_glyphs() {
        if glyph.cluster >= at {
            return pen + glyph.offset[0];
        }
        pen += glyph.advance;
    }
    pen + line.body_glyphs().last().map_or(0.0, |glyph| glyph.offset[0])
}

pub fn caret_at_point(lines: &[Line], point: [f32; 2]) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let mut best = 0usize;
    let mut best_d = f32::MAX;
    for (i, line) in lines.iter().enumerate() {
        let d = (line.baseline - point[1]).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    let line = &lines[best];
    if line.body_glyphs().is_empty() {
        return line.range.start;
    }
    let mut pen = 0.0;
    for g in line.body_glyphs() {
        let mid = pen + g.offset[0] + g.advance * 0.5;
        if point[0] < mid {
            return g.cluster;
        }
        pen += g.advance;
    }
    line.range.end
}

#[cfg(test)]
mod native_frame_contracts {
    use super::*;
    use dtool_core::{Command,Document,Session};
    #[test]
    fn native_hundred_by_fifty_frame_reaches_real_wrapped_layout() {
        let mut session=Session::new(Document::new([400.0,200.0],dtool_core::document::Units::Px,72.0,1));
        let mut command=Command::new("text.frame.add");command.params["rect"]=vec![0.0f64,0.0,100.0,50.0].into();
        command.params["text"]="alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron".into();
        let reply=session.run(&command).unwrap();let id=dtool_core::ObjectId(reply["id"].as_u64().unwrap());
        let object=session.document.get_object(id).unwrap();let dtool_core::ObjectKind::FrameText{story}=&object.kind else {panic!("wrong object type");};
        let frame=object.frame_props().expect("frame props");assert_eq!(frame.size,[100.0,50.0]);let core=session.document.stories.iter().find(|s|s.id==*story).unwrap();
        let story=crate::bridge::story_from_core(core);let library=FontLibrary::shared();let layout=layout_in_frame(&library,&story,frame.size);let via_chain=layout_chain(&library,&story,std::slice::from_ref(&frame));assert_eq!(layout.per_frame.len(),via_chain.per_frame.len());
        assert_eq!(layout.per_frame.len(),1);assert!(layout.per_frame[0].len()>=2);assert!(layout.per_frame[0].iter().all(|line|line.width<=100.001));
        assert_eq!(width(&frame),100.0);assert_eq!(height(&frame),50.0);
        session.run(&Command::new("edit.undo")).unwrap();assert!(session.document.get_object(id).is_none());assert!(session.document.stories.is_empty());
        session.run(&Command::new("edit.redo")).unwrap();assert!(session.document.get_object(id).is_some());
    }
    #[test]
    fn linked_frame_continues_overset_text_and_undo_unlinks() {
        let mut session=Session::new(Document::new([400.0,400.0],dtool_core::document::Units::Px,72.0,1));
        let mut add=Command::new("text.frame.add");add.params["rect"]=vec![0.0f64,0.0,100.0,30.0].into();
        add.params["text"]="alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega".into();
        let a=dtool_core::ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
        let mut add_b=Command::new("text.frame.add");add_b.params["rect"]=vec![150.0f64,0.0,100.0,300.0].into();
        let b=dtool_core::ObjectId(session.run(&add_b).unwrap()["id"].as_u64().unwrap());
        let library=FontLibrary::shared();
        assert_eq!(overset_frames(&library,&session.document),vec![a]);
        let mut link=Command::new("text.link");link.params["from"]=a.0.into();link.params["to"]=b.0.into();session.run(&link).unwrap();
        let story=dtool_core::frame::frame_story(&session.document,a).unwrap();
        assert_eq!(dtool_core::frame::frame_story(&session.document,b),Some(story));
        let flow=layout_doc_flow(&library,&session.document,story).unwrap();
        assert_eq!(flow.frames,vec![a,b]);assert!(flow.layout.overset.is_none());
        let first=flow.frame_range(0).unwrap();let second=flow.frame_range(1).unwrap();assert!(second.start>=first.end);assert!(second.end>second.start);
        assert!(overset_frames(&library,&session.document).is_empty());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.frame_chains.is_empty());assert_ne!(dtool_core::frame::frame_story(&session.document,b),Some(story));
        assert_eq!(overset_frames(&library,&session.document),vec![a]);
        session.run(&Command::new("edit.redo")).unwrap();
        let mut unlink=Command::new("text.unlink");unlink.params["id"]=b.0.into();session.run(&unlink).unwrap();
        assert!(session.document.frame_chains.is_empty());let core=session.document.stories.iter().find(|s|s.id==story).unwrap();assert!(core.text.ends_with("omega"));
        assert_eq!(overset_frames(&library,&session.document),vec![a]);
    }
}
