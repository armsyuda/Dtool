use base64::Engine;
use dtool_core::{blend::BlendMode, Document, Object, ObjectId};
use dtool_core::layer_style::raster::{EffectLayer, effect_bounds, export_effect_layers};
use dtool_core::scene::Prim;
use dtool_protocol::CommandError;
use image::ImageEncoder;
use std::{collections::{HashMap, HashSet}, fmt::Write};
use crate::{DocumentExportExt, ExportArea, ExportControl, ExportReceipt, ExportRequest, ObjectExportExt};

const SVG_WARN_SCOPE: &str = "svg";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let base = Self::default();
        if self.raster_dpi != base.raster_dpi {
            return Some("raster_dpi");
        }
        if self.rasterize != base.rasterize {
            return Some("rasterize");
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
    let (bytes, warnings) = encode_svg(doc, req, opts, control)?;
    crate::write_atomic(&req.path, &bytes).map_err(crate::export_io_error)?;
    Ok(ExportReceipt { path: req.path.clone(), bytes: bytes.len() as u64, warnings })
}

pub(crate) fn encode_svg(
    doc: &Document,
    req: &ExportRequest,
    opts: &SvgOptions,
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
    write_svg_header(&mut svg_content, width, height, unit_points, page_scoped, opts);
    let scale = if opts.set_viewbox { 1.0 } else { unit_points * (96.0 / 72.0) };
    let transformed = origin_x != 0.0 || origin_y != 0.0 || scale != 1.0;
    if transformed {
        svg_content.push_str(&format!(
            "<g transform=\"scale({}) translate({} {})\">\n",
            format_float(scale, opts.precision.max(6)),
            format_float(-origin_x, opts.precision),
            format_float(-origin_y, opts.precision)
        ));
    }
    
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
    let bitmap_blurs = dtool_core::layer_style::raster::export_bitmap_blurs(doc, &effect_sources);
    SvgLayers {
        doc, opts, control, children, object_ids, effect_scene, effect_sources, bitmap_blurs, area: [origin_x, origin_y, width, height],
        ranges: crate::blend_range_mask::RangeMasks::new(doc),
        page_area: page_scoped.then_some((origin_x, origin_y, width, height)),
    }.write_children(None, &mut svg_content, &mut warnings)?;

    if transformed {
        svg_content.push_str("</g>\n");
    }
    write_svg_footer(&mut svg_content);

    Ok((svg_content.into_bytes(), warnings))
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
        let passthrough = group.blend_mode == BlendMode::PassThrough
            && !group.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid());
        if passthrough && (masked || opacity < 1.0) && self.has_blended_child(group.id) {
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
        let range = self.open_blend_range(group, content)?;
        let scale = crate::raster::document_unit_points(self.doc) * self.opts.raster_dpi.max(1) as f32 / 72.0;
        let group_effects = crate::effect_image::group_effect_images(self.doc, group, self.effect_scene.as_ref(), scale);
        for (blend, raster) in group_effects.behind {
            write_effect_image(content, raster, blend, self.opts)?;
        }
        let (bounds, behind, above) = self.effect_sources.get(&group.id).map_or_else(Default::default, |prims| {
            let bounds = effect_bounds(self.doc, &[], group.id, prims);
            let (behind, above) = export_effect_layers(self.doc, group.id, &prims.iter().collect::<Vec<_>>());
            (bounds, behind, above)
        });
        if let Some(bounds) = bounds {
            write_effect_images(content, self.doc, bounds, &behind, self.opts)?;
        }
        self.write_children(Some(group.id), content, warnings)?;
        for (blend, raster) in group_effects.above {
            write_effect_image(content, raster, blend, self.opts)?;
        }
        if let Some(bounds) = bounds {
            write_effect_images(content, self.doc, bounds, &above, self.opts)?;
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
        let k08 = dtool_core::clip_mask::clip_region(doc, object.id);
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
        let crop = crate::vector_crop_bounds(object);
        if let Some([x, y, w, h]) = crop {
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
        if let Some(bounds) = bounds {
            write_effect_images(content, doc, bounds, &behind, opts)?;
        }
        let clipped = k08.as_ref().is_some_and(|rings| write_k08_open(content, doc, object, rings, opts));
        let pixel_mask = write_pixel_mask(content, doc, object, opts)?;
        if pixel_mask {
            let _ = writeln!(content, "<g mask=\"url(#dtool-pixel-mask-{})\">", object.id.0);
        }
        let transparent = self.open_transparency(object, content, warnings);
        let clear_text = self.ranges.as_ref().and_then(|ranges| ranges.clear_text_background(object));
        let range = clear_text.is_none() && self.open_blend_range(object, content)?;
        let object = clear_text.as_ref().unwrap_or(object);
        if let Some(ink) = dtool_core::path_brush::Ink::new(doc, object) {
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
            write_object(content, doc, &base, id, opts, self.page_area.is_some(), warnings);
            if !under {
                content.push_str(&image);
            }
        } else if let Some((bounds, layers)) = bitmap_blur {
            write_effect_images(content, doc, *bounds, layers, opts)?;
        } else if !write_raster_layer(content, doc, object, opts)? {
            write_object(content, doc, object, id, opts, self.page_area.is_some(), warnings);
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
        if let Some(bounds) = bounds {
            write_effect_images(content, doc, bounds, &above, opts)?;
        }
        if crop.is_some() {
            content.push_str("</g>\n");
        }
        if !blend.is_empty() {
            content.push_str("</g>\n");
        }
        Ok(())
    }

    fn open_blend_range(&self, object: &Object, content: &mut String) -> Result<bool, CommandError> {
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
        let Transparency::Gradient { kind, stops, geom } = &object.transparency else { return false };
        let id = object.id.0;
        let [x, y, w, h] = self.area;
        let f = |v| format_float(v, self.opts.precision.max(6));
        let (mut defs, ink) = if *kind == GradientKind::Conical {
            let scale = self.opts.raster_dpi as f32 * crate::raster::document_unit_points(self.doc) / 72.0;
            let Some(([width, height], pixels)) = crate::effect_image::transparency_pixels(object, self.area, scale) else { return false };
            let Ok(data) = png_data(&pixels, width, height, image::ExtendedColorType::Rgba8) else { return false };
            (String::new(), format!("<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{data}\"/>", f(x), f(y), f(w), f(h)))
        } else {
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
    let paint_path = object.path_data().unwrap_or_else(|| outline_path.clone());
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

fn write_svg_header(content: &mut String, width: f32, height: f32, unit_points: f32, page_scoped: bool, opts: &SvgOptions) {
    content.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    content.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" ");
    if page_scoped {
        content.push_str("overflow=\"hidden\" ");
    }
    
    if opts.set_viewbox {
        content.push_str(&format!(
            "viewBox=\"0 0 {} {}\" ",
            format_float(width, opts.precision),
            format_float(height, opts.precision)
        ));
    }
    
    content.push_str(&format!(
        "width=\"{}pt\" height=\"{}pt\">\n",
        format_float(width * unit_points, opts.precision),
        format_float(height * unit_points, opts.precision)
    ));
}

fn write_svg_footer(content: &mut String) {
    content.push_str("</svg>\n");
}

fn write_layout_text(
    content: &mut String,
    doc: &Document,
    object: &dtool_core::Object,
    id: &str,
    opts: &SvgOptions,
    use_shaped_text: bool,
) -> bool {
    use std::fmt::Write;
    if dtool_core::scene::has_live_text_warp(doc, object.id) {
        dtool_text::canvas_text::install();
        let lines = dtool_core::text_lines::text_lines(doc, object.id);
        if let Some(ink) = dtool_core::scene::warped_text_ink(doc, object, &lines, object.opacity) {
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
    // Match the font and baseline used to rasterize the effect's silhouette.
    let tabbed_only = !use_shaped_text && !object.effects.iter().any(|effect| effect.enabled)
        && object.text_scale == dtool_core::object::IDENTITY_TEXT_SCALE;
    let Some(layout) = dtool_text::canvas_text::text_export_layout(&library, doc, object, opts.text_as_curves, tabbed_only) else {
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
    let gradient = gradient_def(&path.paint, &gradient_id, opts);
    if let Some(defs) = gradient.as_deref() {
        content.push_str(defs);
    } else if paint_is_flattened(&path.paint) {
        warnings.push(format!("export.{}.paint_flattened:{}", SVG_WARN_SCOPE, object.0));
    }
    let rule = if path.even_odd { "evenodd" } else { "nonzero" };
    let _ = write!(content, "<path d=\"{}\" fill-rule=\"{rule}\" opacity=\"{}\" ", path.path.to_svg(), format_float(opacity, 4));
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

fn write_object(content: &mut String, doc: &Document, object: &dtool_core::Object, id: &str, opts: &SvgOptions, page_scoped: bool, warnings: &mut Vec<String>) {
    if dtool_core::commands::appearance::has_layers(object)
        && let Some(paths) = dtool_core::commands::appearance::vector_paths(doc, object, &object.fill, false)
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
                write_appearance_path(content, object.id, index, path, opts, warnings, 1.0, Some(&blacken));
                let _ = writeln!(content, "</mask></defs><g mask=\"url(#{mask_id})\">");
                masks.push(index);
            }
        }
        for (index, path) in paths.iter().enumerate() {
            if masks.last() == Some(&index) {
                masks.pop();
                content.push_str("</g>\n");
            } else {
                write_appearance_path(content, object.id, index, path, opts, warnings, opacity, None);
            }
        }
        content.push_str("</g>\n");
        return;
    }
    let gradient_id = format!("dtool-paint-{}", object.id.0);
    let gradient = gradient_def(&object.fill, &gradient_id, opts)
        .or_else(|| bitmap_fill_def(doc, object, &gradient_id));
    let fill_ref = gradient.as_ref().map(|_| gradient_id.as_str());
    if gradient.is_none() && paint_is_flattened(&object.fill) {
        warnings.push(format!("export.{}.paint_flattened:{}", SVG_WARN_SCOPE, object.id.0));
    }
    if paint_is_flattened(&object.stroke.paint) {
        warnings.push(format!("export.{}.stroke_paint_flattened:{}", SVG_WARN_SCOPE, object.id.0));
    }
    
    let warped = object.parent_id.and_then(|_| {
        let path = crate::vector_world_path(object)?;
        dtool_core::commands::warp::live_warp_outline(doc, object.id, &path)
    });
    let open_spiral = object.shape.as_ref().is_some_and(|shape| shape.kind == dtool_core::shape::ShapeKind::Spiral);
    match &object.kind {
        dtool_core::ObjectKind::Table(_) => write_table(content, doc, object, opts),
        dtool_core::ObjectKind::Shape if warped.is_none() && !open_spiral => {
            content.push_str(&format!("<g id=\"{}\">\n", id));
            
            if let Some(path_data) = object.path_data() {
                if let Some(defs) = gradient.as_deref() {
                    content.push_str(defs);
                }
                content.push_str(&format!("<path d=\"{}\" ", path_data));
                
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
        kind if matches!(kind, dtool_core::ObjectKind::Curve(_) | dtool_core::ObjectKind::Compound(_)) || warped.is_some() || open_spiral => {
            let Some(path) = warped.or_else(|| crate::vector_world_path(object)) else { return; };
            let (path, fill_path) = crate::vector_curve_paths(path);
            let _ = writeln!(content, "<g id=\"{id}\" opacity=\"{}\">", format_float(object.opacity, 4));
            if !fill_path.elements().is_empty() {
                if let Some(defs) = gradient.as_deref() {
                    content.push_str(defs);
                }
                let _ = write!(content, "<path d=\"{}\" fill-rule=\"evenodd\" ", fill_path.to_svg());
                write_fill(content, &object.fill, opts, fill_ref);
                content.push_str("/>\n");
            }
            if crate::stroke_colour(&object.stroke).is_some() {
                let _ = write!(content, "<path d=\"{}\" fill=\"none\" ", path.to_svg());
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
                        let _ = write!(content, "<path d=\"{}\" ", path.to_svg());
                        write_fill(content, &object.stroke.paint, opts, None);
                    } else {
                        let _ = write!(content, "<path d=\"{}\" fill=\"none\" ", path.to_svg());
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
            let container = crate::text_container(object).and_then(|container| container.path_data());
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
            let use_shaped_text = page_scoped || container.is_some()
                || matches!(object.kind, dtool_core::ObjectKind::ArtText { .. })
                || dtool_core::tone_blend::for_layer(doc, object.id).is_some();
            if write_layout_text(content, doc, object, id, opts, use_shaped_text) {
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
                
                content.push_str(&format!("<text id=\"{}\" x=\"{}\" y=\"{}\"", id, x, y));
                
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
                if opts.line_breaks && text_content.contains('\n') {
                    let line_height = font_size.unwrap_or(12.0) * 1.2;
                    for (index, line) in text_content.lines().enumerate() {
                        let dy = if index == 0 { 0.0 } else { line_height };
                        content.push_str(&format!(
                            "<tspan x=\"{}\" dy=\"{}\">{}</tspan>",
                            x,
                            format_float(dy, opts.precision),
                            escape_xml(line)
                        ));
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
                let hex = colour_to_hex(&colour);
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
                    hex,
                    format_float(width, p)
                ));
                let x = format_float(geometry.label_at[0] as f32, p);
                let y = format_float(geometry.label_at[1] as f32, p);
                content.push_str(&format!(
                    "<text x=\"{x}\" y=\"{y}\" font-size=\"{}\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{hex}\" transform=\"rotate({} {x} {y})\">{}</text>\n</g>\n",
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
    let f = |value: f64| format_float(value as f32, p);
    let dx = geom.end[0] - geom.start[0];
    let dy = geom.end[1] - geom.start[1];
    let (open, close) = match kind {
        GradientKind::Linear => (
            format!(
                "<linearGradient id=\"{}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
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
            colour_to_hex(&stop.colour)
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

fn bitmap_fill_def(doc: &Document, object: &Object, id: &str) -> Option<String> {
    let dtool_core::paint::Paint::Bitmap(bitmap) = &object.fill else { return None };
    let ([width, height], pixels) = dtool_core::scene::bitmap_source(doc, &bitmap.resource_id)?;
    let data = png_data(&pixels, width as u32, height as u32, image::ExtendedColorType::Rgba8).ok()?;
    let [x, y] = object.pos.map(f64::from);
    let transform = kurbo::Affine::new(dtool_core::geometry::object_matrix(object).map(f64::from))
        * kurbo::Affine::translate((x, y)) * kurbo::Affine::new(bitmap.matrix);
    let [a, b, c, d, e, f] = transform.as_coeffs();
    Some(format!(
        "<defs><pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{width}\" height=\"{height}\" patternTransform=\"matrix({a} {b} {c} {d} {e} {f})\"><image width=\"{width}\" height=\"{height}\" xlink:href=\"data:image/png;base64,{data}\"/></pattern></defs>\n"
    ))
}

fn write_fill(content: &mut String, fill: &dtool_core::paint::Paint, opts: &SvgOptions, gradient_id: Option<&str>) {
    if let Some(gradient_id) = gradient_id {
        content.push_str(&format!("fill=\"url(#{})\" ", gradient_id));
        return;
    }
    if let Some(colour) = crate::paint_colour(fill) {
        if opts.hex_colours {
            content.push_str(&format!("fill=\"{}\" ", colour_to_hex(&colour)));
        } else {
            content.push_str(&format!("fill=\"rgb({},{},{})\" ", 
                colour.r, 
                colour.g, 
                colour.b));
        }
        if colour.a < 255 {
            content.push_str(&format!("fill-opacity=\"{}\" ", format_float(f32::from(colour.a) / 255.0, 2)));
        }
    } else {
        content.push_str("fill=\"none\" ");
    }
}

fn write_stroke(content: &mut String, stroke: &dtool_core::paint::StrokeStyle, opts: &SvgOptions) {
    if let Some(colour) = crate::paint_colour(&stroke.paint) {
        if opts.hex_colours {
            content.push_str(&format!("stroke=\"{}\" ", colour_to_hex(&colour)));
        } else {
            content.push_str(&format!("stroke=\"rgb({},{},{})\" ", 
                colour.r, 
                colour.g, 
                colour.b));
        }
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
        Cap::Round => content.push_str("stroke-linecap=\"round\" "),
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
    }
}

fn colour_to_hex(colour: &dtool_core::paint::Colour) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b)
}

fn format_float(value: f32, precision: u8) -> String {
    match precision {
        0 => format!("{}", value as i32),
        1 => format!("{:.1}", value),
        2 => format!("{:.2}", value),
        3 => format!("{:.3}", value),
        4 => format!("{:.4}", value),
        _ => format!("{:.6}", value),
    }
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
}
