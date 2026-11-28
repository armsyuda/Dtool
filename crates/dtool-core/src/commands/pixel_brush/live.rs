use super::{BrushOp, BrushSettings, StrokeArgs, brush, edit_layer, install_pixels};
use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::pixel::PixelLayer;
use crate::raster_view::RasterViewMut;
use crate::{Document, ObjectId, Session};

#[derive(Debug, Clone, Default)]
pub struct LiveStroke {
    cache: Option<StrokeCache>,
}

#[derive(Debug, Clone)]
struct StrokeCache {
    instance: crate::document::DocInstance,
    generation: u64,
    object: crate::Object,
    settings: BrushSettings,
    operation: BrushOp,
    base: PixelLayer,
    stable: PixelLayer,
    stable_count: usize,
    dabs: Vec<brush::Dab>,
    created: bool,
}

impl LiveStroke {
    pub(super) fn preview(
        &mut self, session: &Session, args: &StrokeArgs, operation: BrushOp,
    ) -> Option<(Document, Option<ObjectId>)> {
        if session.document.pixel_selection.is_some() { return None; }
        let settings = super::settings(session, operation);
        if settings.symmetry > 1 || settings.mirror || !brush::independent_dabs(&settings, operation) {
            return None;
        }
        let selected = super::selected_raster(session).map(|object| object.id);
        if self.cache.as_ref().is_none_or(|cache| {
            cache.instance != session.document.instance || cache.generation != session.generation
                || cache.settings != settings || cache.operation != operation
                || (!cache.created).then_some(cache.object.id) != selected
        }) {
            let add = operation == BrushOp::Paint && !settings.protect_alpha;
            let (object, pixels, created) = edit_layer(session, None, Some(args.points[0]), add).ok()??;
            self.cache = Some(StrokeCache {
                instance: session.document.instance, generation: session.generation, object, settings, operation,
                base: pixels.clone(), stable: pixels, stable_count: 0, dabs: Vec::new(), created,
            });
        }
        let cache = self.cache.as_mut()?;
        let inverse = invert_matrix(&object_matrix(&cache.object))?;
        let size = cache.object.size.map(f32::abs);
        if !size.iter().all(|side| side.is_finite() && *side > 0.0) { return None; }
        let scale = [cache.base.width as f32 / size[0], cache.base.height as f32 / size[1]];
        let radius = cache.settings.max_radius() + 2.0;
        let points = args.samples(|point| {
            let local = apply_matrix(&inverse, point);
            std::array::from_fn(|axis| {
                let edge = [cache.base.width, cache.base.height][axis] as f32 + radius;
                ((local[axis] - cache.object.pos[axis]) * scale[axis]).clamp(-radius, edge)
            })
        });
        if points.iter().any(|point| !point.x.is_finite() || !point.y.is_finite()) { return None; }
        let dabs = brush::dabs(&points, &cache.settings);
        // Stabilization and the provisional end dab can revise the tail.
        // Preserve only the prefix that agrees with the new sample sequence.
        let stable = dabs.iter().zip(&cache.dabs).take_while(|(next, previous)| next == previous).count();
        if stable < cache.stable_count {
            cache.stable = cache.base.clone();
            cache.stable_count = 0;
        }
        rasterize(&mut cache.stable, &dabs[cache.stable_count..stable], &cache.settings, operation)?;
        cache.stable_count = stable;
        let mut pixels = cache.stable.clone();
        rasterize(&mut pixels, &dabs[stable..], &cache.settings, operation)?;
        cache.dabs = dabs;
        if pixels == cache.base { return None; }
        let mut document = session.document.clone();
        let id = install_pixels(&mut document, &session.selection.ids, cache.object.clone(), pixels, cache.created);
        Some((document, Some(id)))
    }
}

fn rasterize(
    pixels: &mut PixelLayer, dabs: &[brush::Dab], settings: &BrushSettings, operation: BrushOp,
) -> Option<()> {
    if dabs.is_empty() { return Some(()); }
    let size = [pixels.width, pixels.height];
    let [left, top, right, bottom] = dabs.iter()
        .map(|dab| brush::dab_bounds(dab, settings.tip_kind, size))
        .filter(|[x0, y0, x1, y1]| x0 < x1 && y0 < y1)
        .fold([pixels.width, pixels.height, 0, 0], |[x0, y0, x1, y1], [l, t, r, b]| {
            [x0.min(l), y0.min(t), x1.max(r), y1.max(b)]
        });
    if left >= right || top >= bottom { return Some(()); }
    let [width, height] = [right - left, bottom - top];
    if u64::from(width) * u64::from(height) > 32_000_000 { return None; }
    let region = [left, top, width, height];
    let mut rgba = pixels.read_region(region);
    let mut raster = RasterViewMut::new(width, height, &mut rgba);
    for dab in dabs {
        let mut local = *dab;
        local.x -= left as f32;
        local.y -= top as f32;
        brush::paint_dab(&mut raster, &local, settings, operation, [left, top]);
    }
    pixels.set_region(region, &rgba);
    pixels.compact();
    Some(())
}
