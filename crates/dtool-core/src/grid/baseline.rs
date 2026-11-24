use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Document,
    #[default]
    Spread,
    Page,
    Margin,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub offset: f32,
    pub origin: Origin,
    pub threshold_percent: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, offset: 0.0, origin: Origin::Spread, threshold_percent: 70.0 }
    }
}

impl Settings {
    pub fn valid(self) -> bool {
        self.offset.is_finite() && (0.0..=100_000.0).contains(&self.threshold_percent)
    }
}

pub fn enabled(grid: &super::Grid) -> bool {
    grid.baseline.map_or(grid.show_baseline, |settings| settings.enabled && settings.valid())
}

fn start(doc: &crate::Document, rects: &[[f32; 4]], page: usize, spread_top: Option<f32>) -> Option<f32> {
    let Some(settings) = doc.grid.baseline else { return Some(doc.grid.origin[1]); };
    let page_top = rects.get(page)?[1];
    let top = match settings.origin {
        Origin::Document => 0.0,
        Origin::Page => page_top,
        Origin::Margin if doc.setup.margins.enabled => page_top + doc.setup.margins.top,
        Origin::Margin => page_top,
        Origin::Spread => spread_top.unwrap_or(page_top),
    };
    Some(top + settings.offset)
}

pub fn start_for_page(doc: &crate::Document, page: usize) -> Option<f32> {
    if !enabled(&doc.grid) { return None; }
    if doc.grid.baseline.is_none() { return Some(doc.grid.origin[1]); }
    let rects = crate::scene::page_rects(doc);
    let spread = crate::data_merge::layout::spread_ranges(doc).into_iter().find(|spread| spread.contains(&page));
    let top = spread.and_then(|spread| spread_top(&rects, spread));
    start(doc, &rects, page, top)
}

fn spread_top(rects: &[[f32; 4]], spread: std::ops::Range<usize>) -> Option<f32> {
    rects.get(spread)?.iter().map(|rect| rect[1]).reduce(f32::min)
}

pub struct PageGrid {
    pub rect: [f32; 4],
    pub first: f32,
    pub spacing: f32,
}

pub fn pages(doc: &crate::Document) -> impl Iterator<Item = PageGrid> + '_ {
    let spacing = doc.grid.baseline_spacing;
    let draw = doc.grid.show_baseline && enabled(&doc.grid) && spacing.is_finite() && spacing > 0.0;
    let rects = if draw { crate::scene::page_rects(doc) } else { Vec::new() };
    let spreads = if doc.grid.baseline.is_some_and(|settings| settings.origin == Origin::Spread) {
        crate::data_merge::layout::spread_ranges(doc)
    } else { Vec::new() };
    let mut spreads = spreads.into_iter().peekable();
    (0..rects.len()).filter_map(move |page| {
        let rect = rects[page];
        while spreads.peek().is_some_and(|range| range.end <= page) { spreads.next(); }
        let top = spreads.peek().filter(|range| range.contains(&page)).and_then(|range| spread_top(&rects, range.clone()));
        let first = start(doc, &rects, page, top)?;
        let first = first + ((rect[1] - first) / spacing).ceil().max(0.0) * spacing;
        (rect.iter().all(|v| v.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0 && first.is_finite() && first <= rect[1] + rect[3])
            .then_some(PageGrid { rect, first, spacing })
    })
}
