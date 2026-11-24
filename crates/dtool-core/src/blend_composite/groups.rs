use super::{blend_of, composite, drawn_object, BlendMode};
use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::scene::{Prim, Scene};
use crate::selection::PixelSelection;
use crate::{Document, Object, ObjectId, ObjectKind, ViewState};
use std::collections::{HashMap, HashSet};

type GroupPaths = HashMap<ObjectId, Vec<ObjectId>>;
type Tagged<'a, T> = (&'a T, &'a [ObjectId], usize);

fn paths(doc: &Document, owners: impl Iterator<Item = ObjectId>, effect_groups: &[ObjectId]) -> GroupPaths {
    let masked: HashSet<_> = doc.layer_masks.iter().map(|mask| mask.object).collect();
    let isolated: HashSet<_> = doc.objects.iter()
        .filter(|object| crate::artboard::is_artboard(object) || matches!(object.kind, ObjectKind::Group)
            && (object.blend_mode != BlendMode::PassThrough || object.tone_blend.is_some() || blend_of(doc, object.id).is_some()
                || masked.contains(&object.id) || !matches!(object.transparency, crate::paint::Transparency::None)
                || object.opacity < 1.0 && (effect_groups.contains(&object.id)
                    || crate::layer_style::raster::has_group_effect(object))))
        .map(|object| object.id)
        .collect();
    if isolated.is_empty() {
        return HashMap::new();
    }
    let objects: HashMap<_, _> = doc.objects.iter().map(|object| (object.id, object.parent_id)).collect();
    let mut paths = HashMap::new();
    for id in owners {
        paths.entry(id).or_insert_with(|| {
            let mut path = Vec::new();
            let mut visited = HashSet::from([id]);
            if isolated.contains(&id) {
                path.push(id);
            }
            let mut parent = objects.get(&id).copied().flatten();
            while let Some(group) = parent {
                if !visited.insert(group) {
                    return Vec::new();
                }
                if isolated.contains(&group) {
                    path.push(group);
                }
                parent = objects.get(&group).copied().flatten();
            }
            path.reverse();
            path
        });
    }
    paths.retain(|_, path| !path.is_empty());
    paths
}

pub(super) fn scene_has_groups(doc: &Document, scene: &Scene) -> bool {
    !paths(doc, scene.prims.iter().filter_map(drawn_object), &[]).is_empty()
}

#[derive(Clone, Copy)]
pub(crate) struct RasterGrid<'a> {
    pub bounds: [f32; 4],
    pub size: [u32; 2],
    pub view: Option<&'a ViewState>,
}

#[derive(Clone, Copy)]
pub(super) struct PixelMask<'a> {
    coverage: &'a PixelSelection,
    inverse: [f32; 6],
    position: [f32; 2],
    size: [f32; 2],
}

impl RasterGrid<'_> {
    fn point(self, index: usize) -> [f32; 2] {
        let [width, height] = self.size;
        crate::scene::doc_point(
            self.bounds, width, height, (index % width as usize) as u32,
            (index / width as usize) as u32, self.view,
        )
    }
}

impl<'a> PixelMask<'a> {
    pub(super) fn new(doc: &'a Document, group: &Object) -> Option<Self> {
        let coverage = crate::raster_source::compositing_mask(doc, group)?;
        let size = group.size.map(f32::abs);
        if !size.iter().all(|extent| extent.is_finite() && *extent > 0.0) {
            return None;
        }
        Some(Self {
            coverage, inverse: invert_matrix(&object_matrix(group))?, position: group.pos, size,
        })
    }

    fn alpha(&self, grid: RasterGrid<'_>, index: usize) -> u8 {
        let point = grid.point(index);
        let [x, y] = apply_matrix(&self.inverse, point);
        let u = (x - self.position[0]) / self.size[0];
        let v = (y - self.position[1]) / self.size[1];
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            return 0;
        }
        self.coverage.get(
            ((u * self.coverage.width as f32) as u32).min(self.coverage.width - 1),
            ((v * self.coverage.height as f32) as u32).min(self.coverage.height - 1),
        )
    }

    pub(super) fn apply(&self, layer: &mut [u8], grid: RasterGrid<'_>) {
        for (index, pixel) in layer.chunks_exact_mut(4).enumerate() {
            if pixel[3] > 0 {
                pixel[3] = ((u16::from(pixel[3]) * u16::from(self.alpha(grid, index)) + 127) / 255) as u8;
            }
        }
    }
}

fn mix_backdrop(destination: &mut [u8], result: &[u8], weight: f32) {
    if weight >= 1.0 {
        destination.copy_from_slice(result);
    } else if weight > 0.0 {
        let before_alpha = f32::from(destination[3]) * (1.0 - weight);
        let result_alpha = f32::from(result[3]) * weight;
        let alpha = before_alpha + result_alpha;
        if alpha <= 0.0 {
            destination.fill(0);
            return;
        }
        for channel in 0..3 {
            destination[channel] = ((f32::from(destination[channel]) * before_alpha
                + f32::from(result[channel]) * result_alpha) / alpha).round() as u8;
        }
        destination[3] = alpha.round() as u8;
    }
}

pub(crate) fn render<T: Clone>(
    doc: &Document,
    items: &[T],
    owner: impl Fn(&T) -> Option<ObjectId>,
    effect_groups: &[ObjectId],
    grid: RasterGrid<'_>,
    pixels: &mut [u8],
    mut draw: impl FnMut(Vec<T>, f32, &mut [u8]),
) -> bool {
    let paths = paths(doc, items.iter().filter_map(&owner), effect_groups);
    if paths.is_empty() {
        return false;
    }
    let order: HashMap<_, _> = doc.objects.iter().enumerate().map(|(index, object)| (object.id, index + 1)).collect();
    let tagged = items.iter().map(|item| {
        let id = owner(item);
        let path = id.and_then(|id| paths.get(&id)).map_or(&[][..], Vec::as_slice);
        let position = id.and_then(|id| order.get(&id)).copied().unwrap_or_default();
        (item, path, position)
    }).collect();
    draw_groups(doc, &order, tagged, 1.0, grid, pixels, &mut draw);
    true
}

enum Batch<'a, T> {
    Item(&'a T),
    Group(ObjectId),
}

fn draw_groups<T: Clone>(
    doc: &Document,
    positions: &HashMap<ObjectId, usize>,
    items: Vec<Tagged<'_, T>>,
    inherited_opacity: f32,
    grid: RasterGrid<'_>,
    pixels: &mut [u8],
    draw: &mut impl FnMut(Vec<T>, f32, &mut [u8]),
) {
    let mut order = Vec::new();
    let mut groups = HashMap::<ObjectId, Vec<Tagged<'_, T>>>::new();
    let mut current_position = 0;
    // Group rows occupy their own position even when their descendants are hidden or interleaved.
    for (item, path, position) in items {
        if let Some((&id, rest)) = path.split_first() {
            groups.entry(id).or_insert_with(|| {
                order.push((positions[&id], Batch::Group(id)));
                Vec::new()
            }).push((item, rest, position));
        } else {
            current_position = current_position.max(position);
            order.push((current_position, Batch::Item(item)));
        }
    }
    order.sort_by_key(|(position, _)| *position);
    let mut run = Vec::new();
    for (_, batch) in order {
        let id = match batch {
            Batch::Item(item) => {
                run.push(item.clone());
                continue;
            }
            Batch::Group(id) => id,
        };
        if !run.is_empty() {
            draw(std::mem::take(&mut run), inherited_opacity, pixels);
        }
        let Some(children) = groups.remove(&id) else { continue };
        let Some(group) = doc.get_object(id) else { continue };
        let opacity = group.opacity.clamp(0.0, 1.0);
        if !(opacity > 0.0) {
            continue;
        }
        let mask = PixelMask::new(doc, group);
        let blend = blend_of(doc, id);
        let board = crate::artboard::is_artboard(group);
        let inverse = board.then(|| invert_matrix(&object_matrix(group))).flatten();
        let board_contains = |index| !board || inverse.is_some_and(|matrix| {
            let [x, y] = apply_matrix(&matrix, grid.point(index));
            let [left, top] = group.pos;
            x >= left && y >= top && x < left + group.size[0].abs() && y < top + group.size[1].abs()
        });
        if (mask.is_some() || board || group.tone_blend.is_some()) && group.blend_mode == BlendMode::PassThrough && blend.is_none()
            && matches!(group.transparency, crate::paint::Transparency::None)
        {
            let mut layer = pixels.to_vec();
            draw_groups(doc, positions, children, inherited_opacity * opacity, grid, &mut layer, draw);
            for (index, (destination, result)) in pixels.chunks_exact_mut(4).zip(layer.chunks_exact(4)).enumerate() {
                if destination != result && board_contains(index) {
                    let coverage = mask.map_or(255, |mask| mask.alpha(grid, index));
                    mix_backdrop(destination, result, opacity * f32::from(coverage) / 255.0);
                }
            }
        } else {
            let mut layer = vec![0; pixels.len()];
            draw_groups(doc, positions, children, inherited_opacity * opacity, grid, &mut layer, draw);
            if let Some(mask) = mask {
                mask.apply(&mut layer, grid);
            }
            if board {
                for (index, pixel) in layer.chunks_exact_mut(4).enumerate() {
                    if pixel[3] != 0 && !board_contains(index) {
                        pixel.fill(0);
                    }
                }
            }
            super::apply_transparency(&mut layer, group, grid.bounds, grid.size, grid.view);
            composite(pixels, &layer, blend, opacity);
        }
    }
    if !run.is_empty() {
        draw(run, inherited_opacity, pixels);
    }
}

pub(crate) fn normalize_opacity(opacity: &mut f32, group_opacity: f32) {
    if group_opacity > 0.0 && group_opacity < 1.0 {
        *opacity = (*opacity / group_opacity).clamp(0.0, 1.0);
    }
}

pub(crate) fn normalize_prim_opacity(prim: &mut Prim, group_opacity: f32) {
    match prim {
        Prim::Fill { opacity, .. } | Prim::Appearance { opacity, .. } | Prim::Stroke { opacity, .. } | Prim::Image { opacity, .. }
        | Prim::Text { opacity, .. } | Prim::Label { opacity, .. } => normalize_opacity(opacity, group_opacity),
        _ => {}
    }
}
