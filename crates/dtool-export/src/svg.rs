use base64::Engine;
use dtool_core::{blend::BlendMode, Document, Object, ObjectId};
use dtool_core::layer_style::{EffectKind, EffectParams, LayerEffect};
use dtool_core::layer_style::raster::{EffectLayer, effect_bounds, export_effect_layers};
use dtool_core::scene::Prim;
use dtool_protocol::CommandError;
use image::ImageEncoder;
use std::{collections::{HashMap, HashSet}, fmt::Write};
use crate::{DocumentExportExt, ExportArea, ExportControl, ExportReceipt, ExportRequest, ObjectExportExt};

const SVG_WARN_SCOPE: &str = "svg";
pub const MAX_COORDINATE_PRECISION: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SvgOptions {
    pub text_as_curves: bool,
    pub raster_dpi: u32,
    pub rasterize: RasterizeMode,
    pub hex_colours: bool,
    pub tile_patterns: bool,
    pub line_breaks: bool,
    pub flatten_transforms: bool,
    pub set_viewbox: bool,
    pub longer_text_spans: bool,
    pub precision: u8,
    pub embed_images: bool,
    pub skip_slice_hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RasterizeMode {
    Nothing,
    Unsupported,
    Everything,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum SvgPreset {
    ForExport,
    DigitalSmall,
    DigitalHigh,
    Flatten,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            text_as_curves: false,
            raster_dpi: 72,
            rasterize: RasterizeMode::Unsupported,
            hex_colours: true,
            tile_patterns: false,
            line_breaks: true,
            flatten_transforms: false,
            set_viewbox: true,
            longer_text_spans: false,
            precision: 3,
            embed_images: true,
            skip_slice_hidden: true,
        }
    }
}

impl SvgOptions {
    pub fn preset(p: SvgPreset) -> Self {
        match p {
            SvgPreset::ForExport => Self::default(),
            SvgPreset::DigitalSmall => Self {
                precision: 2,
                ..Self::default()
            },
            SvgPreset::DigitalHigh => Self {
                precision: 4,
                ..Self::default()
            },
            SvgPreset::Flatten => Self {
                text_as_curves: true,
                ..Self::default()
            },
        }
    }

    pub fn unapplied_field(&self) -> Option<&'static str> {
        use dtool_core::effect_raster::{MAX_DPI, MIN_DPI};
        if self.precision > MAX_COORDINATE_PRECISION {
            return Some("precision");
        }
        if !(MIN_DPI as u32..=MAX_DPI as u32).contains(&self.raster_dpi) {
            return Some("raster_dpi");
        }
        if self.flatten_transforms {
            return Some("flatten_transforms");
        }
        if self.tile_patterns {
            return Some("tile_patterns");
        }
        if self.longer_text_spans {
            return Some("longer_text_spans");
        }
        if !self.embed_images {
            return Some("embed_images");
        }
        if !self.skip_slice_hidden {
            return Some("skip_slice_hidden");
        }
        None
    }
}

pub fn export_svg(
    doc: &Document,
    req: &ExportRequest,
    opts: &SvgOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    let (bytes, warnings) = encode_svg(doc, req, opts, None, control)?;
    crate::write_atomic(&req.path, &bytes).map_err(crate::export_io_error)?;
    Ok(ExportReceipt { path: req.path.clone(), bytes: bytes.len() as u64, warnings })
}

pub(crate) fn encode_svg(
    doc: &Document,
    req: &ExportRequest,
    opts: &SvgOptions,
    output_size: Option<(u32, u32)>,
    control: &ExportControl,
) -> Result<(Vec<u8>, Vec<String>), CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("export.cancelled"));
    }
    if opts.unapplied_field().is_some() {
        return Err(crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable"));
    }

    let (origin_x, origin_y, width, height) = match &req.area {
        ExportArea::AllPages | ExportArea::AllSpreads | ExportArea::PageRun { .. } => return Err(crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable")),
        area @ (ExportArea::WholeDocument | ExportArea::CurrentPage | ExportArea::CurrentSpread | ExportArea::Page(_) | ExportArea::Spread(_)) => match doc.area_bounds(area) {
            Some(bounds) => bounds,
            None => return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area")),
        },
        ExportArea::Rect([x, y, w, h]) => (*x, *y, *w, *h),
        ExportArea::Artboard(id) => match crate::raster::artboard_region(doc, *id) {
            Some(bounds) => bounds,
            None => return Err(crate::export_error("NOT_FOUND", "error.export.bad_area")),
        },
        ExportArea::SelectionWithBackground | ExportArea::SelectionWithoutBackground => match doc.selection_bounds() {
            Some(bounds) => bounds,
            None => return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area")),
        },
    };

    if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
        return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
    }

    let mut svg_content = String::new();
    let mut warnings = Vec::new();
    
    let unit_points = crate::raster::document_unit_points(doc);
    let page_scoped = req.area.limits_objects_to_page();
    write_svg_header(&mut svg_content, width, height, unit_points, output_size, page_scoped, opts);
    let native_css_scale = unit_points * (96.0 / 72.0);
    let css_scale = output_size.map_or([native_css_scale; 2], |(w, h)| [w as f32 / width, h as f32 / height]);
    let vector_tolerance = crate::vector_export_tolerance(f64::from(css_scale[0].max(css_scale[1])) * 0.75);
    let [scale_x, scale_y] = if opts.set_viewbox { [1.0; 2] } else { css_scale };
    if opts.rasterize == RasterizeMode::Everything {
        let region = (origin_x, origin_y, width, height);
        let resolution = crate::raster::ExportOptions { dpi: Some(opts.raster_dpi as f32), ..Default::default() };
        let (pixels_w, pixels_h) = crate::raster::target_size(
            (0.0, 0.0, width * css_scale[0], height * css_scale[1]), 96.0, &resolution,
        )?;
        let size = [pixels_w as usize, pixels_h as usize];
        let pixels = if page_scoped {
            let pages: Vec<_> = crate::output_page_batches(doc, &req.area, req.pages.as_deref()).into_iter().flatten().collect();
            if pages.is_empty() { vec![0; size[0] * size[1] * 4] } else { crate::render_pages_rgba_region(doc, &pages, region, size) }
        } else {
            crate::raster::render_rgba_for_area(doc, &req.area, region, size)
        };
        if control.is_cancelled() { return Err(CommandError::new("export.cancelled")); }
        let data = png_data(&pixels, pixels_w, pixels_h, image::ExtendedColorType::Rgba8)?;
        let width = format_float(width * scale_x, opts.precision.max(6));
        let height = format_float(height * scale_y, opts.precision.max(6));
        let _ = writeln!(svg_content, "<image width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/png;base64,{data}\"/>");
        return Ok((finish_svg(svg_content, opts.line_breaks), warnings));
    }
    let transformed = origin_x != 0.0 || origin_y != 0.0 || scale_x != 1.0 || scale_y != 1.0;
    if transformed {
        let x = format_float(scale_x, 6);
        let scale = if scale_x == scale_y { x } else { format!("{x} {}", format_float(scale_y, 6)) };
        svg_content.push_str(&format!(
            "<g transform=\"scale({scale}) translate({} {})\">\n",
            format_float(-origin_x, opts.precision),
            format_float(-origin_y, opts.precision)
        ));
    }
    let scaled_options = output_size.map(|_| {
        let scale = css_scale[0].max(css_scale[1]) / native_css_scale;
        SvgOptions { raster_dpi: (opts.raster_dpi as f32 * scale).round().max(1.0) as u32, ..opts.clone() }
    });
    let opts = scaled_options.as_ref().unwrap_or(opts);
    
    let mut children = HashMap::<Option<ObjectId>, Vec<&Object>>::new();
    let mut named_ids = HashSet::new();
    let mut object_ids = HashMap::new();
    let members = crate::artboard_members(doc, &req.area);
    for object in doc.objects() {
        if members.as_ref().is_some_and(|ids| !ids.contains(&object.id)) {
            continue;
        }
        // Groups have no outline of their own, so export_visible excludes them.
        let visible = if object.is_group() || dtool_core::artboard::is_artboard(object) {
            dtool_core::scene::is_drawn(doc, object.id) && object.opacity.is_finite() && object.opacity > 0.0
        } else {
            crate::export_visible(doc, object)
        };
        if !visible || dtool_core::clip_mask::is_mask(doc, object) || dtool_core::tone::attached(doc, object) {
            continue;
        }
        if !object.is_group() && !dtool_core::artboard::is_artboard(object) {
            let id = if page_scoped && matches!(object.kind, dtool_core::ObjectKind::FrameText { .. }) {
                // An automatic frame name may contain its entire linked story.
                format!("dtool-frame-{}", object.id.0)
            } else {
                let name = sanitize_id(&object.name);
                // Keep named layers separate from generated clip, mask, and resource IDs.
                let reserved = ["dtool-", "crop-", "k08-"].iter().any(|prefix| name.starts_with(prefix));
                let valid_start = name.starts_with(|ch: char| ch == '_' || ch.is_alphabetic());
                if !reserved && valid_start && named_ids.insert(name.clone()) {
                    name
                } else {
                    format!("dtool-layer-{}", object.id.0)
                }
            };
            object_ids.insert(object.id, id);
        }
        let mut parent = object.parent_id.and_then(|id| doc.get_object(id));
        while let Some(owner) = parent.filter(|owner| !owner.is_group() && !dtool_core::artboard::is_artboard(owner)) {
            parent = owner.parent_id.and_then(|id| doc.get_object(id));
        }
        children.entry(parent.map(|owner| owner.id)).or_default().push(object);
    }
    let effect_scene = if children.values().flatten().any(|object| object.effects.iter().any(|effect| effect.enabled)) {
        dtool_render::scene::install();
        Some(dtool_core::scene::build_scene(doc, &dtool_core::CanvasFlags::default()))
    } else {
        None
    };
    let effect_sources = effect_scene.as_ref().map_or_else(Default::default, |scene| {
        dtool_core::layer_style::raster::export_effect_sources(doc, scene)
    });
    let bitmap_blurs = if opts.rasterize == RasterizeMode::Nothing { HashMap::new() }
        else { dtool_core::layer_style::raster::export_bitmap_blurs(doc, &effect_sources) };
    SvgLayers {
        doc, opts, control, children, object_ids, effect_scene, effect_sources, bitmap_blurs, area: [origin_x, origin_y, width, height],
        ranges: crate::blend_range_mask::RangeMasks::new(doc), vector_tolerance,
        page_area: page_scoped.then_some((origin_x, origin_y, width, height)),
    }.write_children(None, &mut svg_content, &mut warnings)?;

    if transformed {
        svg_content.push_str("</g>\n");
    }
    Ok((finish_svg(svg_content, opts.line_breaks), warnings))
}

struct SvgLayers<'a> {
    doc: &'a Document,
    opts: &'a SvgOptions,
    control: &'a ExportControl,
    children: HashMap<Option<ObjectId>, Vec<&'a Object>>,
    object_ids: HashMap<ObjectId, String>,
    effect_scene: Option<dtool_core::scene::Scene>,
    effect_sources: HashMap<ObjectId, Vec<Prim>>,
    bitmap_blurs: HashMap<ObjectId, ([f32; 4], Vec<EffectLayer>)>,
    area: [f32; 4],
    ranges: Option<crate::blend_range_mask::RangeMasks<'a>>,
    page_area: Option<(f32, f32, f32, f32)>,
    vector_tolerance: f64,
}

impl SvgLayers<'_> {
    fn write_children(&self, parent: Option<ObjectId>, content: &mut String, warnings: &mut Vec<String>) -> Result<(), CommandError> {
        for &object in self.children.get(&parent).into_iter().flatten() {
            if self.control.is_cancelled() {
                return Err(CommandError::new("export.cancelled"));
            }
            if object.is_group() || dtool_core::artboard::is_artboard(object) {
                self.write_group(object, content, warnings)?;
            } else {
                self.write_leaf(object, content, warnings)?;
            }
        }
        Ok(())
    }

    fn has_blended_child(&self, id: ObjectId) -> bool {
        self.children.get(&Some(id)).into_iter().flatten().any(|child| {
            !child.blend_mode.is_normal()
                || child.is_group() && child.blend_mode == BlendMode::PassThrough && self.has_blended_child(child.id)
        })
    }

    fn write_group(&self, group: &Object, content: &mut String, warnings: &mut Vec<String>) -> Result<(), CommandError> {
        if !self.children.contains_key(&Some(group.id)) {
            return Ok(());
        }
        let board_clip = dtool_core::artboard::is_artboard(group)
            && write_k08_open(content, self.doc, group, &[crate::artboard_ring(group)], self.opts);
        let masked = write_pixel_mask(content, self.doc, group, self.opts)?;
        let opacity = group.opacity.clamp(0.0, 1.0);
        let blur = dtool_core::layer_style::raster::group_effect_target(group)
            .then(|| group.effects.iter().find_map(dtool_core::layer_style::raster::group_blur_params))
            .flatten();
        let shadow = native_outer_shadow(group).filter(|_| {
            dtool_core::layer_style::raster::group_effect_target(group)
                && !self.has_blended_child(group.id)
                && !self.doc.subtree_ids(group.id).iter().any(|id| {
                    *id != group.id && self.doc.get_object(*id).is_some_and(|child| child.effects.iter().any(|effect| effect.enabled))
                })
        });
        let passthrough = group.blend_mode == BlendMode::PassThrough
            && !group.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid());
        if passthrough && (masked || opacity < 1.0 || blur.is_some()) && self.has_blended_child(group.id) {
            warnings.push(format!("export.svg.pass_through_isolated:{}", group.id.0));
        }
        let id = group.id.0;
        let blend = blend_style(group.blend_mode);
        let isolation = if passthrough { "" } else { "isolation:isolate;" };
        let _ = write!(content, "<g id=\"dtool-group-{id}\" opacity=\"{}\"", format_float(opacity, 6));
        if !isolation.is_empty() || !blend.is_empty() {
            let _ = write!(content, " style=\"{isolation}{blend}\"");
        }
        if masked {
            let _ = write!(content, " mask=\"url(#dtool-pixel-mask-{id})\"");
        }
        content.push_str(">\n");
        let transparent = self.open_transparency(group, content, warnings);
        let range = self.open_blend_range(group, content, warnings)?;
        let filter_close = (blur.is_some() || shadow.is_some()).then(|| {
            let scene = self.effect_scene.as_ref()?;
            let members = self.doc.subtree_ids(group.id);
            let prims = scene.prims.iter().filter(|prim| {
                dtool_core::layer_style::raster::prim_object(prim).is_some_and(|id| id != group.id && members.contains(&id))
            });
            let bounds = effect_bounds(self.doc, &[], group.id, prims)?;
            Some(if let Some(blur) = blur {
                open_gaussian_blur(content, id, blur, bounds, self.opts)
            } else {
                open_drop_shadow(content, id, shadow?, bounds, self.opts)
            })
        }).flatten();
        let rasterize = self.opts.rasterize != RasterizeMode::Nothing;
        let shadow_written = shadow.is_some() && filter_close.is_some();
        let group_effects = if rasterize && !shadow_written {
            let scale = crate::raster::document_unit_points(self.doc) * self.opts.raster_dpi.max(1) as f32 / 72.0;
            crate::effect_image::group_effect_images(self.doc, group, self.effect_scene.as_ref(), scale)
        } else {
            Default::default()
        };
        for (blend, raster) in group_effects.behind {
            write_effect_image(content, raster, blend, self.opts)?;
        }
        let (bounds, behind, above) = self.effect_sources.get(&group.id).map_or_else(Default::default, |prims| {
            let bounds = effect_bounds(self.doc, &[], group.id, prims);
            let (behind, above) = export_effect_layers(self.doc, group.id, &prims.iter().collect::<Vec<_>>());
            (bounds, behind, above)
        });
        if !rasterize && (!behind.is_empty() || !above.is_empty()
            || (!shadow_written && group.effects.iter().any(dtool_core::layer_style::raster::group_effect_enabled)))
        {
            warnings.push(format!("export.svg.effects_not_rasterized:{id}"));
        }
        if rasterize && let Some(bounds) = bounds {
            write_effect_images(content, self.doc, bounds, &behind, self.opts)?;
        }
        self.write_children(Some(group.id), content, warnings)?;
        for (blend, raster) in group_effects.above {
            write_effect_image(content, raster, blend, self.opts)?;
        }
        if rasterize && let Some(bounds) = bounds {
            write_effect_images(content, self.doc, bounds, &above, self.opts)?;
        }
        if let Some(close) = filter_close {
            content.push_str(close);
        }
        if range {
            content.push_str("</g>\n");
        }
        if transparent {
            content.push_str("</g>\n");
        }
        content.push_str("</g>\n");
        if board_clip {
            content.push_str("</g>\n");
        }
        Ok(())
    }

    fn write_leaf(&self, object: &Object, content: &mut String, warnings: &mut Vec<String>) -> Result<(), CommandError> {
        let doc = self.doc;
        let opts = self.opts;
        let rasterize = opts.rasterize != RasterizeMode::Nothing;
        let id = &self.object_ids[&object.id];
        let sources = self.effect_sources.get(&object.id);
        let bounds = sources.and_then(|prims| effect_bounds(doc, &[], object.id, prims));
        let bitmap_blur = self.bitmap_blurs.get(&object.id);
        if self.page_area.is_some_and(|area| {
            !crate::vector_bounds_intersect(crate::vector_object_bounds(doc, object), area)
                && !bounds.or_else(|| bitmap_blur.map(|(rect, _)| *rect))
                    .is_some_and(|[x, y, w, h]| crate::vector_bounds_intersect([x, y, x + w, y + h], area))
        }) {
            return Ok(());
        }
        let k08 = dtool_core::clip_mask::layer_clip_region(doc, object.id);
        if k08.as_ref().is_some_and(|rings| !rings.iter().any(|ring| ring.len() >= 3)) {
            return Ok(());
        }
        let blend = blend_style(object.blend_mode);
        if !blend.is_empty() {
            let _ = writeln!(content, "<g style=\"{blend}\">");
        }
        let (behind, above) = sources.map_or_else(Default::default, |prims| {
            export_effect_layers(doc, object.id, &prims.iter().collect::<Vec<_>>())
        });
        let native_blur = crate::effect_image::solid_leaf_blur(object, sources.map(Vec::as_slice)).zip(bounds);
        let crop = crate::vector_crop_bounds(object);
        let filter_close = if let Some(((_, blur), bounds)) = native_blur {
            Some(open_gaussian_blur(content, object.id.0, blur, bounds, opts))
        } else if let Some(shadow) = native_outer_shadow(object)
            && let Some(bounds) = bounds
            && matches!(sources.map(Vec::as_slice), Some([Prim::Fill { paint: dtool_core::paint::Paint::Solid { .. }, .. }]))
            && object.path_brush.is_none()
            && crop.is_none() && k08.is_none()
            && dtool_core::scene::oriented_vector_crop(object).is_none()
            && matches!(object.transparency, dtool_core::paint::Transparency::None)
            && object.blend_ranges.is_none() && object.tone_blend.is_none()
            && dtool_core::raster_source::compositing_mask(doc, object).is_none()
        {
            Some(open_drop_shadow(content, object.id.0, shadow, bounds, opts))
        } else {
            None
        };
        if filter_close.is_none() && rasterize && let Some(bounds) = bounds {
            // Effects use the cropped source geometry and may extend beyond it.
            write_effect_images(content, doc, bounds, &behind, opts)?;
        } else if filter_close.is_none() && !rasterize && (!behind.is_empty() || !above.is_empty()) {
            warnings.push(format!("export.svg.effects_not_rasterized:{}", object.id.0));
        }
        if let Some(quad) = dtool_core::scene::oriented_vector_crop(object) {
            let points = quad.into_iter().map(|[x, y]| {
                format!("{},{}", format_float(x, opts.precision), format_float(y, opts.precision))
            }).collect::<Vec<_>>().join(" ");
            let id = object.id.0;
            let _ = writeln!(content, "<clipPath id=\"crop-{id}\"><polygon points=\"{points}\"/></clipPath>\n<g clip-path=\"url(#crop-{id})\">");
        } else if let Some([x, y, w, h]) = crop {
            content.push_str(&format!(
                "<clipPath id=\"crop-{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>\n<g clip-path=\"url(#crop-{})\">\n",
                object.id.0,
                format_float(x, opts.precision),
                format_float(y, opts.precision),
                format_float(w, opts.precision),
                format_float(h, opts.precision),
                object.id.0
            ));
        }
        let clipped = k08.as_ref().is_some_and(|rings| write_k08_open(content, doc, object, rings, opts));
        let pixel_mask = write_pixel_mask(content, doc, object, opts)?;
        if pixel_mask {
            let _ = writeln!(content, "<g mask=\"url(#dtool-pixel-mask-{})\">", object.id.0);
        }
        let transparent = self.open_transparency(object, content, warnings);
        let clear_text = self.ranges.as_ref().and_then(|ranges| ranges.clear_text_background(object));
        let range = clear_text.is_none() && self.open_blend_range(object, content, warnings)?;
        let object = clear_text.as_ref().unwrap_or(object);
        if rasterize && let Some(ink) = dtool_core::path_brush::Ink::new(doc, object) {
            let mut base = object.clone();
            base.stroke.paint = dtool_core::paint::Paint::None;
            let under = matches!(object.paint_order, dtool_core::paint::PaintOrder::StrokeUnder);
            let mut image = String::new();
            if let Some((bounds, layer)) = crate::effect_image::brush_blur(object, &ink) {
                write_effect_images(&mut image, doc, bounds, &[layer], opts)?;
            } else {
                image = path_brush_image(doc, object, &ink, opts)?;
            }
            if under {
                content.push_str(&image);
            }
            write_object(content, doc, &base, id, opts, warnings, self.vector_tolerance);
            if !under {
                content.push_str(&image);
            }
        } else if let Some((bounds, layers)) = bitmap_blur {
            write_effect_images(content, doc, *bounds, layers, opts)?;
        } else if !write_raster_layer(content, doc, object, opts)? {
            if !rasterize && object.path_brush.is_some() {
                warnings.push(format!("export.svg.path_brush_not_rasterized:{}", object.id.0));
            }
            write_object(content, doc, object, id, opts, warnings, self.vector_tolerance);
        }
        if range {
            content.push_str("</g>\n");
        }
        if transparent {
            content.push_str("</g>\n");
        }
        if pixel_mask {
            content.push_str("</g>\n");
        }
        if clipped {
            content.push_str("</g>\n");
        }
        if crop.is_some() {
            content.push_str("</g>\n");
        }
        if let Some(close) = filter_close {
            content.push_str(close);
        } else if rasterize && let Some(bounds) = bounds {
            write_effect_images(content, doc, bounds, &above, opts)?;
        }
        if !blend.is_empty() {
            content.push_str("</g>\n");
        }
        Ok(())
    }

    fn open_blend_range(&self, object: &Object, content: &mut String, warnings: &mut Vec<String>) -> Result<bool, CommandError> {
        if self.opts.rasterize == RasterizeMode::Nothing {
            if object.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid()) {
                warnings.push(format!("export.svg.blend_range_not_rasterized:{}", object.id.0));
            }
            return Ok(false);
        }
        let scale = self.opts.raster_dpi.max(300) as f32 * crate::raster::document_unit_points(self.doc) / 72.0;
        let Some(mask) = self.ranges.as_ref().and_then(|ranges| ranges.sample(object, self.area, scale)) else {
            return Ok(false);
        };
        let [x, y, width, height] = mask.bounds.map(|value| format_float(value, self.opts.precision.max(6)));
        let pixels: Vec<_> = mask.alpha.into_iter().flat_map(|alpha| [255, 255, 255, alpha]).collect();
        let data = png_data(&pixels, mask.size[0], mask.size[1], image::ExtendedColorType::Rgba8)?;
        let id = object.id.0;
        let _ = writeln!(content,
            "<defs><mask id=\"dtool-blend-range-{id}\" maskUnits=\"userSpaceOnUse\" maskContentUnits=\"userSpaceOnUse\" x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" style=\"mask-type:alpha\"><image x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{data}\"/></mask></defs>\n<g mask=\"url(#dtool-blend-range-{id})\">");
        Ok(true)
    }

    fn open_transparency(&self, object: &Object, content: &mut String, warnings: &mut Vec<String>) -> bool {
        use dtool_core::paint::{GradientKind, Paint, Transparency};
        if matches!(object.transparency, Transparency::None) { return false; }
        let id = object.id.0;
        let [x, y, w, h] = self.area;
        let f = |v| format_float(v, self.opts.precision.max(6));
        let (mut defs, ink) = if let Transparency::InverseBitmap(bitmap) = &object.transparency {
            let Some(([width, height], pixels)) = crate::effect_image::bitmap_mask_tile(self.doc, bitmap) else { return false };
            let Ok(data) = png_data(&pixels, width, height, image::ExtendedColorType::Rgba8) else { return false };
            let world = kurbo::Affine::new(dtool_core::geometry::object_matrix(object).map(f64::from))
                * kurbo::Affine::translate((f64::from(object.pos[0]), f64::from(object.pos[1])));
            let transform = world * kurbo::Affine::new(bitmap.matrix);
            let fill_id = format!("dtool-transparency-tile-{id}");
            let defs = bitmap_pattern_data(&fill_id, width as usize, height as usize, transform, &data);
            let ink = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#{fill_id})\"/>", f(x), f(y), f(w), f(h));
            (defs, ink)
        } else if matches!(&object.transparency, Transparency::Gradient { kind: GradientKind::Conical, .. }) {
            if self.opts.rasterize == RasterizeMode::Nothing {
                warnings.push(format!("export.svg.transparency_unsupported:{id}"));
                return false;
            }
            let scale = self.opts.raster_dpi as f32 * crate::raster::document_unit_points(self.doc) / 72.0;
            let Some(([width, height], pixels)) = crate::effect_image::transparency_pixels(object, self.area, scale) else { return false };
            let Ok(data) = png_data(&pixels, width, height, image::ExtendedColorType::Rgba8) else { return false };
            (String::new(), format!("<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{data}\"/>", f(x), f(y), f(w), f(h)))
        } else if let Transparency::Gradient { kind, stops, geom } = &object.transparency {
            let fill_id = format!("dtool-transparency-gradient-{id}");
            let gradient = Paint::Gradient { kind: *kind, stops: stops.clone(), geom: *geom };
            let Some(definition) = gradient_def(&gradient, &fill_id, self.opts) else {
                warnings.push(format!("export.svg.transparency_unsupported:{id}"));
                return false;
            };
            let [x, y] = object.pos.map(f64::from);
            let world = kurbo::Affine::new(dtool_core::geometry::object_matrix(object).map(f64::from))
                * kurbo::Affine::translate((x, y));
            let matrix = world.as_coeffs().map(|v| v as f32);
            let Some(inverse) = dtool_core::geometry::invert_matrix(&matrix) else { return false };
            let [lx, ly, lw, lh] = dtool_core::scene::clip_bounds(self.area, &inverse);
            let transform = matrix.map(f).join(" ");
            (definition, format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" transform=\"matrix({transform})\" fill=\"url(#{fill_id})\"/>",
                f(lx), f(ly), f(lw), f(lh)))
        } else {
            return false;
        };
        let _ = writeln!(defs,
            "<mask id=\"dtool-transparency-{id}\" maskUnits=\"userSpaceOnUse\" style=\"mask-type:alpha\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\">{ink}</mask>\n<g mask=\"url(#dtool-transparency-{id})\">",
            f(x), f(y), f(w), f(h));
        content.push_str(&defs);
        true
    }
}

fn blend_style(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => "",
        BlendMode::Multiply => "mix-blend-mode:multiply",
        BlendMode::Screen => "mix-blend-mode:screen",
        BlendMode::Overlay => "mix-blend-mode:overlay",
        BlendMode::Darken => "mix-blend-mode:darken",
        BlendMode::Lighten => "mix-blend-mode:lighten",
        BlendMode::ColorDodge => "mix-blend-mode:color-dodge",
        BlendMode::ColorBurn => "mix-blend-mode:color-burn",
        BlendMode::HardLight => "mix-blend-mode:hard-light",
        BlendMode::SoftLight => "mix-blend-mode:soft-light",
        BlendMode::Difference => "mix-blend-mode:difference",
        BlendMode::Exclusion => "mix-blend-mode:exclusion",
        BlendMode::Hue => "mix-blend-mode:hue",
        BlendMode::Saturation => "mix-blend-mode:saturation",
        BlendMode::Color => "mix-blend-mode:color",
        BlendMode::Luminosity => "mix-blend-mode:luminosity",
    }
}

fn write_pixel_mask(content: &mut String, doc: &Document, group: &Object, opts: &SvgOptions) -> Result<bool, CommandError> {
    let Some(mask) = dtool_core::raster_source::compositing_mask(doc, group) else {
        return Ok(false);
    };
    let [w, h] = group.size.map(f32::abs);
    let matrix = dtool_core::geometry::object_matrix(group);
    if ![w, h].iter().all(|size| size.is_finite() && *size > 0.0)
        || dtool_core::geometry::invert_matrix(&matrix).is_none()
        || !group.pos.iter().all(|value| value.is_finite())
    {
        return Ok(false);
    }
    let data = png_data(&mask.coverage, mask.width, mask.height, image::ExtendedColorType::L8)?;
    let id = group.id.0;
    let f = |value| format_float(value, opts.precision.max(6));
    let [x, y] = group.pos.map(f);
    let [left, top, width, height] = dtool_core::geometry::object_world_bounds(group).map(f);
    let [a, b, c, d, e, g] = matrix.map(f);
    let [w, h] = [f(w), f(h)];
    let _ = writeln!(content,
        "<defs><mask id=\"dtool-pixel-mask-{id}\" maskUnits=\"userSpaceOnUse\" maskContentUnits=\"userSpaceOnUse\" x=\"{left}\" y=\"{top}\" width=\"{width}\" height=\"{height}\"><image x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" image-rendering=\"optimizeSpeed\" transform=\"matrix({a} {b} {c} {d} {e} {g})\" xlink:href=\"data:image/png;base64,{data}\"/></mask></defs>");
    Ok(true)
}

fn native_outer_shadow(object: &Object) -> Option<&LayerEffect> {
    let mut enabled = object.effects.iter().filter(|effect| effect.enabled);
    let effect = enabled.next()?;
    (enabled.next().is_none() && effect.kind == EffectKind::OuterShadow && effect.blend.is_normal()
        && effect.opacity > 0.0 && effect.band_pass.is_none() && effect.is_valid()
        && matches!(effect.params, EffectParams::Shadow { intensity: 0.0, knock_out: false, .. }))
        .then_some(effect)
}

fn open_drop_shadow(
    content: &mut String, id: u64, effect: &LayerEffect, bounds: [f32; 4], opts: &SvgOptions,
) -> &'static str {
    let EffectParams::Shadow { radius, radius_y, blur_angle, offset, angle, ref colour, .. } = effect.params
        else { unreachable!("native_outer_shadow checked the effect") };
    let [mut radius_x, mut radius_y] = [radius, radius_y.unwrap_or(radius)];
    let tilt = if radius_x == radius_y { 0.0 } else { blur_angle.unwrap_or_default().rem_euclid(180.0) };
    let tilt = if tilt == 90.0 {
        std::mem::swap(&mut radius_x, &mut radius_y);
        0.0
    } else {
        tilt
    };
    let (sin, cos) = angle.to_radians().sin_cos();
    let shift = kurbo::Vec2::new(f64::from(-cos * offset), f64::from(sin * offset));
    let local = (kurbo::Affine::rotate(-f64::from(tilt).to_radians()) * shift.to_point()).to_vec2();
    let precision = opts.precision.max(6);
    let [dx, dy, opacity] = [local.x as f32, local.y as f32, effect.opacity * f32::from(colour.a) / 255.0]
        .map(|value| format_float(value, precision));
    let sigma = filter_deviation([radius_x, radius_y], precision);
    let (r, g, b) = (colour.r, colour.g, colour.b);
    let filter_id = format!("dtool-shadow-{id}");
    open_filter_definition(content, &filter_id, bounds, tilt, precision);
    let _ = write!(content, "<feDropShadow dx=\"{dx}\" dy=\"{dy}\" stdDeviation=\"{sigma}\" flood-color=\"#{r:02x}{g:02x}{b:02x}\" flood-opacity=\"{opacity}\"/>");
    open_filter_group(content, &filter_id, tilt, precision)
}

fn filter_deviation(radii: [f32; 2], precision: u8) -> String {
    let [x, y] = radii.map(|radius| format_float(radius * 0.5, precision));
    if x == y { x } else { format!("{x} {y}") }
}

fn open_filter_definition(content: &mut String, id: &str, [x, y, width, height]: [f32; 4], angle: f32, precision: u8) {
    let [x, y, width, height] = if angle == 0.0 {
        [x, y, width, height]
    } else {
        let bounds = kurbo::Rect::from_origin_size((f64::from(x), f64::from(y)), (f64::from(width), f64::from(height)));
        let local = kurbo::Affine::rotate(-f64::from(angle).to_radians()).transform_rect_bbox(bounds);
        [local.x0 as f32, local.y0 as f32, local.width() as f32, local.height() as f32]
    };
    let [x, y, width, height] = [x, y, width, height].map(|v| format_float(v, precision));
    let _ = write!(content,
        "<defs><filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" color-interpolation-filters=\"sRGB\">");
}

fn open_filter_group(content: &mut String, id: &str, angle: f32, precision: u8) -> &'static str {
    content.push_str("</filter></defs>\n");
    if angle == 0.0 {
        let _ = writeln!(content, "<g filter=\"url(#{id})\">");
        "</g>\n"
    } else {
        let angle = format_float(angle, precision);
        // Rotate the filter's coordinate space, then compensate for its source.
        let _ = writeln!(content, "<g transform=\"rotate({angle})\"><g filter=\"url(#{id})\"><g transform=\"rotate(-{angle})\">");
        "</g>\n</g>\n</g>\n"
    }
}

fn open_gaussian_blur(
    content: &mut String, id: u64, blur: dtool_core::layer_style::raster::GaussianBlur,
    bounds: [f32; 4], opts: &SvgOptions,
) -> &'static str {
    let precision = opts.precision.max(6);
    let sigma = filter_deviation(blur.radii, precision);
    let filter_id = format!("dtool-blur-{id}");
    open_filter_definition(content, &filter_id, bounds, blur.angle, precision);
    let _ = write!(content, "<feGaussianBlur stdDeviation=\"{sigma}\"/>");
    if blur.preserve_alpha {
        content.push_str("<feComponentTransfer><feFuncA type=\"linear\" slope=\"0\" intercept=\"1\"/></feComponentTransfer><feComposite in2=\"SourceAlpha\" operator=\"in\"/>");
    }
    open_filter_group(content, &filter_id, blur.angle, precision)
}

fn png_data(pixels: &[u8], width: u32, height: u32, colour: image::ExtendedColorType) -> Result<String, CommandError> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(pixels, width, height, colour)
        .map_err(crate::export_io_error)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}

fn write_effect_images(
    content: &mut String, doc: &Document, bounds: [f32; 4], layers: &[EffectLayer], opts: &SvgOptions,
) -> Result<(), CommandError> {
    let scale = crate::raster::document_unit_points(doc) * opts.raster_dpi.max(1) as f32 / 72.0;
    let Some([w, h]) = crate::effect_image::size(bounds, layers, scale) else { return Ok(()) };
    for layer in layers {
        let Some(raster) = crate::effect_image::rgba(doc, bounds, [w, h], layer) else { continue };
        write_effect_image(content, raster, layer.blend, opts)?;
    }
    Ok(())
}

fn write_effect_image(
    content: &mut String, raster: crate::effect_image::EffectImage, blend: BlendMode, opts: &SvgOptions,
) -> Result<(), CommandError> {
    let [x, y, width, height] = raster.bounds.map(|value| format_float(value, opts.precision.max(6)));
    let data = png_data(&raster.pixels, raster.size[0], raster.size[1], image::ExtendedColorType::Rgba8)?;
    let _ = write!(content, "<image x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"none\"");
    let blend = blend_style(blend);
    if !blend.is_empty() {
        let _ = write!(content, " style=\"{blend}\"");
    }
    let _ = writeln!(content, " xlink:href=\"data:image/png;base64,{data}\"/>");
    Ok(())
}

fn path_brush_image(
    doc: &Document, object: &Object, ink: &dtool_core::path_brush::Ink, opts: &SvgOptions,
) -> Result<String, CommandError> {
    let scale = crate::raster::document_unit_points(doc) * opts.raster_dpi.max(1) as f32 / 72.0;
    let [width, height] = ink.pixel_size(scale);
    let data = png_data(&ink.render([width, height]), width as u32, height as u32, image::ExtendedColorType::Rgba8)?;
    let [x, y, width, height] = ink.bounds.map(|value| format_float(value, opts.precision.max(6)));
    let opacity = format_float(object.opacity.clamp(0.0, 1.0), 6);
    Ok(format!(
        "<image x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" opacity=\"{opacity}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/png;base64,{data}\"/>\n"
    ))
}

fn write_raster_layer(content: &mut String, doc: &Document, object: &Object, opts: &SvgOptions) -> Result<bool, CommandError> {
    use dtool_core::geometry::{apply_matrix, object_matrix, object_world_quad};
    use dtool_core::paint::Paint;

    let Some(([width, height], pixels)) = dtool_core::raster_source::rgba(doc, object) else {
        return Ok(false);
    };
    let id = object.id.0;
    let matrix = object_matrix(object);
    let frame = dtool_core::commands::picture_frame::frame_geometry(doc, object);
    let [a, b, _, d] = frame
        .map(|geometry| geometry.content_corners().map(|point| apply_matrix(&matrix, point)))
        .unwrap_or_else(|| object_world_quad(object));
    let transform = [b[0] - a[0], b[1] - a[1], d[0] - a[0], d[1] - a[1], a[0], a[1]];
    if dtool_core::geometry::invert_matrix(&transform).is_none() {
        return Ok(false);
    }
    let data = png_data(&pixels, width as u32, height as u32, image::ExtendedColorType::Rgba8)?;
    let outline = frame.map(|geometry| geometry.outline().into_iter().map(|point| apply_matrix(&matrix, point)).collect())
        .unwrap_or_else(|| object_world_quad(object).to_vec());
    let outline_path = contour_path(&[outline], opts.precision.max(6));
    let paint_path = shape_path_data(object, opts.precision).unwrap_or_else(|| outline_path.clone());
    let opacity = format_float(object.opacity.clamp(0.0, 1.0), 6);
    let stroke_under = matches!(object.paint_order, dtool_core::paint::PaintOrder::StrokeUnder);
    let write_outline = |out: &mut String| {
        if !matches!(object.stroke.paint, Paint::None) && object.stroke.width > 0.0 {
            let _ = write!(out, "<path d=\"{paint_path}\" fill=\"none\" opacity=\"{opacity}\" ");
            write_stroke(out, &object.stroke, opts);
            out.push_str("/>\n");
        }
    };
    let _ = writeln!(content, "<g id=\"dtool-image-{id}\">");
    if stroke_under {
        write_outline(content);
    }
    if !matches!(object.fill, Paint::None) {
        let fill_id = format!("dtool-paint-{id}");
        let gradient = gradient_def(&object.fill, &fill_id, opts);
        if let Some(definition) = &gradient {
            content.push_str(definition);
        }
        let _ = write!(content, "<path d=\"{paint_path}\" opacity=\"{opacity}\" ");
        write_fill(content, &object.fill, opts, gradient.as_ref().map(|_| fill_id.as_str()));
        content.push_str("/>\n");
    }
    if frame.is_some() {
        let _ = writeln!(content, "<clipPath id=\"dtool-image-clip-{id}\"><path d=\"{outline_path}\"/></clipPath>\n<g clip-path=\"url(#dtool-image-clip-{id})\">");
    }
    let [a, b, c, d, x, y] = transform.map(|value| format_float(value, opts.precision.max(6)));
    let _ = writeln!(content,
        "<image width=\"1\" height=\"1\" opacity=\"{opacity}\" preserveAspectRatio=\"none\" image-rendering=\"optimizeSpeed\" transform=\"matrix({a} {b} {c} {d} {x} {y})\" xlink:href=\"data:image/png;base64,{data}\"/>");
    if frame.is_some() {
        content.push_str("</g>\n");
    }
    if !stroke_under {
        write_outline(content);
    }
    content.push_str("</g>\n");
    Ok(true)
}

/// K08: clip chain only -> `<clipPath>`; any vector mask in the chain -> `<mask>` (white path, even-odd).
/// Returns false (nothing written) when the object is clipped away completely.
fn write_k08_open(content: &mut String, doc: &Document, object: &dtool_core::Object, rings: &[Vec<[f64; 2]>], opts: &SvgOptions) -> bool {
    let rings: Vec<&Vec<[f64; 2]>> = rings.iter().filter(|r| r.len() >= 3).collect();
    if rings.is_empty() {
        return false;
    }
    let f = |v: f64| format_float(v as f32, opts.precision);
    let mut d = String::new();
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for ring in &rings {
        for (i, p) in ring.iter().enumerate() {
            d.push_str(&format!("{}{} {} ", if i == 0 { "M" } else { "L" }, f(p[0]), f(p[1])));
            b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
        }
        d.push_str("Z ");
    }
    let d = d.trim_end();
    let id = object.id.0;
    let mut masked = false;
    let mut current = Some(object);
    for _ in 0..64 {
        let Some(o) = current else { break };
        if !dtool_core::clip_mask::vector_masks(doc, o.id).is_empty() {
            masked = true;
            break;
        }
        current = o.parent_id.and_then(|p| doc.get_object(p));
    }
    if masked {
        content.push_str(&format!(
            "<mask id=\"k08-mask-{id}\" maskUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><path d=\"{d}\" fill=\"#ffffff\" fill-rule=\"evenodd\"/></mask>\n<g mask=\"url(#k08-mask-{id})\">\n",
            f(b[0]), f(b[1]), f((b[2] - b[0]).max(0.0)), f((b[3] - b[1]).max(0.0))
        ));
    } else {
        content.push_str(&format!("<clipPath id=\"k08-clip-{id}\"><path d=\"{d}\" clip-rule=\"evenodd\"/></clipPath>\n<g clip-path=\"url(#k08-clip-{id})\">\n"));
    }
    true
}

fn write_svg_header(content: &mut String, width: f32, height: f32, unit_points: f32, output_size: Option<(u32, u32)>, page_scoped: bool, opts: &SvgOptions) {
    let size = |value: f32| {
        if f64::from(value) * 10.0_f64.powi(i32::from(opts.precision)) < 0.5 {
            value.to_string()
        } else {
            format_float(value, opts.precision)
        }
    };
    content.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    content.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" ");
    if page_scoped {
        content.push_str("overflow=\"hidden\" ");
    }
    
    if opts.set_viewbox {
        content.push_str(&format!(
            "viewBox=\"0 0 {} {}\" ",
            size(width),
            size(height)
        ));
        if output_size.is_some() {
            content.push_str("preserveAspectRatio=\"none\" ");
        }
    }
    if let Some((width, height)) = output_size {
        let _ = writeln!(content, "width=\"{width}\" height=\"{height}\">");
        return;
    }
    content.push_str(&format!(
        "width=\"{}pt\" height=\"{}pt\">\n",
        size(width * unit_points),
        size(height * unit_points)
    ));
}

fn finish_svg(mut content: String, line_breaks: bool) -> Vec<u8> {
    content.push_str("</svg>\n");
    if !line_breaks {
        // User text escapes line breaks as character references. Only strip
        // formatting newlines emitted after tags by the SVG writer.
        let mut after_tag = false;
        content.retain(|ch| {
            let keep = ch != '\n' || !after_tag;
            after_tag = ch == '>';
            keep
        });
    }
    content.into_bytes()
}

fn write_layout_text(
    content: &mut String,
    doc: &Document,
    object: &dtool_core::Object,
    id: &str,
    opts: &SvgOptions,
    tolerance: f64,
) -> bool {
    use std::fmt::Write;
    if dtool_core::scene::has_live_text_warp(doc, object.id) {
        dtool_text::canvas_text::install();
        let lines = dtool_core::text_lines::text_lines(doc, object.id);
        if let Some(ink) = dtool_core::scene::warped_text_ink_with_tolerance(doc, object, &lines, object.opacity, tolerance) {
            let _ = writeln!(content, "<g id=\"{id}\" opacity=\"{}\">", format_float(object.opacity, 3));
            for (index, prim) in ink.into_iter().enumerate() {
                let dtool_core::scene::Prim::Fill { contours, paint, .. } = prim else { continue };
                let path = contour_path(&contours, opts.precision);
                let fill_id = format!("dtool-warp-text-{}-{index}", object.id.0);
                let gradient = gradient_def(&paint, &fill_id, opts);
                if let Some(definition) = gradient.as_deref() {
                    content.push_str(definition);
                }
                let _ = write!(content, "<path d=\"{path}\" fill-rule=\"evenodd\" ");
                write_fill(content, &paint, opts, gradient.as_ref().map(|_| fill_id.as_str()));
                content.push_str("/>\n");
            }
            content.push_str("</g>\n");
            return true;
        }
    }
    let library = dtool_text::FontLibrary::shared();
    let Some(layout) = dtool_text::canvas_text::text_export_layout(&library, doc, object, opts.text_as_curves) else {
        return false;
    };
    let fill_id = format!("dtool-tab-text-{}", object.id.0);
    let gradient = gradient_def(&layout.paint, &fill_id, opts);
    if let Some(definition) = gradient.as_deref() {
        content.push_str(definition);
    }
    let f = |value| format_float(value, opts.precision);
    let _ = write!(content, "<g id=\"{id}\" opacity=\"{}\" ", format_float(object.opacity, 3));
    write_fill(content, &layout.paint, opts, gradient.as_ref().map(|_| fill_id.as_str()));
    if layout.matrix != dtool_core::geometry::IDENTITY {
        let [a, b, c, d, x, y] = layout.matrix.map(f);
        let _ = write!(content, " transform=\"matrix({a} {b} {c} {d} {x} {y})\"");
    }
    content.push_str(">\n");
    let decorations = dtool_core::text_lines::text_decorations(doc, object.id);
    let draw_decorations = |content: &mut String, under| {
        for (index, decoration) in decorations.iter().enumerate().filter(|(_, item)| item.under_text == under) {
            let [x, y, w, h] = decoration.rect.map(f);
            let fill_id = format!("{fill_id}-decoration-{index}");
            let gradient = gradient_def(&decoration.paint, &fill_id, opts);
            if let Some(definition) = gradient.as_deref() {
                content.push_str(definition);
            }
            let _ = write!(content, "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" ");
            write_fill(content, &decoration.paint, opts, gradient.as_ref().map(|_| fill_id.as_str()));
            content.push_str("/>\n");
        }
    };
    draw_decorations(content, true);
    if let Some(ink) = layout.outline_ink {
        for (index, run) in ink.into_iter().enumerate() {
            let path = contour_path(&run.contours, opts.precision);
            if path.is_empty() { continue }
            let fill_id = format!("{fill_id}-outline-{index}");
            let gradient = gradient_def(&run.paint, &fill_id, opts);
            if let Some(definition) = gradient.as_deref() { content.push_str(definition); }
            let _ = write!(content, "<path d=\"{path}\" fill-rule=\"evenodd\" ");
            write_fill(content, &run.paint, opts, gradient.as_ref().map(|_| fill_id.as_str()));
            content.push_str("/>\n");
        }
    } else {
        let path = contour_path(&layout.contours, opts.precision);
        if !path.is_empty() {
            let _ = writeln!(content, "<path d=\"{path}\" fill-rule=\"evenodd\"/>");
        }
    }
    if !layout.spans.is_empty() {
        let span_gradients: Vec<_> = layout.spans.iter().enumerate().map(|(index, span)| {
            let paint = span.style.fill.as_ref().filter(|paint| *paint != &layout.paint)?;
            let id = format!("{fill_id}-span-{index}");
            let definition = gradient_def(paint, &id, opts)?;
            content.push_str(&definition);
            Some(id)
        }).collect();
        content.push_str("<text xml:space=\"preserve\">");
        for (span, gradient) in layout.spans.into_iter().zip(span_gradients) {
            let font_family = library.face(span.font).map(|face| face.family.as_str()).unwrap_or(&span.style.family);
            let _ = write!(content, "<tspan x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" font-weight=\"{}\"",
                f(span.origin[0]), f(span.origin[1]), escape_xml(font_family), f(span.style.size_pt), span.style.weight);
            if span.style.italic {
                content.push_str(" font-style=\"italic\"");
            }
            if span.width > 0.0 {
                let _ = write!(content, " textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"", f(span.width));
            }
            if let Some(paint) = span.style.fill.as_ref().filter(|paint| *paint != &layout.paint) {
                let _ = write!(content, " ");
                write_fill(content, paint, opts, gradient.as_deref());
            }
            let _ = write!(content, ">{}</tspan>", escape_xml(&span.text));
        }
        content.push_str("</text>\n");
    }
    draw_decorations(content, false);
    content.push_str("</g>\n");
    true
}

fn contour_path(contours: &[Vec<[f32; 2]>], precision: u8) -> String {
    use std::fmt::Write;
    let f = |value| format_float(value, precision);
    let mut path = String::new();
    for contour in contours.iter().filter(|contour| contour.len() >= 3) {
        let [x, y] = contour[0];
        let _ = write!(path, "M{} {}", f(x), f(y));
        for &[x, y] in &contour[1..] {
            let _ = write!(path, "L{} {}", f(x), f(y));
        }
        path.push('Z');
    }
    path
}

fn write_table(content: &mut String, doc: &Document, object: &dtool_core::Object, opts: &SvgOptions) {
    use std::fmt::Write;
    let _ = writeln!(content, "<g id=\"dtool-table-{}\">", object.id.0);
    for (index, prim) in dtool_render::scene::table_prims(doc, object).into_iter().enumerate() {
        let dtool_core::scene::Prim::Fill { contours, paint, .. } = prim else { continue };
        let path = contour_path(&contours, opts.precision);
        if path.is_empty() { continue; }
        let gradient_id = format!("dtool-table-{}-{index}", object.id.0);
        let defs = gradient_def(&paint, &gradient_id, opts);
        if let Some(definition) = &defs { content.push_str(definition); }
        let _ = write!(content, "<path d=\"{path}\" fill-rule=\"evenodd\" opacity=\"{}\" ", format_float(object.opacity, 3));
        write_fill(content, &paint, opts, defs.as_ref().map(|_| gradient_id.as_str()));
        content.push_str("/>\n");
    }
    content.push_str("</g>\n");
}

fn css_appearance_blend(mode: dtool_core::blend::BlendMode) -> Option<&'static str> {
    use dtool_core::blend::BlendMode::*;
    Some(match mode {
        Normal | PassThrough => return None,
        Multiply => "multiply", Screen => "screen", Overlay => "overlay",
        Darken => "darken", Lighten => "lighten", ColorDodge => "color-dodge",
        ColorBurn => "color-burn", HardLight => "hard-light", SoftLight => "soft-light",
        Difference => "difference", Exclusion => "exclusion", Hue => "hue",
        Saturation => "saturation", Color => "color", Luminosity => "luminosity",
    })
}

fn write_appearance_path(
    content: &mut String,
    doc: &Document,
    object: dtool_core::ObjectId,
    index: usize,
    path: &dtool_core::commands::appearance::AppearancePath,
    opts: &SvgOptions,
    warnings: &mut Vec<String>,
    opacity: f32,
    filter: Option<&str>,
) {
    use dtool_core::commands::appearance::RowBlend;
    let gradient_id = format!("dtool-appearance-{}-{index}", object.0);
    let gradient = gradient_def(&path.paint, &gradient_id, opts)
        .or_else(|| bitmap_pattern_def(doc, &path.paint, kurbo::Affine::IDENTITY, &gradient_id))
        .or_else(|| conical_pattern_def(doc, path, &gradient_id, opts));
    if let Some(defs) = gradient.as_deref() {
        content.push_str(defs);
    } else if paint_is_flattened(&path.paint) {
        warnings.push(format!("export.{}.paint_flattened:{}", SVG_WARN_SCOPE, object.0));
    }
    let rule = if path.even_odd { "evenodd" } else { "nonzero" };
    let _ = write!(content, "<path d=\"{}\" fill-rule=\"{rule}\" opacity=\"{}\" ", bezier_path_data(&path.path, opts.precision), format_float(opacity, 4));
    if let RowBlend::Standard(mode) = path.blend
        && let Some(mode) = css_appearance_blend(mode)
    {
        let _ = write!(content, "style=\"mix-blend-mode:{mode}\" ");
    }
    if let Some(filter) = filter {
        let _ = write!(content, "filter=\"url(#{filter})\" ");
    }
    write_fill(content, &path.paint, opts, gradient.as_ref().map(|_| gradient_id.as_str()));
    content.push_str("/>\n");
}

fn write_object(content: &mut String, doc: &Document, object: &dtool_core::Object, id: &str, opts: &SvgOptions, warnings: &mut Vec<String>, tolerance: f64) {
    use dtool_core::paint::{GradientKind, Paint, PaintOrder, StrokeAlign};
    let conical = opts.rasterize != RasterizeMode::Nothing && opts.embed_images
        && [&object.fill, &object.stroke.paint].into_iter()
            .any(|paint| matches!(paint, Paint::Gradient { kind: GradientKind::Conical, .. }));
    let styled_stroke = object.stroke.width > 0.0 && object.stroke.paint != Paint::None
        && (!object.stroke.pressure.is_empty() || object.stroke.align != StrokeAlign::Centre
            || object.paint_order == PaintOrder::StrokeUnder);
    if (dtool_core::commands::appearance::has_layers(object) || object.stroke.cap == dtool_core::paint::Cap::RoundInset
        || styled_stroke
        || conical
        || matches!(object.stroke.paint, dtool_core::paint::Paint::Bitmap(_)
            | dtool_core::paint::Paint::Gradient { kind: dtool_core::paint::GradientKind::Linear
                | dtool_core::paint::GradientKind::Radial | dtool_core::paint::GradientKind::Elliptical, .. }))
        && let Some(paths) = dtool_core::commands::appearance::vector_paths_with_tolerance(doc, object, &object.fill, false, tolerance)
    {
        let isolated = object.opacity < 1.0 || paths.iter().any(|path| !path.blend.is_normal());
        let opacity = if isolated { 1.0 } else { object.opacity };
        if isolated {
            let _ = writeln!(content, "<g id=\"{id}\" style=\"isolation:isolate\" opacity=\"{}\">", format_float(object.opacity, 4));
        } else {
            let _ = writeln!(content, "<g id=\"{id}\">");
        }
        // Mask the earlier rows in nested groups so the following rows can paint above each eraser.
        let mut masks = Vec::new();
        if paths.iter().any(|path| path.blend.erases()) {
            let [x, y, right, bottom] = crate::vector_object_bounds(doc, object);
            let rect = format!("x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\"", right - x, bottom - y);
            let blacken = format!("dtool-erase-black-{}", object.id.0);
            let _ = writeln!(content, "<defs><filter id=\"{blacken}\" filterUnits=\"userSpaceOnUse\" {rect}><feColorMatrix type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0\"/></filter></defs>");
            for (index, path) in paths.iter().enumerate().rev().filter(|(_, path)| path.blend.erases()) {
                let mask_id = format!("dtool-erase-{}-{index}", object.id.0);
                let _ = writeln!(content, "<defs><mask id=\"{mask_id}\" maskUnits=\"userSpaceOnUse\" {rect} style=\"mask-type:luminance\"><rect {rect} fill=\"white\"/>");
                write_appearance_path(content, doc, object.id, index, path, opts, warnings, 1.0, Some(&blacken));
                let _ = writeln!(content, "</mask></defs><g mask=\"url(#{mask_id})\">");
                masks.push(index);
            }
        }
        for (index, path) in paths.iter().enumerate() {
            if masks.last() == Some(&index) {
                masks.pop();
                content.push_str("</g>\n");
            } else {
                write_appearance_path(content, doc, object.id, index, path, opts, warnings, opacity, None);
            }
        }
        content.push_str("</g>\n");
        return;
    }
    let gradient_id = format!("dtool-paint-{}", object.id.0);
    let [x, y] = object.pos.map(f64::from);
    let transform = kurbo::Affine::new(dtool_core::geometry::object_matrix(object).map(f64::from))
        * kurbo::Affine::translate((x, y));
    let gradient = if let dtool_core::paint::Paint::Gradient { kind, stops, geom } = &object.fill {
        let paint = dtool_core::paint::Paint::Gradient { kind: *kind, stops: stops.clone(), geom: geom.transform(transform.as_coeffs()) };
        gradient_def(&paint, &gradient_id, opts)
    } else {
        bitmap_pattern_def(doc, &object.fill, transform, &gradient_id)
    };
    let fill_ref = gradient.as_ref().map(|_| gradient_id.as_str());
    if gradient.is_none() && paint_is_flattened(&object.fill) {
        warnings.push(format!("export.{}.paint_flattened:{}", SVG_WARN_SCOPE, object.id.0));
    }
    if paint_is_flattened(&object.stroke.paint) {
        warnings.push(format!("export.{}.stroke_paint_flattened:{}", SVG_WARN_SCOPE, object.id.0));
    }
    
    let open_spiral = object.shape.as_ref().is_some_and(|shape| shape.kind == dtool_core::shape::ShapeKind::Spiral);
    let uses_path = matches!(object.kind, dtool_core::ObjectKind::Curve(_) | dtool_core::ObjectKind::Compound(_)) || open_spiral;
    let path = (uses_path || object.parent_id.is_some())
        .then(|| dtool_core::outline::object_outline_world_with_tolerance(object, tolerance)).flatten();
    let warped = path.as_ref().and_then(|path| {
        dtool_core::commands::warp::live_warp_outline_with_tolerance(doc, object.id, path, tolerance)
    });
    match &object.kind {
        dtool_core::ObjectKind::Table(_) => write_table(content, doc, object, opts),
        dtool_core::ObjectKind::Shape if warped.is_none() && !open_spiral => {
            content.push_str(&format!("<g id=\"{}\">\n", id));
            
            if let Some(path_data) = shape_path_data(object, opts.precision) {
                if let Some(defs) = gradient.as_deref() {
                    content.push_str(defs);
                }
                let rule = if object.fill_rule.is_even_odd() { "evenodd" } else { "nonzero" };
                content.push_str(&format!("<path d=\"{path_data}\" fill-rule=\"{rule}\" "));
                
                match &object.fill {
                    dtool_core::paint::Paint::None => content.push_str("fill=\"none\" "),
                    fill => write_fill(content, fill, opts, fill_ref),
                }
                
                if !matches!(object.stroke.paint, dtool_core::paint::Paint::None) {
                    write_stroke(content, &object.stroke, opts);
                }
                
                if object.opacity < 1.0 {
                    content.push_str(&format!("opacity=\"{}\" ", format_float(object.opacity, 2)));
                }
                
                content.push_str("/>\n");
            }
            
            content.push_str("</g>\n");
        }
        _ if uses_path || warped.is_some() => {
            let Some(path) = warped.or(path) else { return; };
            let (path, fill_path) = crate::vector_curve_paths(path);
            let _ = writeln!(content, "<g id=\"{id}\" opacity=\"{}\">", format_float(object.opacity, 4));
            if !fill_path.elements().is_empty() {
                if let Some(defs) = gradient.as_deref() {
                    content.push_str(defs);
                }
                let rule = if object.fill_rule.is_even_odd() { "evenodd" } else { "nonzero" };
                let _ = write!(content, "<path d=\"{}\" fill-rule=\"{rule}\" ", bezier_path_data(&fill_path, opts.precision));
                write_fill(content, &object.fill, opts, fill_ref);
                content.push_str("/>\n");
            }
            if crate::stroke_colour(&object.stroke).is_some() {
                let _ = write!(content, "<path d=\"{}\" fill=\"none\" ", bezier_path_data(&path, opts.precision));
                write_stroke(content, &object.stroke, opts);
                content.push_str("/>\n");
                let mut arrow_stroke = object.stroke.clone();
                arrow_stroke.dash.clear();
                for head in crate::arrowheads_for(Some(doc), object) {
                    let mut path = kurbo::BezPath::new();
                    for (index, [x, y]) in head.points.into_iter().enumerate() {
                        let point = (f64::from(x), f64::from(y));
                        if index == 0 { path.move_to(point); } else { path.line_to(point); }
                    }
                    if head.filled {
                        path.close_path();
                        let _ = write!(content, "<path d=\"{}\" ", bezier_path_data(&path, opts.precision));
                        write_fill(content, &object.stroke.paint, opts, None);
                    } else {
                        let _ = write!(content, "<path d=\"{}\" fill=\"none\" ", bezier_path_data(&path, opts.precision));
                        arrow_stroke.width = head.line_width;
                        write_stroke(content, &arrow_stroke, opts);
                    }
                    content.push_str("/>\n");
                }
            }
            content.push_str("</g>\n");
        }
        dtool_core::ObjectKind::Text(_)
        | dtool_core::ObjectKind::ArtText { .. }
        | dtool_core::ObjectKind::FrameText { .. }
        | dtool_core::ObjectKind::PathText(_) => {
            let container = crate::text_container(object).and_then(|container| shape_path_data(&container, opts.precision));
            let has_fill = container.is_some() && !matches!(object.fill, dtool_core::paint::Paint::None);
            let stroke_under = matches!(object.paint_order, dtool_core::paint::PaintOrder::StrokeUnder);
            let write_container_stroke = |content: &mut String| {
                if let Some(path) = &container && object.stroke.width > 0.0
                    && !matches!(object.stroke.paint, dtool_core::paint::Paint::None)
                {
                    let _ = write!(content, "<path d=\"{path}\" fill=\"none\" opacity=\"{}\" ", format_float(object.opacity, 4));
                    write_stroke(content, &object.stroke, opts);
                    content.push_str("/>\n");
                }
            };
            if stroke_under {
                write_container_stroke(content);
            }
            if let Some(path) = &container && has_fill {
                if let Some(defs) = gradient.as_deref() {
                    content.push_str(defs);
                }
                let _ = write!(content, "<path d=\"{path}\" opacity=\"{}\" ", format_float(object.opacity, 4));
                write_fill(content, &object.fill, opts, fill_ref);
                content.push_str("/>\n");
            }
            if write_layout_text(content, doc, object, id, opts, tolerance) {
                if !stroke_under {
                    write_container_stroke(content);
                }
                return;
            }
            if opts.text_as_curves && object.is_text_frame() {
                warnings.push(format!("export.{}.text_kept_as_text:{}", SVG_WARN_SCOPE, object.id.0));
            }
            if let Some(text_content) = doc.object_text(object) {
                if !has_fill && let Some(defs) = gradient.as_deref() {
                    content.push_str(defs);
                }
                let font_size = object.font_size();
                let baseline = object.y() + font_size.unwrap_or(0.0);
                let x = format_float(object.x(), opts.precision);
                let y = format_float(baseline, opts.precision);
                
                content.push_str(&format!("<text id=\"{}\" x=\"{}\" y=\"{}\" xml:space=\"preserve\"", id, x, y));
                
                if let Some(font_size) = font_size {
                    content.push_str(&format!(" font-size=\"{}\"", format_float(font_size, opts.precision)));
                }
                
                content.push(' ');
                if has_fill || matches!(object.fill, dtool_core::paint::Paint::None) {
                    content.push_str("fill=\"#000000\"");
                } else {
                    write_fill(content, &object.fill, opts, fill_ref);
                }
                
                if object.opacity < 1.0 {
                    content.push_str(&format!(" opacity=\"{}\"", format_float(object.opacity, 2)));
                }
                
                content.push_str(">");
                if text_content.contains('\n') {
                    let line_height = font_size.unwrap_or(12.0) * 1.2;
                    for (index, line) in text_content.lines().enumerate() {
                        let line_y = format_float(baseline + line_height * index as f32, opts.precision);
                        let _ = write!(content, "<tspan x=\"{x}\" y=\"{line_y}\">{}</tspan>", escape_xml(line));
                    }
                } else {
                    content.push_str(&escape_xml(&text_content));
                }
                content.push_str("</text>\n");
            }
            if !stroke_under {
                write_container_stroke(content);
            }
        }
        dtool_core::ObjectKind::Dimension(_) => {
            if let Some(geometry) = dtool_core::outline::dimension_geometry(object) {
                let colour = crate::stroke_colour(&object.stroke)
                    .or_else(|| crate::paint_colour(&object.fill))
                    .unwrap_or(dtool_core::paint::Colour::new(0, 0, 0, 255));
                let colour = svg_colour(&colour, opts.hex_colours);
                let width = if object.stroke.width.is_finite() && object.stroke.width > 0.0 { object.stroke.width } else { 1.0 };
                let p = opts.precision;
                let mut segments = vec![geometry.line];
                segments.extend(geometry.extensions.iter().copied());
                segments.extend(geometry.ticks.iter().copied());
                let data: Vec<String> = segments
                    .iter()
                    .map(|[a, b]| format!("M{} {} L{} {}", format_float(a[0] as f32, p), format_float(a[1] as f32, p), format_float(b[0] as f32, p), format_float(b[1] as f32, p)))
                    .collect();
                content.push_str(&format!(
                    "<g id=\"{}\">\n<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>\n",
                    id,
                    data.join(" "),
                    colour,
                    format_float(width, p)
                ));
                let x = format_float(geometry.label_at[0] as f32, p);
                let y = format_float(geometry.label_at[1] as f32, p);
                content.push_str(&format!(
                    "<text x=\"{x}\" y=\"{y}\" font-size=\"{}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{colour}\" transform=\"rotate({} {x} {y})\">{}</text>\n</g>\n",
                    format_float(dtool_core::outline::DIMENSION_LABEL_SIZE as f32, p),
                    format_float(geometry.label_angle_deg as f32, p),
                    escape_xml(&geometry.label)
                ));
            }
        }
        _ => {}
    }
}

fn paint_is_flattened(paint: &dtool_core::paint::Paint) -> bool {
    !matches!(paint, dtool_core::paint::Paint::None | dtool_core::paint::Paint::Solid { .. })
}

fn gradient_def(fill: &dtool_core::paint::Paint, gradient_id: &str, opts: &SvgOptions) -> Option<String> {
    use dtool_core::paint::{GradientKind, Paint};
    let Paint::Gradient { kind, stops, geom } = fill else {
        return None;
    };
    if stops.is_empty() || geom.start.iter().chain(geom.end.iter()).any(|v| !v.is_finite()) {
        return None;
    }
    let p = opts.precision;
    let f = |value: f64| format_float(value, p);
    let spread = if geom.spread.is_pad() {
        String::new()
    } else {
        format!(" spreadMethod=\"{}\"", geom.spread.as_str())
    };
    let dx = geom.end[0] - geom.start[0];
    let dy = geom.end[1] - geom.start[1];
    let (open, close) = match kind {
        GradientKind::Linear => (
            format!(
                "<linearGradient id=\"{}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{spread}>",
                gradient_id,
                f(geom.start[0]),
                f(geom.start[1]),
                f(geom.end[0]),
                f(geom.end[1])
            ),
            "</linearGradient>",
        ),
        GradientKind::Radial | GradientKind::Elliptical => {
            let radius = dx.hypot(dy);
            if radius <= 1e-14 {
                return None;
            }
            let mut open = format!(
                "<radialGradient id=\"{}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\"",
                gradient_id,
                f(geom.start[0]),
                f(geom.start[1]),
                f(radius)
            );
            let ratio = geom.ratio;
            if matches!(kind, GradientKind::Elliptical) && ratio.is_finite() && ratio.abs() > 1e-14 && (ratio - 1.0).abs() > 1e-9 {
                open.push_str(&format!(
                    " gradientTransform=\"translate({} {}) rotate({}) scale(1 {}) rotate({}) translate({} {})\"",
                    f(geom.start[0]),
                    f(geom.start[1]),
                    f(dy.atan2(dx).to_degrees()),
                    f(ratio),
                    f(-dy.atan2(dx).to_degrees()),
                    f(-geom.start[0]),
                    f(-geom.start[1])
                ));
            }
            if let Some([fx, fy]) = geom.focus {
                let [x, y] = geom.start;
                let _ = write!(open, " fx=\"{}\" fy=\"{}\"", f(x + fx * dx - fy * dy), f(y + fx * dy + fy * dx));
            }
            if geom.focus_radius > 0.0 {
                let _ = write!(open, " fr=\"{}\"", f(geom.focus_radius * dx.hypot(dy)));
            }
            open.push_str(&spread);
            open.push('>');
            (open, "</radialGradient>")
        }
        GradientKind::Conical => return None,
    };
    let mut out = format!("<defs>{}", open);
    for stop in stops {
        out.push_str(&format!(
            "<stop offset=\"{}\" stop-color=\"{}\"",
            format_float(stop.offset.clamp(0.0, 1.0), 4),
            svg_colour(&stop.colour, opts.hex_colours)
        ));
        if stop.colour.a < 255 {
            out.push_str(&format!(" stop-opacity=\"{}\"", format_float(f32::from(stop.colour.a) / 255.0, 2)));
        }
        out.push_str("/>");
    }
    out.push_str(close);
    out.push_str("</defs>\n");
    Some(out)
}

fn bitmap_pattern_def(doc: &Document, paint: &dtool_core::paint::Paint, transform: kurbo::Affine, id: &str) -> Option<String> {
    let dtool_core::paint::Paint::Bitmap(bitmap) = paint else { return None };
    let ([width, height], pixels) = dtool_core::scene::bitmap_source(doc, &bitmap.resource_id)?;
    let data = png_data(&pixels, width as u32, height as u32, image::ExtendedColorType::Rgba8).ok()?;
    let transform = transform * kurbo::Affine::new(bitmap.matrix);
    Some(bitmap_pattern_data(id, width, height, transform, &data))
}

fn conical_pattern_def(
    doc: &Document,
    appearance: &dtool_core::commands::appearance::AppearancePath,
    id: &str,
    opts: &SvgOptions,
) -> Option<String> {
    use dtool_core::paint::{GradientKind, Paint, gradient_parameter, interpolate_stops};
    use kurbo::Shape;

    let Paint::Gradient { kind: GradientKind::Conical, stops, geom } = &appearance.paint else { return None };
    if opts.rasterize == RasterizeMode::Nothing || !opts.embed_images || stops.is_empty() {
        return None;
    }
    let bounds = appearance.path.bounding_box();
    let (w, h) = (bounds.width(), bounds.height());
    if !bounds.is_finite() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let scale = crate::raster::document_unit_points(doc) * opts.raster_dpi.max(1) as f32 / 72.0;
    let [width, height] = dtool_core::path_brush::pixel_size([w as f32, h as f32], scale);
    let (step_x, step_y) = (w / width as f64, h / height as f64);
    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let y = bounds.y0 + (row as f64 + 0.5) * step_y;
        for column in 0..width {
            let x = bounds.x0 + (column as f64 + 0.5) * step_x;
            let position = gradient_parameter(GradientKind::Conical, x, y, geom);
            let colour = interpolate_stops(stops, position);
            pixels.extend_from_slice(&[colour.r, colour.g, colour.b, colour.a]);
        }
    }
    let data = png_data(&pixels, width as u32, height as u32, image::ExtendedColorType::Rgba8).ok()?;
    let transform = kurbo::Affine::translate((bounds.x0, bounds.y0)) * kurbo::Affine::scale_non_uniform(step_x, step_y);
    Some(bitmap_pattern_data(id, width, height, transform, &data))
}

fn bitmap_pattern_data(id: &str, width: usize, height: usize, transform: kurbo::Affine, data: &str) -> String {
    let [a, b, c, d, e, f] = transform.as_coeffs();
    format!(
        "<defs><pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{width}\" height=\"{height}\" patternTransform=\"matrix({a} {b} {c} {d} {e} {f})\"><image width=\"{width}\" height=\"{height}\" xlink:href=\"data:image/png;base64,{data}\"/></pattern></defs>\n"
    )
}

fn write_fill(content: &mut String, fill: &dtool_core::paint::Paint, opts: &SvgOptions, gradient_id: Option<&str>) {
    if let Some(gradient_id) = gradient_id {
        content.push_str(&format!("fill=\"url(#{})\" ", gradient_id));
        return;
    }
    if let Some(colour) = crate::paint_colour(fill) {
        let _ = write!(content, "fill=\"{}\" ", svg_colour(&colour, opts.hex_colours));
        if colour.a < 255 {
            content.push_str(&format!("fill-opacity=\"{}\" ", format_float(f32::from(colour.a) / 255.0, 2)));
        }
    } else {
        content.push_str("fill=\"none\" ");
    }
}

fn write_stroke(content: &mut String, stroke: &dtool_core::paint::StrokeStyle, opts: &SvgOptions) {
    if let Some(colour) = crate::paint_colour(&stroke.paint) {
        let _ = write!(content, "stroke=\"{}\" ", svg_colour(&colour, opts.hex_colours));
        if colour.a < 255 {
            content.push_str(&format!("stroke-opacity=\"{}\" ", format_float(f32::from(colour.a) / 255.0, 2)));
        }
    }
    
    if stroke.width.is_finite() && stroke.width > 0.0 {
        content.push_str(&format!("stroke-width=\"{}\" ", format_float(stroke.width, opts.precision)));
    }

    use dtool_core::paint::{Cap, Join};
    match stroke.cap {
        Cap::Butt => {}
        Cap::Round | Cap::RoundInset => content.push_str("stroke-linecap=\"round\" "),
        Cap::Square => content.push_str("stroke-linecap=\"square\" "),
    }
    match stroke.join {
        Join::Round => content.push_str("stroke-linejoin=\"round\" "),
        Join::Bevel => content.push_str("stroke-linejoin=\"bevel\" "),
        Join::Miter { limit } if limit.is_finite() && limit >= 1.0 => {
            let _ = write!(content, "stroke-miterlimit=\"{}\" ", format_float(limit, opts.precision));
        }
        Join::Miter { .. } => {}
    }
    if !stroke.dash.is_empty() {
        let dash: Vec<String> = stroke.dash.iter().map(|value| format_float(*value, opts.precision)).collect();
        content.push_str(&format!("stroke-dasharray=\"{}\" ", dash.join(" ")));
        if stroke.dash_offset != 0.0 {
            content.push_str(&format!("stroke-dashoffset=\"{}\" ", format_float(stroke.dash_offset, opts.precision)));
        }
    }
}

fn svg_colour(colour: &dtool_core::paint::Colour, hex: bool) -> String {
    if hex {
        format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
    } else {
        format!("rgb({},{},{})", colour.r, colour.g, colour.b)
    }
}

fn shape_path_data(object: &Object, precision: u8) -> Option<String> {
    if !matches!(object.kind, dtool_core::ObjectKind::Shape) {
        return None;
    }
    if object.shape.is_some() && let Some(path) = crate::vector_world_path(object) {
        return Some(bezier_path_data(&path, precision));
    }
    let points = object.outline_points();
    let (first, rest) = points.split_first()?;
    if rest.is_empty() { return None; }
    let mut path = kurbo::BezPath::new();
    path.move_to((f64::from(first.0), f64::from(first.1)));
    for &(x, y) in rest { path.line_to((f64::from(x), f64::from(y))); }
    path.close_path();
    Some(bezier_path_data(&path, precision))
}

fn bezier_path_data(path: &kurbo::BezPath, precision: u8) -> String {
    use kurbo::PathEl;
    let coordinate = |value: f64| {
        let mut text = format_float(value, precision);
        if let Some(dot) = text.find('.') {
            let length = text.trim_end_matches('0').len();
            text.truncate(if length == dot + 1 { dot } else { length });
        }
        if text == "-0" { text.remove(0); }
        text
    };
    let point = |p: kurbo::Point| format!("{},{}", coordinate(p.x), coordinate(p.y));
    let mut data = String::new();
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(p) => { let _ = write!(data, "M{}", point(p)); }
            PathEl::LineTo(p) => { let _ = write!(data, " L{}", point(p)); }
            PathEl::QuadTo(a, p) => { let _ = write!(data, " Q{} {}", point(a), point(p)); }
            PathEl::CurveTo(a, b, p) => { let _ = write!(data, " C{} {} {}", point(a), point(b), point(p)); }
            PathEl::ClosePath => data.push_str(" Z"),
        }
    }
    data
}

fn format_float(value: impl Into<f64>, precision: u8) -> String {
    let places = usize::from(precision.min(MAX_COORDINATE_PRECISION));
    format!("{:.*}", places, value.into())
}

fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect::<String>()
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;")
}
