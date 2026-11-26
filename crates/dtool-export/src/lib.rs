pub mod pdf;
mod pdf_links;
pub mod pdf_paint;
pub mod svg;
pub mod gif;
pub mod raster;
mod blend_range_mask;
mod effect_image;
pub mod jpeg;
pub mod png;
pub mod webp;
pub mod cad;
pub mod wmf;
pub mod cmyk;
pub mod epub;
pub mod macros;
pub mod eps_paint;
pub mod eps;
pub mod webp_lossy;
pub use eps::{export_eps, export_eps_value, eps_document};

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::document::Document;
use dtool_core::paint::{Colour, Paint, PaintOrder, StrokeStyle};
use dtool_core::shape::ShapeKind;
use dtool_core::{CommandError, Story};

pub use svg::{SvgOptions, SvgPreset};
pub use pdf_paint::{
    glyph_paint_to_fill, glyph_paint_to_krilla, glyph_paint_to_stroke, paint_to_fill,
    paint_to_krilla, paint_to_stroke, push_transparency_soft_mask, transparency_soft_mask,
    PdfPaintResources, SliceResources,
};
pub use jpeg::{export_jpeg, export_jpeg_to, export_jpeg_value, JpegOptions};
pub use png::{export_png, export_png_to, export_png_value, PngOptions};
pub use webp::{export_webp, export_webp_to, export_webp_value, WebpOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Webp,
    Tiff,
    Gif,
    Pdf,
    Svg,
    Eps,
    Wmf,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExportArea {
    WholeDocument,
    #[serde(alias = "current_page")]
    CurrentPage,
    #[serde(alias = "all_pages")]
    AllPages,
    #[serde(alias = "current_spread")]
    CurrentSpread,
    #[serde(alias = "all_spreads")]
    AllSpreads,
    #[serde(alias = "page")]
    Page(u32),
    #[serde(alias = "spread")]
    Spread(u32),
    PageRun { first: u32, last: u32 },
    SelectionWithBackground,
    SelectionWithoutBackground,
    Artboard(dtool_core::ObjectId),
    Rect([f32; 4]),
}

impl ExportArea {
    pub fn is_multi(&self) -> bool {
        matches!(self, ExportArea::AllPages | ExportArea::AllSpreads)
    }

    pub fn is_page_based(&self) -> bool {
        matches!(self, ExportArea::WholeDocument | ExportArea::CurrentPage | ExportArea::AllPages | ExportArea::CurrentSpread | ExportArea::AllSpreads | ExportArea::Page(_) | ExportArea::Spread(_) | ExportArea::PageRun { .. })
    }

    pub(crate) fn limits_objects_to_page(&self) -> bool {
        self.is_page_based() && !matches!(self, Self::WholeDocument)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportContext {
    pub selection: Vec<dtool_core::ObjectId>,
    pub current_page: usize,
}

impl ExportContext {
    pub fn from_session(session: &dtool_core::Session) -> Self {
        let selection = session.selection.ids.clone();
        let current_page = selection.first().map(|id| dtool_core::data_merge::layout::object_page_index(&session.document, *id)).unwrap_or(0);
        Self { selection, current_page }
    }

    pub fn for_view(session: &dtool_core::Session, size: [f32; 2]) -> Self {
        let current_page = {
            let point = dtool_core::view::view_to_doc(&session.view, [size[0] * 0.5, size[1] * 0.5]);
            dtool_core::scene::page_rects(&session.document).iter().enumerate().min_by(|(_, a), (_, b)| {
                let distance = |r: &[f32; 4]| (point[0] - point[0].clamp(r[0], r[0] + r[2])).powi(2) + (point[1] - point[1].clamp(r[1], r[1] + r[3])).powi(2);
                distance(a).total_cmp(&distance(b))
            }).map(|(i, _)| i).unwrap_or(0)
        };
        Self { selection: session.selection.ids.clone(), current_page }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedArea {
    pub suffix: Option<String>,
    pub area: ExportArea,
    pub only: Option<Vec<dtool_core::ObjectId>>,
}

fn valid_region(region: (f32, f32, f32, f32)) -> Option<(f32, f32, f32, f32)> {
    let (x, y, w, h) = region;
    (x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some(region)
}

pub fn page_region(doc: &Document, index: usize) -> Option<(f32, f32, f32, f32)> {
    if doc.pages.is_empty() && index == 0 {
        return valid_region((0.0, 0.0, DocumentExportExt::width(doc), DocumentExportExt::height(doc)));
    }
    dtool_core::scene::page_rects(doc).get(index).and_then(|[x, y, w, h]| valid_region((*x, *y, *w, *h)))
}

pub fn spreads(doc: &Document) -> Vec<Vec<usize>> {
    dtool_core::data_merge::layout::spread_ranges(doc).into_iter().map(Iterator::collect).collect()
}

pub fn spread_of_page(doc: &Document, page: usize) -> usize {
    spreads(doc).iter().position(|pages| pages.contains(&page)).unwrap_or(0)
}

pub fn spread_region(doc: &Document, index: usize) -> Option<(f32, f32, f32, f32)> {
    let pages = spreads(doc).into_iter().nth(index)?;
    page_batch_region(&output_page_rects(doc), &pages)
}

pub fn output_page_rects(doc: &Document) -> Vec<[f32; 4]> {
    if doc.pages.is_empty() {
        return vec![[0.0, 0.0, DocumentExportExt::width(doc), DocumentExportExt::height(doc)]];
    }
    dtool_core::scene::page_rects(doc)
}

fn union_regions((x, y, w, h): (f32, f32, f32, f32), (rx, ry, rw, rh): (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let left = x.min(rx);
    let top = y.min(ry);
    (left, top, (x + w).max(rx + rw) - left, (y + h).max(ry + rh) - top)
}

fn whole_document_region(doc: &Document) -> Option<(f32, f32, f32, f32)> {
    if doc.layout_artboard_origins.is_none() {
        let artboards = dtool_core::view::ViewTarget::artboards(doc)
            .filter(|board| board.parent_id.is_none() && !doc.master_pages.roots.contains(&board.id))
            .filter_map(|board| {
                let [x, y, w, h] = dtool_core::geometry::object_world_bounds(board);
                valid_region((x, y, w, h))
            }).reduce(union_regions);
        if let Some(bounds) = artboards {
            let pages = output_page_rects(doc);
            let union = doc.pages.iter().zip(pages).enumerate()
                .filter(|(index, _)| !dtool_core::artboard::edit::is_backing_page(doc, *index))
                .filter_map(|(_, (_, [x, y, w, h]))| valid_region((x, y, w, h)))
                .fold(bounds, union_regions);
            return valid_region(union);
        }
    }
    page_region(doc, 0)
}

pub fn page_batch_region(rects: &[[f32; 4]], pages: &[usize]) -> Option<(f32, f32, f32, f32)> {
    let mut rect: Option<(f32, f32, f32, f32)> = None;
    for &page in pages {
        let &[x, y, w, h] = rects.get(page)?;
        let bounds = valid_region((x, y, w, h))?;
        rect = Some(rect.map_or(bounds, |previous| union_regions(previous, bounds)));
    }
    rect.and_then(valid_region)
}

/// The printable portions of facing pages when their bleed is enabled.
/// Preserve positioned spreads whose page interiors overlap.
pub fn facing_bleed_regions(
    doc: &Document, pages: &[usize], [bleed_left, bleed_top, bleed_right, bleed_bottom]: [f32; 4],
) -> Option<Vec<(usize, (f32, f32, f32, f32))>> {
    if pages.len() < 2 || [bleed_left, bleed_top, bleed_right, bleed_bottom] == [0.0; 4] {
        return None;
    }
    let mut regions = pages.iter().map(|&page| Some((page, page_region(doc, page)?))).collect::<Option<Vec<_>>>()?;
    regions.sort_by(|a, b| a.1.0.total_cmp(&b.1.0));
    let mut edges = vec![f32::NEG_INFINITY];
    for neighbors in regions.windows(2) {
        let (_, (x, _, width, _)) = neighbors[0];
        let left = x + width;
        let right = neighbors[1].1.0;
        let gap = right - left;
        // Page origins may round independently at each spine.
        let tolerance = f32::EPSILON * left.abs().max(right.abs()).max(1.0) * 4.0;
        if gap < -tolerance { return None; }
        edges.push(left + gap / 2.0);
    }
    edges.push(f32::INFINITY);
    regions.into_iter().zip(edges.windows(2)).map(|((page, (x, y, w, h)), edge)| {
        let left = (x - bleed_left).max(edge[0]);
        let right = (x + w + bleed_right).min(edge[1]);
        valid_region((left, y - bleed_top, right - left, h + bleed_top + bleed_bottom))
            .map(|bounds| (page, bounds))
    }).collect()
}

pub fn page_numbers(doc: &Document, pages: Option<&[u32]>) -> Vec<usize> {
    let count = doc.pages.len().max(1);
    match pages {
        Some(list) if !list.is_empty() => list.iter().filter(|n| **n >= 1 && (**n as usize) <= count).map(|n| *n as usize - 1).collect(),
        _ => (0..count).collect(),
    }
}

fn page_included_in_output(doc: &Document, page: usize) -> bool {
    doc.sections().iter().rev()
        .find(|section| section.start_page > 0 && section.start_page <= page + 1)
        .is_none_or(|section| section.include_on_export)
}

/// Contiguous runs retained after removing sections excluded from output.
pub fn included_page_runs<'a>(doc: &'a Document, pages: &'a [usize]) -> impl Iterator<Item = &'a [usize]> {
    pages.split(move |&page| !page_included_in_output(doc, page)).filter(|pages| !pages.is_empty())
}

pub(crate) fn output_page_batches(doc: &Document, area: &ExportArea, pages: Option<&[u32]>) -> Vec<Vec<usize>> {
    let batches: Vec<Vec<usize>> = match area {
        ExportArea::Page(index) => vec![vec![*index as usize]],
        ExportArea::CurrentPage => vec![vec![0]],
        ExportArea::AllPages => page_numbers(doc, pages).into_iter().map(|index| vec![index]).collect(),
        ExportArea::Spread(index) => spreads(doc).into_iter().nth(*index as usize).into_iter().collect(),
        ExportArea::CurrentSpread => spreads(doc).into_iter().take(1).collect(),
        ExportArea::PageRun { first, last } => page_run(doc, *first, *last).map(|pages| pages.collect()).into_iter().collect(),
        ExportArea::AllSpreads => {
            let wanted: std::collections::HashSet<_> = page_numbers(doc, pages).into_iter()
                .filter(|&page| page_included_in_output(doc, page)).collect();
            spreads(doc).into_iter().filter(|spread| spread.iter().any(|index| wanted.contains(index))).collect()
        }
        _ => Vec::new(),
    };
    if doc.sections().iter().all(|section| section.include_on_export) {
        return batches;
    }
    batches.into_iter().flat_map(|batch| {
        // Trim off excluded sides. An excluded interior panel separates the remaining runs.
        included_page_runs(doc, &batch).map(<[usize]>::to_vec).collect::<Vec<_>>()
    }).collect()
}

fn page_run(doc: &Document, first: u32, last: u32) -> Option<std::ops::RangeInclusive<usize>> {
    let (first, last) = (first as usize, last as usize);
    (first <= last && last < doc.pages.len().max(1)).then_some(first..=last)
}

pub(crate) fn pdf_regions_for_batches(doc: &Document, batches: &[Vec<usize>]) -> Option<Vec<(f32, f32, f32, f32)>> {
    if batches.is_empty() { return None; }
    let rects = output_page_rects(doc);
    batches.iter().map(|pages| page_batch_region(&rects, pages)).collect()
}

pub fn pdf_output_page_count(doc: &Document, area: &ExportArea, pages: Option<&[u32]>) -> usize {
    if !area.limits_objects_to_page() { return 1; }
    pdf_regions_for_batches(doc, &output_page_batches(doc, area, pages)).map_or(0, |regions| regions.len())
}

pub fn page_members(doc: &Document, pages: &[usize]) -> std::collections::HashSet<dtool_core::ObjectId> {
    let pages: std::collections::HashSet<_> = pages.iter().copied().collect();
    let mut members: std::collections::HashSet<_> = doc.objects.iter().filter(|object| {
        !dtool_core::commands::masters::owns_source(doc, object.id)
            && pages.contains(&dtool_core::data_merge::layout::object_page_index(doc, object.id))
    }).map(|object| object.id).collect();
    let starting: Vec<_> = members.iter().copied().collect();
    for id in starting {
        let mut parent = doc.get_object(id).and_then(|object| object.parent_id);
        while let Some(id) = parent {
            if !members.insert(id) {
                break;
            }
            parent = doc.get_object(id).and_then(|object| object.parent_id);
        }
    }
    members
}

pub fn retain_page_scene(scene: &mut dtool_core::scene::Scene, pages: &[usize], members: &std::collections::HashSet<dtool_core::ObjectId>) {
    scene.prims.retain(|prim| match prim {
        dtool_core::scene::Prim::Page { index, .. } => pages.contains(index),
        _ => dtool_core::layer_style::raster::prim_object(prim).is_some_and(|id| members.contains(&id)),
    });
    scene.quads.retain(|(id, _)| members.contains(id));
}

pub fn area_regions(doc: &Document, area: &ExportArea, pages: Option<&[u32]>) -> Option<Vec<(f32, f32, f32, f32)>> {
    match area {
        ExportArea::AllPages => {
            let regions: Option<Vec<_>> = page_numbers(doc, pages).into_iter().map(|i| page_region(doc, i)).collect();
            regions.filter(|r| !r.is_empty())
        }
        ExportArea::AllSpreads => {
            let wanted = page_numbers(doc, pages);
            let regions: Option<Vec<_>> = spreads(doc).iter().enumerate().filter(|(_, p)| p.iter().any(|i| wanted.contains(i))).map(|(i, _)| spread_region(doc, i)).collect();
            regions.filter(|r| !r.is_empty())
        }
        other => doc.area_bounds(other).map(|region| vec![region]),
    }
}

pub fn selection_region(doc: &Document, ids: &[dtool_core::ObjectId]) -> Option<(f32, f32, f32, f32)> {
    dtool_core::command::selection_bounds(doc, ids).and_then(|[x, y, w, h]| valid_region((x, y, w, h)))
}

fn selection_output_region(doc: &Document, ids: &[dtool_core::ObjectId]) -> Option<(f32, f32, f32, f32)> {
    use kurbo::{Rect, Shape};

    let [x, y, width, height] = dtool_core::command::selection_bounds(doc, ids)?;
    let mut bounds = Rect::new(x.into(), y.into(), (x + width).into(), (y + height).into());
    let selected = selected_subtrees(doc, ids);
    for object in doc.objects.iter().filter(|object| {
        selected.contains(&object.id) && export_visible(doc, object) && !dtool_core::clip_mask::is_mask(doc, object)
    }) {
        let fallback = || {
            let [x0, y0, x1, y1] = vector_object_bounds(doc, object).map(f64::from);
            Rect::new(x0, y0, x1, y1)
        };
        let geometry = dtool_core::commands::appearance::vector_paths(doc, object, &object.fill, false);
        let mut ink = geometry.and_then(|paths| {
            paths.into_iter().filter(|item| !item.blend.erases() && !item.path.elements().is_empty())
                .map(|item| item.path.bounding_box()).reduce(|bounds, next| bounds.union(next))
        }).map_or_else(fallback, |ink| {
            let margin = f64::from(dtool_core::layer_style::raster::effect_margin(doc, object.id, &[]));
            ink.inflate(margin, margin)
        });
        let artboard = dtool_core::artboard::owner(doc, object.id).map(dtool_core::geometry::object_world_bounds);
        for clip in [vector_crop_bounds(object), artboard].into_iter().flatten() {
            let [x, y, w, h] = clip.map(f64::from);
            ink = ink.intersect(Rect::new(x, y, x + w, y + h));
        }
        if ink.is_finite() && ink.width() > 0.0 && ink.height() > 0.0 {
            bounds = bounds.union(ink);
        }
    }
    let mut groups = doc.objects.iter().filter(|object| selected.contains(&object.id)
        && crate::effect_image::has_group_effect(object) && dtool_core::scene::is_drawn(doc, object.id)).peekable();
    if groups.peek().is_some() {
        use dtool_core::layer_style::raster::{effect_bounds, prim_object};
        dtool_render::scene::install();
        let scene = dtool_core::scene::build_scene(doc, &dtool_core::CanvasFlags::default());
        for group in groups {
            let members = doc.subtree_ids(group.id);
            let prims = scene.prims.iter().filter(|prim| prim_object(prim).is_some_and(|id| members.contains(&id)));
            if let Some([x, y, w, h]) = effect_bounds(doc, &[], group.id, prims).map(|rect| rect.map(f64::from)) {
                let mut ink = Rect::new(x, y, x + w, y + h);
                if let Some(board) = dtool_core::artboard::owner(doc, group.id) {
                    let [x, y, w, h] = dtool_core::geometry::object_world_bounds(board).map(f64::from);
                    ink = ink.intersect(Rect::new(x, y, x + w, y + h));
                }
                if ink.is_finite() && ink.width() > 0.0 && ink.height() > 0.0 {
                    bounds = bounds.union(ink);
                }
            }
        }
    }
    valid_region((bounds.x0 as f32, bounds.y0 as f32, bounds.width() as f32, bounds.height() as f32))
}

pub fn resolve_areas(doc: &Document, area: &ExportArea, context: &ExportContext, pages: Option<&[u32]>) -> Result<Vec<ResolvedArea>, CommandError> {
    let bad = || export_error("INVALID_PARAMS", "error.export.bad_area");
    if pages.is_some_and(|list| list.iter().any(|n| *n == 0 || *n as usize > doc.pages.len().max(1))) {
        return Err(bad());
    }
    let single = |area: ExportArea| Ok(vec![ResolvedArea { suffix: None, area, only: None }]);
    let numbered = |areas: Vec<ExportArea>| {
        let many = areas.len() > 1;
        areas.into_iter().enumerate().map(|(i, area)| ResolvedArea { suffix: many.then(|| format!("-{:02}", i + 1)), area, only: None }).collect::<Vec<_>>()
    };
    match area {
        ExportArea::CurrentPage => {
            let page = context.current_page.min(doc.pages.len().max(1) - 1);
            single(ExportArea::Page(page as u32))
        }
        ExportArea::CurrentSpread => single(ExportArea::Spread(spread_of_page(doc, context.current_page) as u32)),
        ExportArea::AllPages => {
            let list = page_numbers(doc, pages);
            if list.is_empty() {
                return Err(bad());
            }
            Ok(numbered(list.into_iter().map(|i| ExportArea::Page(i as u32)).collect()))
        }
        ExportArea::AllSpreads => {
            let wanted = page_numbers(doc, pages);
            let list: Vec<ExportArea> = spreads(doc).iter().enumerate().filter(|(_, p)| p.iter().any(|i| wanted.contains(i))).map(|(i, _)| ExportArea::Spread(i as u32)).collect();
            if list.is_empty() {
                return Err(bad());
            }
            Ok(numbered(list))
        }
        ExportArea::SelectionWithBackground | ExportArea::SelectionWithoutBackground if !context.selection.is_empty() => {
            let (x, y, w, h) = selection_output_region(doc, &context.selection).ok_or_else(bad)?;
            let only = matches!(area, ExportArea::SelectionWithoutBackground).then(|| context.selection.clone());
            Ok(vec![ResolvedArea { suffix: None, area: ExportArea::Rect([x, y, w, h]), only }])
        }
        other => single(other.clone()),
    }
}

pub(crate) fn artboard_members(doc: &Document, area: &ExportArea) -> Option<std::collections::HashSet<dtool_core::ObjectId>> {
    match area {
        ExportArea::Artboard(id) => Some(doc.subtree_ids(*id)),
        _ => None,
    }
}

pub(crate) fn artboard_ring(board: &Object) -> Vec<[f64; 2]> {
    dtool_core::geometry::object_world_quad(board).into_iter().map(|point| point.map(f64::from)).collect()
}

fn selected_subtrees(doc: &Document, ids: &[dtool_core::ObjectId]) -> std::collections::HashSet<dtool_core::ObjectId> {
    let mut keep: std::collections::HashSet<dtool_core::ObjectId> = ids.iter().copied().collect();
    let mut grew = true;
    while grew {
        grew = false;
        for object in &doc.objects {
            if !keep.contains(&object.id) && object.parent_id.is_some_and(|parent| keep.contains(&parent)) {
                keep.insert(object.id);
                grew = true;
            }
        }
    }
    keep
}

pub fn only_objects(doc: &Document, ids: &[dtool_core::ObjectId]) -> Document {
    let mut keep = selected_subtrees(doc, ids);
    let mut ancestors = Vec::new();
    for id in keep.iter() {
        let mut current = doc.get_object(*id).and_then(|object| object.parent_id);
        let mut steps = 0;
        while let Some(parent) = current {
            if steps > doc.objects.len() {
                break;
            }
            ancestors.push(parent);
            current = doc.get_object(parent).and_then(|object| object.parent_id);
            steps += 1;
        }
    }
    keep.extend(ancestors);
    let mut out = doc.clone();
    out.objects.retain(|object| keep.contains(&object.id));
    out.pixel_selection = None;
    out
}

pub fn suffixed_path(path: &std::path::Path, suffix: &str) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match path.extension() {
        Some(ext) => format!("{stem}{suffix}.{}", ext.to_string_lossy()),
        None => format!("{stem}{suffix}"),
    };
    path.with_file_name(name)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    pub format: ExportFormat,
    pub area: ExportArea,
    pub pages: Option<Vec<u32>>,
    pub path: PathBuf,
    pub preset: Option<String>,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportReceipt {
    pub path: PathBuf,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExportControl {
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub progress: std::sync::Arc<std::sync::Mutex<f32>>,
}

impl ExportControl {
    pub fn new() -> Self {
        Self {
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            progress: std::sync::Arc::new(std::sync::Mutex::new(0.0)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_progress(&self, value: f32) {
        if let Ok(mut p) = self.progress.lock() {
            *p = value;
        }
    }
}

pub fn crate_ready() -> bool {
    true
}

pub mod batch;
pub use batch::batch_store;

pub const DEFAULT_LAYER_NAME: &str = "0";
pub const ELLIPSE_SEGMENTS: usize = 64;

pub fn export_error(code: &str, message_key: &str) -> CommandError {
    CommandError {
        code: code.to_string(),
        message_key: message_key.to_string(),
    }
}

pub fn export_io_error<E: std::fmt::Display>(_error: E) -> CommandError {
    CommandError::new("IO_ERROR")
}

pub fn export_cancelled() -> CommandError {
    CommandError::new("CANCELLED")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportLayer {
    pub id: Option<dtool_core::ObjectId>,
    pub name: String,
}

pub fn paint_colour(paint: &Paint) -> Option<Colour> {
    match paint {
        Paint::None => None,
        Paint::Solid { colour } => Some(*colour),
        Paint::Gradient { stops, .. } => average_colour(stops.iter().map(|stop| stop.colour)),
        Paint::Hatch(hatch) => Some(hatch.colour),
        Paint::Mesh(mesh) => average_colour(mesh.colours.iter().copied()),
        Paint::Bitmap(_) | Paint::Pattern(_) | Paint::Diffusion(_) => None,
    }
}

pub fn stroke_colour(stroke: &StrokeStyle) -> Option<Colour> {
    if !stroke.width.is_finite() || stroke.width <= 0.0 {
        return None;
    }
    paint_colour(&stroke.paint)
}

fn average_colour(colours: impl Iterator<Item = Colour>) -> Option<Colour> {
    let mut sum = [0u32; 4];
    let mut count = 0u32;
    for colour in colours {
        sum[0] += u32::from(colour.r);
        sum[1] += u32::from(colour.g);
        sum[2] += u32::from(colour.b);
        sum[3] += u32::from(colour.a);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some(Colour::new(
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
        (sum[3] / count) as u8,
    ))
}

fn layer_label(object: &Object) -> String {
    let cleaned: String = object
        .name
        .chars()
        .map(|c| if matches!(c, '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | '=' | '`') || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        format!("LAYER_{}", object.id.0)
    } else {
        trimmed.to_string()
    }
}

fn format_number(value: f32) -> String {
    let text = format!("{:.3}", value);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" || text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

fn regular_polygon(x: f32, y: f32, w: f32, h: f32, sides: usize, inner_ratio: Option<f32>) -> Vec<(f32, f32)> {
    let sides = sides.max(3);
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let rx = w * 0.5;
    let ry = h * 0.5;
    let steps = if inner_ratio.is_some() { sides * 2 } else { sides };
    (0..steps)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / steps as f32;
            let scale = match inner_ratio {
                Some(ratio) if i % 2 == 1 => ratio,
                _ => 1.0,
            };
            (cx + rx * scale * angle.cos(), cy + ry * scale * angle.sin())
        })
        .collect()
}

pub fn export_visible(doc: &Document, object: &Object) -> bool {
    object.is_visible() && !dtool_core::commands::masters::owns_source(doc, object.id)
        && dtool_core::scene::is_drawn(doc, object.id)
}

pub(crate) fn vector_world_path(object: &Object) -> Option<kurbo::BezPath> {
    let path = dtool_core::outline::object_outline(object)?;
    let matrix = dtool_core::geometry::object_matrix(object).map(f64::from);
    let [x, y] = object.pos.map(f64::from);
    Some(kurbo::Affine::new(matrix) * kurbo::Affine::translate((x, y)) * path)
}

// A geometric error of 0.02 points is about 7 micrometres on the output.
pub(crate) fn vector_export_tolerance(points_per_unit: f64) -> f64 {
    (0.02 / points_per_unit).min(dtool_core::boolean::DEFAULT_TOLERANCE)
}

pub(crate) fn text_container(object: &Object) -> Option<Object> {
    if !matches!(object.kind, ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. })
        || matches!(object.fill, Paint::None)
            && (matches!(object.stroke.paint, Paint::None) || object.stroke.width <= 0.0)
    {
        return None;
    }
    // The canvas uses the text object's rectangular contour for its
    // container paint. Glyph paint comes from the text layout separately.
    Some(Object { kind: ObjectKind::Shape, shape: None, ..object.clone() })
}

pub(crate) fn vector_curve_paths(path: kurbo::BezPath) -> (kurbo::BezPath, kurbo::BezPath) {
    let mut fill = kurbo::BezPath::new();
    let mut start = 0;
    for (index, element) in path.elements().iter().enumerate() {
        match element {
            kurbo::PathEl::MoveTo(_) => start = index,
            kurbo::PathEl::ClosePath => fill.extend(path.elements()[start..=index].iter().copied()),
            _ => {}
        }
    }
    (path, fill)
}

pub(crate) fn arrowheads_for(doc: Option<&Document>, object: &Object) -> Vec<dtool_core::commands::stroke::ArrowHead> {
    let stroke = &object.stroke;
    if (stroke.start_arrow.is_none() && stroke.end_arrow.is_none())
        || !stroke.width.is_finite() || stroke.width <= 0.0 || matches!(stroke.paint, Paint::None)
    {
        return Vec::new();
    }
    let matrix = dtool_core::geometry::object_matrix(object);
    doc.and_then(|doc| dtool_core::scene::warped_contours(doc, object, &matrix))
        .unwrap_or_else(|| dtool_core::scene::doc_contours(object, &matrix)).into_iter()
        .flat_map(|(points, closed)| dtool_core::commands::stroke::arrowheads(stroke, &points, closed, stroke.width))
        .collect()
}

pub(crate) use dtool_core::scene::vector_crop_bounds;

pub(crate) fn vector_object_bounds(doc: &Document, object: &Object) -> [f32; 4] {
    let [x, y, w, h] = dtool_core::geometry::object_world_bounds(object);
    let mut bounds = [x, y, x + w, y + h];
    let mut include = |[left, top, right, bottom]: [f32; 4]| {
        bounds[0] = bounds[0].min(left);
        bounds[1] = bounds[1].min(top);
        bounds[2] = bounds[2].max(right);
        bounds[3] = bounds[3].max(bottom);
    };

    let matrix = dtool_core::geometry::object_matrix(object);
    if let Some(contours) = dtool_core::scene::warped_contours(doc, object, &matrix) {
        for [x, y] in contours.into_iter().flat_map(|(points, _)| points) {
            include([x, y, x, y]);
        }
    }
    for head in arrowheads_for(Some(doc), object) {
        for [x, y] in head.points {
            include([x, y, x, y]);
        }
    }
    if dtool_core::commands::appearance::has_layers(object)
        && let Some(appearance) = dtool_core::commands::appearance::vector_paths(doc, object, &object.fill, false)
    {
        use kurbo::Shape;
        for layer in appearance.iter().filter(|layer| !layer.path.elements().is_empty()) {
            let ink = layer.path.bounding_box();
            include([ink.x0 as f32, ink.y0 as f32, ink.x1 as f32, ink.y1 as f32]);
        }
    }

    if let Some(dimension) = dtool_core::outline::dimension_geometry(object) {
        for segment in std::iter::once(&dimension.line).chain(&dimension.extensions).chain(&dimension.ticks) {
            for &[x, y] in segment {
                include([x as f32, y as f32, x as f32, y as f32]);
            }
        }
        let [x, y] = dimension.label_at.map(|value| value as f32);
        let radius = dtool_core::outline::DIMENSION_LABEL_SIZE as f32 * (dimension.label.chars().count() as f32 + 1.0);
        include([x - radius, y - radius, x + radius, y + radius]);
    } else if let Some(body) = object.text_content().or_else(|| doc.story(object).map(|story| story.text.as_str())) {
        let shaped_text = matches!(object.kind, ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_));
        if shaped_text {
            if dtool_core::scene::has_live_text_warp(doc, object.id) {
                dtool_text::canvas_text::install();
                let lines = dtool_core::text_lines::text_lines(doc, object.id);
                if let Some(ink) = dtool_core::scene::warped_text_ink(doc, object, &lines, object.opacity) {
                    for prim in ink {
                        if let dtool_core::scene::Prim::Fill { contours, .. } = prim {
                            if let Some(bounds) = dtool_core::scene::contours_bounds(&contours) { include(bounds); }
                        }
                    }
                }
            }
            let outlines = dtool_text::canvas_text::text_outlines_with(&dtool_text::FontLibrary::shared(), doc, object.id);
            if let Some(ink) = outlines.as_deref().and_then(dtool_core::scene::contours_bounds) {
                include(ink);
            }
        } else {
            // The legacy vector text serializers use the raw text with a
            // fallback font, so the layer's box alone does not bound its ink.
            let size = object.font_size().unwrap_or(16.0);
            let fonts = dtool_text::FontLibrary::shared();
            let width = body.lines().flat_map(|line| {
                ["serif", "sans-serif"].map(|family| {
                    dtool_text::shape_run(&fonts, line, family, "Regular", size).width
                })
            }).fold(0.0, f32::max);
            let rows = body.lines().count().max(1) as f32;
            let x = object.x();
            let y = object.y();
            include([x - size, y - size * 2.0, x + width * 1.25 + size, y + (rows * 1.2 + 2.0) * size]);
        }
    }
    let stroke = if matches!(object.stroke.paint, Paint::None) {
        0.0
    } else {
        use dtool_core::paint::{Join, StrokeAlign};
        let style = &object.stroke;
        let miter = match style.join {
            Join::Miter { limit } => limit.max(1.5),
            _ => 1.5,
        };
        let pressure = style.pressure.iter().map(|point| point[1]).fold(1.0, f32::max);
        let align = if style.align == StrokeAlign::Centre { 0.5 } else { 1.0 };
        let width = if matches!(object.kind, ObjectKind::Dimension(_)) { style.width.max(1.0) } else { style.width };
        width.abs() * align * pressure * miter
    };
    let padding = stroke + dtool_core::layer_style::raster::effect_margin(doc, object.id, &[]);
    [bounds[0] - padding, bounds[1] - padding, bounds[2] + padding, bounds[3] + padding]
}

pub(crate) fn vector_bounds_intersect(bounds: [f32; 4], region: (f32, f32, f32, f32)) -> bool {
    let (x, y, w, h) = region;
    !bounds.iter().all(|value| value.is_finite())
        || (bounds[0] < x + w && bounds[1] < y + h && bounds[2] > x && bounds[3] > y)
}

pub trait ObjectExportExt {
    fn x(&self) -> f32;
    fn y(&self) -> f32;
    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn is_visible(&self) -> bool;
    fn text(&self) -> Option<&str>;
    fn font_size(&self) -> Option<f32>;
    fn outline_points(&self) -> Vec<(f32, f32)>;
    fn path_data(&self) -> Option<String>;
}

impl ObjectExportExt for Object {
    fn x(&self) -> f32 {
        self.pos[0].min(self.pos[0] + self.size[0])
    }

    fn y(&self) -> f32 {
        self.pos[1].min(self.pos[1] + self.size[1])
    }

    fn width(&self) -> f32 {
        self.size[0].abs()
    }

    fn height(&self) -> f32 {
        self.size[1].abs()
    }

    fn is_visible(&self) -> bool {
        !self.is_group()
            && self.pickable
            && self.opacity.is_finite()
            && self.opacity > 0.0
            && self.pos.iter().chain(self.size.iter()).all(|v| v.is_finite())
    }

    fn text(&self) -> Option<&str> {
        self.text_content()
    }

    fn font_size(&self) -> Option<f32> {
        let height = self.height();
        if height <= 0.0 {
            return None;
        }
        match &self.kind {
            ObjectKind::Text(content) => Some(height / content.lines().count().max(1) as f32),
            ObjectKind::ArtText { .. } => Some(height),
            _ => None,
        }
    }

    fn outline_points(&self) -> Vec<(f32, f32)> {
        let (x, y, w, h) = (self.x(), self.y(), self.width(), self.height());
        let recipe = self.shape.as_ref();
        let points: Vec<(f32, f32)> = match recipe.map(|r| r.kind) {
            Some(ShapeKind::Ellipse) | Some(ShapeKind::Donut) | Some(ShapeKind::CalloutEllipse) => (0..ELLIPSE_SEGMENTS)
                .map(|i| {
                    let t = i as f32 / ELLIPSE_SEGMENTS as f32 * std::f32::consts::TAU;
                    (x + w * 0.5 + w * 0.5 * t.cos(), y + h * 0.5 + h * 0.5 * t.sin())
                })
                .collect(),
            Some(ShapeKind::Triangle) => vec![(x + w * 0.5, y), (x + w, y + h), (x, y + h)],
            Some(ShapeKind::Diamond) => vec![(x + w * 0.5, y), (x + w, y + h * 0.5), (x + w * 0.5, y + h), (x, y + h * 0.5)],
            Some(ShapeKind::Trapezoid) => {
                let indent = recipe.and_then(|r| r.trapezoid.as_ref()).map(|t| t.indent).unwrap_or(0.25).clamp(0.0, 0.5);
                vec![(x + w * indent, y), (x + w * (1.0 - indent), y), (x + w, y + h), (x, y + h)]
            }
            Some(ShapeKind::Polygon) => {
                let sides = recipe.and_then(|r| r.polygon.as_ref()).map(|p| p.number_of_sides as usize).unwrap_or(5);
                regular_polygon(x, y, w, h, sides, None)
            }
            Some(ShapeKind::Star) => {
                let star = recipe.and_then(|r| r.star.as_ref());
                let points = star.map(|s| s.number_of_points as usize).unwrap_or(5);
                let ratio = star
                    .filter(|s| s.outer_radius > 0.0)
                    .map(|s| (s.inner_radius / s.outer_radius).clamp(0.05, 1.0))
                    .unwrap_or(0.5);
                regular_polygon(x, y, w, h, points, Some(ratio))
            }
            Some(ShapeKind::DoubleStar) => {
                let star = recipe.and_then(|r| r.double_star.as_ref());
                let points = star.map(|s| s.number_of_points as usize).unwrap_or(8);
                let ratio = star.map(|s| s.inner_radius.clamp(0.05, 1.0)).unwrap_or(0.5);
                regular_polygon(x, y, w, h, points, Some(ratio))
            }
            _ => vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
        };
        if self.rotation_deg == 0.0 || !self.rotation_deg.is_finite() {
            return points;
        }
        let (sin, cos) = self.rotation_deg.to_radians().sin_cos();
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        points
            .into_iter()
            .map(|(px, py)| {
                let dx = px - cx;
                let dy = py - cy;
                (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos)
            })
            .collect()
    }

    fn path_data(&self) -> Option<String> {
        if !matches!(self.kind, ObjectKind::Shape) {
            return None;
        }
        if self.shape.is_some() && let Some(path) = vector_world_path(self) {
            return Some(path.to_svg());
        }
        let points = self.outline_points();
        if points.len() < 2 {
            return None;
        }
        let mut data = String::new();
        for (index, (px, py)) in points.iter().enumerate() {
            data.push_str(if index == 0 { "M" } else { " L" });
            data.push_str(&format_number(*px));
            data.push(' ');
            data.push_str(&format_number(*py));
        }
        data.push_str(" Z");
        Some(data)
    }
}

pub trait DocumentExportExt {
    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn layers(&self) -> Vec<ExportLayer>;
    fn layer_name(&self, object: &Object) -> String;
    fn selection_bounds(&self) -> Option<(f32, f32, f32, f32)>;
    fn artboard_bounds(&self, id: dtool_core::ObjectId) -> Option<(f32, f32, f32, f32)>;
    fn area_bounds(&self, area: &ExportArea) -> Option<(f32, f32, f32, f32)>;
    fn story(&self, object: &Object) -> Option<&Story>;
    fn object_text(&self, object: &Object) -> Option<String>;
    fn page_objects(&self, page_index: usize) -> Vec<&Object>;
    fn render_rgba_region(&self, region: (f32, f32, f32, f32), width: usize, height: usize) -> Vec<u8>;
    fn render_rgb(&self, width: usize, height: usize) -> Vec<u8>;
}

fn content_extent(doc: &Document) -> (f32, f32) {
    let mut extent = (0.0f32, 0.0f32);
    for object in doc.objects.iter().filter(|o| export_visible(doc, o)) {
        extent.0 = extent.0.max(object.x() + object.width());
        extent.1 = extent.1.max(object.y() + object.height());
    }
    extent
}

fn point_in_polygon(point: (f32, f32), polygon: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for i in 0..polygon.len() {
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        if (yi > point.1) != (yj > point.1) && point.0 < (xj - xi) * (point.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn distance_to_outline(point: (f32, f32), polygon: &[(f32, f32)]) -> f32 {
    let mut best = f32::INFINITY;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let edge = (b.0 - a.0, b.1 - a.1);
        let length = edge.0 * edge.0 + edge.1 * edge.1;
        let t = if length > 0.0 {
            (((point.0 - a.0) * edge.0 + (point.1 - a.1) * edge.1) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let nearest = (a.0 + edge.0 * t, a.1 + edge.1 * t);
        let distance = ((point.0 - nearest.0).powi(2) + (point.1 - nearest.1).powi(2)).sqrt();
        best = best.min(distance);
    }
    best
}

pub fn flatten_rgba(rgba: &[u8], background: [u8; 3]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        let alpha = f32::from(pixel[3]) / 255.0;
        for channel in 0..3 {
            let value = f32::from(pixel[channel]) * alpha + f32::from(background[channel]) * (1.0 - alpha);
            rgb.push(value.round().clamp(0.0, 255.0) as u8);
        }
    }
    rgb
}

/// Render one artboard's layer subtree within the supplied document bounds.
pub fn render_artboard_rgba_region(
    doc: &Document,
    artboard: dtool_core::ObjectId,
    region: (f32, f32, f32, f32),
    size: [usize; 2],
) -> Vec<u8> {
    render_rgba_region(doc, region, size, Some(artboard), &[])
}

pub fn render_page_rgba_region(
    doc: &Document, page: usize, bounds: (f32, f32, f32, f32), size: [usize; 2],
) -> Vec<u8> {
    render_pages_rgba_region(doc, &[page], bounds, size)
}

pub fn render_pages_rgba_region(
    doc: &Document, pages: &[usize], bounds: (f32, f32, f32, f32), size: [usize; 2],
) -> Vec<u8> {
    render_rgba_region(doc, bounds, size, None, pages)
}

/// Rasterize facing pages separately and join their bleed at the spine.
pub fn render_facing_bleed_rgba_region(
    doc: &Document, pages: &[usize], (x, y, w, h): (f32, f32, f32, f32),
    size: [usize; 2], bleed: [f32; 4],
) -> Option<Vec<u8>> {
    let regions = facing_bleed_regions(doc, pages, bleed)?;
    let pixel = |value: f32, origin: f32, span: f32, count: usize| {
        ((f64::from(value) - f64::from(origin)) / f64::from(span) * count as f64)
            .round().clamp(0.0, count as f64) as usize
    };
    let [width, height] = size;
    let mut pixels = vec![0; width * height * 4];
    for (page, (left, top, region_width, region_height)) in regions {
        let start_x = left.max(x);
        let start_y = top.max(y);
        let right = (left + region_width).min(x + w);
        let bottom = (top + region_height).min(y + h);
        let x0 = pixel(start_x, x, w, width);
        let x1 = pixel(right, x, w, width);
        let y0 = pixel(start_y, y, h, height);
        let y1 = pixel(bottom, y, h, height);
        if x1 <= x0 || y1 <= y0 { continue; }
        let content = render_page_rgba_region(
            doc, page, (start_x, start_y, right - start_x, bottom - start_y), [x1 - x0, y1 - y0],
        );
        let row_bytes = (x1 - x0) * 4;
        if content.len() != row_bytes * (y1 - y0) { return Some(Vec::new()); }
        for (row, source) in content.chunks_exact(row_bytes).enumerate() {
            let start = ((row + y0) * width + x0) * 4;
            pixels[start..start + row_bytes].copy_from_slice(source);
        }
    }
    Some(pixels)
}

/// Printable note glyphs whose frames survived the output scope.
pub fn output_note_fills(doc: &Document, scene: &dtool_core::scene::Scene) -> Vec<dtool_core::scene::Prim> {
    use dtool_core::scene::{Prim, effective_opacity};

    if doc.notes.notes.is_empty() {
        return Vec::new();
    }
    let frames: std::collections::HashSet<_> = scene.prims.iter().filter_map(|prim| match prim {
        Prim::Mark { object, role, .. } if matches!(role.as_str(), "Footnote" | "Endnote" | "Sidenote" | "NoteRef") => Some(*object),
        _ => None,
    }).collect();
    if frames.is_empty() {
        return Vec::new();
    }
    let library = dtool_text::FontLibrary::shared();
    let paint = Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
    dtool_text::notes_layout::output_note_lines(doc).into_iter()
        .filter(|line| frames.contains(&line.frame))
        .filter_map(|line| {
            let opacity = effective_opacity(doc, line.frame);
            if opacity <= 0.0 || line.text.is_empty() {
                return None;
            }
            let contours = line.contours(&library);
            (!contours.is_empty()).then(|| Prim::Fill {
                object: line.frame, contours, origin: line.baseline, paint: paint.clone(), opacity,
            })
        }).collect()
}

fn paint_note_fills(
    pixels: &mut [u8], size: [usize; 2], region: [f32; 4],
    fills: Vec<dtool_core::scene::Prim>,
) {
    use resvg::tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

    let [width, height] = size;
    let [x, y, w, h] = region;
    let mut paint = Paint::default();
    paint.anti_alias = true;
    for fill in fills {
        let dtool_core::scene::Prim::Fill { contours, opacity, .. } = fill else { continue };
        let map = |[px, py]: [f32; 2]| [(px - x) * width as f32 / w, (py - y) * height as f32 / h];
        let mut builder = PathBuilder::new();
        for contour in contours {
            let Some((&first, rest)) = contour.split_first() else { continue };
            let [px, py] = map(first);
            builder.move_to(px, py);
            for &point in rest {
                let [px, py] = map(point);
                builder.line_to(px, py);
            }
            builder.close();
        }
        let Some(path) = builder.finish() else { continue };
        let bounds = path.bounds();
        let left = bounds.left().floor().clamp(0.0, width as f32) as usize;
        let top = bounds.top().floor().clamp(0.0, height as f32) as usize;
        let right = bounds.right().ceil().clamp(0.0, width as f32) as usize;
        let bottom = bounds.bottom().ceil().clamp(0.0, height as f32) as usize;
        let tile_width = right - left;
        let Some(mut tile) = Pixmap::new(tile_width as u32, (bottom - top) as u32) else { continue };
        paint.set_color_rgba8(0, 0, 0, (255.0 * opacity) as u8);
        tile.fill_path(&path, &paint, FillRule::EvenOdd, Transform::from_translate(-(left as f32), -(top as f32)), None);
        for (index, pixel) in tile.data().chunks_exact(4).enumerate() {
            if pixel[3] != 0 {
                let offset = ((top + index / tile_width) * width + left + index % tile_width) * 4;
                dtool_core::blend::blend_pixel_normal(pixels, offset, [0, 0, 0, pixel[3]], 1.0);
            }
        }
    }
}

fn render_rgba_region(
    doc: &Document, (x, y, w, h): (f32, f32, f32, f32), [width, height]: [usize; 2],
    artboard: Option<dtool_core::ObjectId>,
    pages: &[usize],
) -> Vec<u8> {
    if width == 0 || height == 0 || !(w > 0.0) || !(h > 0.0) || !x.is_finite() || !y.is_finite() {
        return vec![0; width * height * 4];
    }
    dtool_render::scene::install();
    dtool_text::canvas_text::install();
    use dtool_core::layer_style::raster;
    let flags = dtool_core::CanvasFlags::default();
    let effect_scale = dtool_core::effect_raster::effect_scale(doc, width as f32 / w);
    // Flatten curves finely enough for the output pixels, including documents
    // whose coordinates are expressed in inches or other physical units.
    let pixels_per_unit = (width as f64 / f64::from(w)).max(height as f64 / f64::from(h));
    let tolerance = (0.25 / pixels_per_unit).clamp(1e-6, dtool_core::outline::OUTLINE_TOLERANCE);
    let mut scene = dtool_render::scene::build_scene_with_tolerance(doc, &flags, tolerance);
    dtool_core::clip_mask::apply(doc, &flags, &mut scene);
    if let Some(artboard) = artboard {
        let ids = doc.subtree_ids(artboard);
        // Filter after building the scene to retain transformed geometry and
        // the mask and clipping operations already prepared for these layers.
        scene.prims.retain(|prim| raster::prim_object(prim).is_some_and(|id| ids.contains(&id)));
    }
    if !pages.is_empty() {
        retain_page_scene(&mut scene, pages, &page_members(doc, pages));
    }
    let mut pixels = raster::render_scene_region(doc, &flags, &[], &scene, [x, y, w, h],
        [width as u32, height as u32], effect_scale);
    paint_note_fills(&mut pixels, [width, height], [x, y, w, h], output_note_fills(doc, &scene));
    pixels
}

impl DocumentExportExt for Document {
    fn width(&self) -> f32 {
        match self.pages.first() {
            Some(page) if page.size[0].is_finite() && page.size[0] > 0.0 => page.size[0],
            _ => content_extent(self).0,
        }
    }

    fn height(&self) -> f32 {
        match self.pages.first() {
            Some(page) if page.size[1].is_finite() && page.size[1] > 0.0 => page.size[1],
            _ => content_extent(self).1,
        }
    }

    fn layers(&self) -> Vec<ExportLayer> {
        let mut layers = vec![ExportLayer { id: None, name: DEFAULT_LAYER_NAME.to_string() }];
        for object in &self.objects {
            if object.parent_id.is_none() && object.is_group() {
                let name = layer_label(object);
                if !layers.iter().any(|layer| layer.name == name) {
                    layers.push(ExportLayer { id: Some(object.id), name });
                }
            }
        }
        layers
    }

    fn layer_name(&self, object: &Object) -> String {
        let mut current = object;
        let mut steps = 0usize;
        while let Some(parent_id) = current.parent_id {
            if steps > self.objects.len() {
                break;
            }
            match self.get_object(parent_id) {
                Some(parent) => current = parent,
                None => break,
            }
            steps += 1;
        }
        if current.id != object.id && current.is_group() && current.parent_id.is_none() {
            layer_label(current)
        } else {
            DEFAULT_LAYER_NAME.to_string()
        }
    }

    fn selection_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        self.pixel_selection
            .as_ref()
            .and_then(|selection| selection.bounds())
            .map(|[x, y, w, h]| (x as f32, y as f32, w as f32, h as f32))
    }

    fn artboard_bounds(&self, id: dtool_core::ObjectId) -> Option<(f32, f32, f32, f32)> {
        self.get_object(id)
            .filter(|object| matches!(object.kind, ObjectKind::Artboard))
            .map(|object| {
                let [x, y, w, h] = dtool_core::geometry::object_world_bounds(object);
                (x, y, w, h)
            })
    }

    fn area_bounds(&self, area: &ExportArea) -> Option<(f32, f32, f32, f32)> {
        let bounds = match area {
            ExportArea::WholeDocument => whole_document_region(self),
            ExportArea::CurrentPage => page_region(self, 0),
            ExportArea::Page(index) => page_region(self, *index as usize),
            ExportArea::CurrentSpread => spread_region(self, 0),
            ExportArea::Spread(index) => spread_region(self, *index as usize),
            ExportArea::PageRun { first, last } => {
                let pages: Vec<_> = page_run(self, *first, *last)?.collect();
                page_batch_region(&output_page_rects(self), &pages)
            }
            ExportArea::AllPages => page_region(self, 0),
            ExportArea::AllSpreads => spread_region(self, 0),
            ExportArea::Rect([x, y, w, h]) => Some((*x, *y, *w, *h)),
            ExportArea::SelectionWithBackground | ExportArea::SelectionWithoutBackground => self.selection_bounds(),
            ExportArea::Artboard(id) => self.artboard_bounds(*id),
        };
        bounds.filter(|(x, y, w, h)| x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0)
    }

    fn story(&self, object: &Object) -> Option<&Story> {
        let id = object.story_id()?;
        self.stories.iter().find(|story| story.id == id)
    }

    fn object_text(&self, object: &Object) -> Option<String> {
        if let Some(content) = object.text_content() {
            return Some(content.to_string());
        }
        self.story(object).map(|story| story.text.clone())
    }

    fn page_objects(&self, page_index: usize) -> Vec<&Object> {
        if page_index >= self.pages.len() {
            return Vec::new();
        }
        let drawable = self.objects.iter().filter(|object| !object.is_group());
        if self.pages.len() == 1 {
            return drawable.collect();
        }
        let top: f32 = self.pages[..page_index].iter().map(|page| page.size[1]).sum();
        let bottom = top + self.pages[page_index].size[1];
        let last = page_index + 1 == self.pages.len();
        drawable
            .filter(|object| {
                let centre = object.y() + object.height() * 0.5;
                (centre >= top || page_index == 0) && (centre < bottom || last)
            })
            .collect()
    }

    fn render_rgba_region(&self, region: (f32, f32, f32, f32), width: usize, height: usize) -> Vec<u8> {
        render_rgba_region(self, region, [width, height], None, &[])
    }

    fn render_rgb(&self, width: usize, height: usize) -> Vec<u8> {
        let region = (0.0, 0.0, DocumentExportExt::width(self), DocumentExportExt::height(self));
        let rgba = self.render_rgba_region(region, width, height);
        flatten_rgba(&rgba, [255, 255, 255])
    }
}

pub fn install_text_decorations() -> bool {
    dtool_core::text_lines::register_text_decorations(dtool_text::decorations::text_decorations_shared)
}

pub fn atomic_temp_path(path: &std::path::Path) -> std::path::PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{}.{}.dtool-tmp", name, std::process::id()))
}

pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_with_policy(path, bytes, true)
}

fn write_atomic_with_policy(path: &std::path::Path, bytes: &[u8], overwrite: bool) -> std::io::Result<()> {
    use std::io::Write;
    use std::path::Path;

    let directory = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new().prefix(".dtool-export-").suffix(".tmp").tempfile_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    let written = if overwrite {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    };
    written.map(|_| ()).map_err(|error| error.error)
}

pub fn export_object(doc: &Document, kind: &ObjectKind) -> Result<serde_json::Value, String> {
    match kind {
        ObjectKind::Shape => Ok(serde_json::json!({"exported": "shape"})),
        ObjectKind::Group => Ok(serde_json::json!({"exported": "group"})),
        ObjectKind::Table(_) => Ok(serde_json::json!({"exported": "table"})),
        ObjectKind::Text(content) => Ok(serde_json::json!({
            "exported": "text",
            "text_length": content.len(),
            "char_count": content.chars().count()
        })),
        ObjectKind::ArtText { story } => {
            if let Some(s) = doc.stories.iter().find(|s| s.id == *story) {
                Ok(serde_json::json!({
                    "exported": "art_text",
                    "text_length": s.text.len(),
                    "char_count": s.text.chars().count(),
                    "char_runs": s.ranges.len()
                }))
            } else {
                Err("story_not_found".to_string())
            }
        }
        ObjectKind::FrameText { story } => {
            if let Some(s) = doc.stories.iter().find(|s| s.id == *story) {
                let frame = doc
                    .objects
                    .iter()
                    .find(|object| matches!(&object.kind, ObjectKind::FrameText { story: id } if id == story))
                    .map(|object| [object.width(), object.height()])
                    .unwrap_or([0.0, 0.0]);
                Ok(serde_json::json!({
                    "exported": "frame_text",
                    "text_length": s.text.len(),
                    "char_count": s.text.chars().count(),
                    "char_runs": s.ranges.len(),
                    "frame_width": frame[0],
                    "frame_height": frame[1]
                }))
            } else {
                Err("story_not_found".to_string())
            }
        }
        ObjectKind::PathText(data) => {
            if let Some(s) = doc.stories.iter().find(|s| s.id == data.story) {
                Ok(serde_json::json!({
                    "exported": "path_text",
                    "text_length": s.text.len(),
                    "char_count": s.text.chars().count(),
                    "char_runs": s.ranges.len(),
                    "path_id": data.path.0,
                    "flows_count": data.flows.len()
                }))
            } else {
                Err("story_not_found".to_string())
            }
        }
        ObjectKind::Curve(curve) => Ok(serde_json::json!({
            "exported": "curve",
            "points": curve.contour.points.len(),
            "holes": curve.holes.len(),
            "closed": curve.contour.closed,
            "source_object": curve.source_object
        })),
        ObjectKind::Dimension(d) => Ok(serde_json::json!({
            "exported": "dimension",
            "length": d.length,
            "units": d.units
        })),
        ObjectKind::Artboard => Ok(serde_json::json!({"exported": "artboard"})),
        ObjectKind::Image => Ok(serde_json::json!({"exported": "image"})),
        ObjectKind::PictureFrame => Ok(serde_json::json!({"exported": "picture_frame"})),
        ObjectKind::Compound(_) => Ok(serde_json::json!({"exported": "compound"})),
    }
}
