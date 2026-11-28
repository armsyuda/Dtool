use super::{blob_object, flood_fill_paint};
use crate::blend::BlendMode;
use crate::boolean::{flood_appearance_masks, flood_source_curve, FloodReplacement};
use crate::command::Session;
use crate::commands::appearance::{AppearanceList, AppearanceSlots, Kind, PaintSlot, RowBlend};
use crate::curve::Curve;
use crate::object::{ClipRole, Object, ObjectId, ObjectKind};
use crate::paint::{Paint, StrokeStyle, Transparency};
use dtool_protocol::{Command, CommandError, CommandResult};

pub(super) fn apply_partial(
    session: &mut Session, command: &Command, source: ObjectId, curve: Curve, mask: Curve, paint: Paint,
) -> CommandResult {
    let local = flood_source_curve(&session.document, source, curve.clone())
        .ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?;
    let mut replacement = blob_object(local, Paint::None, StrokeStyle::default())?;
    replacement.fill = flood_fill_paint(session, command, paint, replacement.size)?;
    let id = apply_partition(session, source, Some(replacement), Some(mask), curve)?
        .ok_or_else(CommandError::not_found)?;
    finish(session, command, source, id, true)
}

pub(super) fn apply_connected(
    session: &mut Session, command: &Command, clicked: Option<ObjectId>, curve: Curve,
    replacements: Vec<FloodReplacement>, paint: Paint,
) -> CommandResult {
    let whole = blob_object(curve.clone(), Paint::None, StrokeStyle::default())?;
    let paint = flood_fill_paint(session, command, paint, whole.size)?;
    let paint_uses_frame = !matches!(paint, Paint::None | Paint::Solid { .. });
    let mut applied = Vec::new();
    let mut changed = false;
    for FloodReplacement { source, whole: entire_source, mask, removed_region, paint_region } in replacements {
        let warped_frame = if paint_uses_frame && crate::commands::warp::LiveWarp::for_object(&session.document, source).is_some() {
            let local = flood_source_curve(&session.document, source, curve.clone())
                .ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?;
            Some(blob_object(local, Paint::None, StrokeStyle::default())?)
        } else { None };
        let frame = warped_frame.as_ref().unwrap_or(&whole);
        if entire_source {
            let object = session.document.get_object(source).ok_or_else(CommandError::not_found)?;
            if session.document.is_object_locked(object) {
                return Err(CommandError::new("CANNOT_EXECUTE"));
            }
            // An untransformed source already has the right frame. A solid fill
            // is independent of the source frame, including rotation and shear.
            if matches!(paint, Paint::None | Paint::Solid { .. })
                || crate::geometry::object_matrix(object) == crate::geometry::IDENTITY
            {
                let mut local = paint.clone();
                let [x, y] = [frame.pos[0] - object.pos[0], frame.pos[1] - object.pos[1]].map(f64::from);
                crate::paint::transform_paint_matrix(&mut local, [1.0, 0.0, 0.0, 1.0, x, y]);
                let appearance = object.appearance();
                let mut next = appearance.clone();
                next.replace_fill_stack(local)?;
                if next != appearance {
                    let object = session.document.find_object_mut(source.0).ok_or_else(CommandError::not_found)?;
                    object.set_appearance(&next)?;
                    changed = true;
                }
                applied.push((source, source));
                continue;
            }
        }
        let replacement = if let Some(region) = paint_region {
            let region = flood_source_curve(&session.document, source, region)
                .ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?;
            let mut piece = blob_object(region, Paint::None, StrokeStyle::default())?;
            let [x, y] = [frame.pos[0] - piece.pos[0], frame.pos[1] - piece.pos[1]].map(f64::from);
            // Blob geometry starts at its own origin. Keep one paint coordinate space across all pieces.
            piece.fill = paint.clone();
            crate::paint::transform_paint_matrix(&mut piece.fill, [1.0, 0.0, 0.0, 1.0, x, y]);
            Some(piece)
        } else {
            None
        };
        if let Some(id) = apply_partition(session, source, replacement, mask, removed_region)? {
            applied.push((source, id));
        }
        changed = true;
    }
    let (source, id) = applied.iter().find(|(source, _)| Some(*source) == clicked)
        .or_else(|| applied.first()).copied().ok_or_else(CommandError::not_found)?;
    let mut result = finish(session, command, source, id, changed)?;
    result["regions"] = serde_json::json!(applied.into_iter().map(|(_, id)| id.0).collect::<Vec<_>>());
    Ok(result)
}

fn apply_partition(
    session: &mut Session, source: ObjectId, mut replacement: Option<Object>, mask: Option<Curve>,
    removed_region: Curve,
) -> Result<Option<ObjectId>, CommandError> {
    let document = &session.document;
    let index = document.objects.iter().position(|object| object.id == source).ok_or_else(CommandError::not_found)?;
    let original = document.objects[index].clone();
    if document.is_object_locked(&original) {
        return Err(CommandError::new("CANNOT_EXECUTE"));
    }

    let appearance = original.appearance();
    if let Some(layer) = &mut replacement {
        layer.parent_id = Some(source);
        layer.page_id = original.page_id;
        layer.baked_warps.clone_from(&original.baked_warps);
    }

    let mut shell = original.clone();
    shell.kind = ObjectKind::Group;
    shell.shape = None;
    shell.path_brush = None;
    shell.vector_crop = None;
    shell.contour = None;
    shell.expanded_stroke_effects = true;
    if shell.blend_mode == BlendMode::PassThrough {
        shell.blend_mode = BlendMode::Normal;
    }
    shell.set_appearance(&AppearanceList::default())?;

    let mut ink = original.clone();
    ink.parent_id = Some(source);
    ink.blend_mode = BlendMode::Normal;
    ink.blend_ranges = None;
    ink.transparency = Transparency::None;
    ink.effects.clear();
    ink.opacity = 1.0;
    ink.clip_role = ClipRole::None;
    ink.stroke_mask_outline = false;
    ink.set_appearance(&AppearanceList::default())?;

    let layers = if appearance.render_order(ink.paint_order).into_iter().any(|(kind, index)| {
        let slot = PaintSlot { kind, index };
        appearance.hidden.is_visible(slot) && appearance.blends.get(slot).erases()
    }) {
        let masks = flood_appearance_masks(document, &original, removed_region, replacement.as_ref())
            .ok_or_else(|| CommandError::new("CANNOT_EXECUTE"))?;
        isolated_layers(ink, appearance, replacement, masks)?
    } else {
        split_rows(ink, appearance, replacement, mask)?
    };

    let document = &mut session.document;
    document.objects[index] = shell;
    let mut next = index + 1;
    let mut replacement_id = None;
    for (layer, clip, is_replacement) in layers {
        let id = document.create_object(layer);
        let layer = document.objects.pop().expect("created fill layer");
        document.objects.insert(next, layer);
        next += 1;
        if let Some(mut mask) = clip {
            mask.parent_id = Some(id);
            document.create_object(mask);
            let mask = document.objects.pop().expect("created fill mask");
            document.objects.insert(next, mask);
            next += 1;
        }
        if is_replacement {
            replacement_id = Some(id);
        }
    }
    Ok(replacement_id)
}

type PartitionLayer = (Object, Option<Object>, bool);

fn clip_layer(source: &Object, curve: Curve) -> Result<Object, CommandError> {
    let mut layer = blob_object(curve, Paint::None, StrokeStyle::default())?;
    layer.clip_role = ClipRole::Mask;
    layer.page_id = source.page_id;
    layer.baked_warps.clone_from(&source.baked_warps);
    Ok(layer)
}

fn isolated_layers(
    mut ink: Object, appearance: AppearanceList, replacement: Option<Object>, masks: [Option<Curve>; 3],
) -> Result<Vec<PartitionLayer>, CommandError> {
    let [original, cleared, painted] = masks;
    let mut layers = Vec::new();
    for (mask, replace_fill, is_replacement) in [
        (original, false, false), (cleared, true, false), (painted, true, true),
    ] {
        let Some(mask) = mask else { continue };
        let mut layer = ink.clone();
        let mut rows = appearance.clone();
        if replace_fill {
            let paint = if is_replacement && let Some(paint_layer) = &replacement {
                let mut paint = paint_layer.fill.clone();
                let [x, y] = ink.pos.map(f64::from);
                let world = kurbo::Affine::new(crate::geometry::object_matrix(&ink).map(f64::from))
                    * kurbo::Affine::translate((x, y));
                if world.determinant() == 0.0 || !world.is_finite() {
                    return Err(CommandError::invalid_params());
                }
                let [px, py] = paint_layer.pos.map(f64::from);
                let to_object = world.inverse() * kurbo::Affine::translate((px, py));
                crate::paint::transform_paint_matrix(&mut paint, to_object.as_coeffs());
                paint
            } else {
                Paint::None
            };
            rows.replace_fill_stack(paint)?;
        }
        layer.set_appearance(&rows)?;
        layers.push((layer, Some(clip_layer(&ink, mask)?), is_replacement));
    }
    // Clipping or masking by the original object still uses its silhouette.
    ink.path_brush = None;
    ink.stroke_mask_outline = true;
    ink.pickable = false;
    layers.push((ink, None, false));
    Ok(layers)
}

fn split_rows(
    mut ink: Object, appearance: AppearanceList, mut replacement: Option<Object>, mask: Option<Curve>,
) -> Result<Vec<PartitionLayer>, CommandError> {
    let complement = mask.map(|curve| clip_layer(&ink, curve)).transpose()?;
    let mut layers = Vec::new();
    let mut has_outline = false;
    for (kind, index) in appearance.render_order(ink.paint_order) {
        let slot = PaintSlot { kind, index };
        let mut layer = ink.clone();
        layer.blend_mode = match appearance.blends.get(slot) {
            RowBlend::Standard(mode) => mode,
            RowBlend::Operator(_) => BlendMode::Normal,
        };
        match kind {
            Kind::Fill => {
                layer.path_brush = None;
                layer.fill = appearance.fills[index].clone();
                if appearance.hidden.is_visible(slot) && layer.fill != Paint::None
                    && let Some(mask) = &complement
                {
                    layers.push((layer, Some(mask.clone()), false));
                }
                if index == 0 && let Some(paint) = replacement.take() {
                    layers.push((paint, None, true));
                }
            }
            Kind::Stroke => {
                layer.stroke = appearance.strokes[index].clone();
                layer.stroke_mask_outline = !has_outline;
                has_outline = true;
                if !appearance.hidden.is_visible(slot) {
                    layer.stroke = StrokeStyle::default();
                    layer.path_brush = None;
                }
                layers.push((layer, None, false));
            }
        }
    }
    if !has_outline {
        ink.path_brush = None;
        ink.stroke_mask_outline = true;
        layers.push((ink, None, false));
    }

    Ok(layers)
}

fn finish(session: &mut Session, command: &Command, source: ObjectId, id: ObjectId, changed: bool) -> CommandResult {
    if changed && command.params["select"] == true {
        session.selection.set(vec![id], crate::selection::SelectOp::Replace);
    }
    Ok(serde_json::json!({"id": id.0, "source": source.0, "mode": "knockout", "changed": changed}))
}
