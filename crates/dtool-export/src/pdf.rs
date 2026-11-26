use dtool_core::{Document, Object, ObjectId, blend::BlendMode};
use dtool_core::paint::StrokeStyle;
use dtool_protocol::{CommandError, CommandResult};
use krilla::surface::Surface as PageBuilder;
use krilla::text::{Font, TextDirection};
use krilla::color::{Color, cmyk, rgb};
use krilla::Document as KrillaDocument;
use krilla::geom::{PathBuilder, Point, Transform};
use krilla::image::{BitsPerComponent, CustomImage, Image, ImageColorspace};
use krilla::num::NormalizedF32;
use krilla::page::{PageLabel, PageSettings};
use krilla::paint::{Fill, FillRule, Stroke};
use std::path::PathBuf;
use std::sync::Arc;
use crate::{DocumentExportExt, ObjectExportExt};

pub use crate::{ExportArea, ExportControl, ExportReceipt, ExportRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfCompat {
    V1_4,
    V1_5,
    V1_6,
    V1_7,
    V2_0,
    X1a2003,
    X3_2003,
    X4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfFonts {
    TextAsCurves,
    All,
    Uncommon,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterizeMode {
    Nothing,
    Unsupported,
    Everything,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterState {
    NothingRasterised,
    SomeApproximated,
    SomeRasterised,
    AllRasterised,
}

#[derive(Debug, Clone)]
pub struct PdfMarks {
    pub crop: bool,
    pub fold: bool,
    pub registration: bool,
    pub colour_bars: bool,
    pub page_info: bool,
}

#[derive(Debug, Clone)]
pub struct PdfPassword {
    pub open: Option<String>,
    pub permissions: Option<String>,
    pub allow_printing: bool,
    pub allow_modify: bool,
    pub allow_copy: bool,
    pub allow_advanced: bool,
}

#[derive(Debug, Clone)]
pub struct PdfOptions {
    pub compat: PdfCompat,
    pub colour_space: String,
    pub profile: Option<String>,
    pub fonts: PdfFonts,
    pub outline_strokes: bool,
    pub scale: f32,
    pub subset_fonts: bool,
    pub downsample: bool,
    pub downsample_dpi: u32,
    pub allow_jpeg: bool,
    pub jpeg_quality: u8,
    pub raster_dpi: u32,
    pub raster_dpi_override: Option<f32>,
    pub rasterize: RasterizeMode,
    pub include_layers: bool,
    pub include_invisible_layers: bool,
    pub include_bookmarks: bool,
    pub include_hyperlinks: bool,
    pub tagged: bool,
    pub include_bleed: bool,
    pub printers_marks: bool,
    pub marks: PdfMarks,
    pub overprint_black: bool,
    pub honour_spot: bool,
    pub convert_image_colour_spaces: bool,
    pub skip_slice_hidden: bool,
    pub password: Option<PdfPassword>,
    pub embed_icc: bool,
    pub embed_metadata: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            compat: PdfCompat::V1_7,
            colour_space: "CMYK".to_string(),
            profile: None,
            fonts: PdfFonts::TextAsCurves,
            outline_strokes: false,
            scale: 1.0,
            subset_fonts: true,
            downsample: false,
            downsample_dpi: 300,
            allow_jpeg: true,
            jpeg_quality: 85,
            raster_dpi: 300,
            raster_dpi_override: None,
            rasterize: RasterizeMode::Unsupported,
            include_layers: false,
            include_invisible_layers: false,
            include_bookmarks: true,
            include_hyperlinks: true,
            tagged: false,
            include_bleed: false,
            printers_marks: false,
            marks: PdfMarks {
                crop: false,
                fold: false,
                registration: false,
                colour_bars: false,
                page_info: false,
            },
            overprint_black: false,
            honour_spot: true,
            convert_image_colour_spaces: true,
            skip_slice_hidden: false,
            password: None,
            embed_icc: false,
            embed_metadata: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfPreset {
    ForPrint,
    PressReady,
    DigitalSmall,
    DigitalHigh,
    ForExport,
    Flatten,
    X1a,
    X3,
    X4,
}

impl PdfOptions {
    pub fn preset(p: PdfPreset) -> Self {
        match p {
            PdfPreset::PressReady => Self {
                compat: PdfCompat::V1_7,
                colour_space: "CMYK".to_string(),
                profile: None,
                fonts: PdfFonts::TextAsCurves,
                outline_strokes: false,
                scale: 1.0,
                subset_fonts: true,
                downsample: false,
                downsample_dpi: 300,
                allow_jpeg: true,
                jpeg_quality: 85,
                raster_dpi: 300,
                raster_dpi_override: None,
                rasterize: RasterizeMode::Unsupported,
                include_layers: false,
                include_invisible_layers: false,
                include_bookmarks: true,
                include_hyperlinks: true,
                tagged: false,
                include_bleed: true,
                printers_marks: true,
                marks: PdfMarks {
                    crop: true,
                    fold: false,
                    registration: true,
                    colour_bars: true,
                    page_info: true,
                },
                overprint_black: true,
                honour_spot: true,
                convert_image_colour_spaces: true,
                skip_slice_hidden: false,
                password: None,
                embed_icc: true,
                embed_metadata: true,
            },
            _ => Self::default(),
        }
    }

    pub fn validate(&self) -> Result<(), CommandError> {
        if self.raster_dpi_override.is_some_and(|dpi| {
            !(dtool_core::effect_raster::MIN_DPI..=dtool_core::effect_raster::MAX_DPI).contains(&dpi)
        }) {
            return Err(CommandError::invalid_params());
        }
        if !self.scale.is_finite() || self.scale <= 0.0 {
            return Err(CommandError::invalid_params());
        }
        if matches!(self.compat, PdfCompat::X1a2003 | PdfCompat::X3_2003 | PdfCompat::X4) && self.password.is_some() {
            return Err(CommandError::validation("error.pdf.password_not_x"));
        }
        if self.password.is_some() && matches!(self.compat, PdfCompat::V1_4 | PdfCompat::V1_5) {
            return Err(CommandError::validation("error.pdf.password_needs_16"));
        }
        Ok(())
    }

    fn uses_cmyk(&self) -> bool {
        self.colour_space.eq_ignore_ascii_case("CMYK")
    }
}

pub fn raster_state(doc: &Document, opts: &PdfOptions) -> RasterState {
    match opts.rasterize {
        RasterizeMode::Nothing => RasterState::NothingRasterised,
        RasterizeMode::Everything => RasterState::AllRasterised,
        RasterizeMode::Unsupported => {
            if doc.objects().iter().any(|obj| obj.raster_data.is_some()) {
                RasterState::SomeRasterised
            } else {
                RasterState::SomeApproximated
            }
        }
    }
}

pub fn export_pdf_value(
    doc: &Document,
    req: &ExportRequest,
    opts: &PdfOptions,
    control: &ExportControl,
) -> CommandResult {
    let receipt = export_pdf(doc, req, opts, control)?;
    Ok(serde_json::json!({
        "path": receipt.path.to_string_lossy(),
        "bytes": receipt.bytes,
        "warnings": receipt.warnings,
    }))
}

struct PdfShape {
    path: krilla::geom::Path,
    fill_path: Option<krilla::geom::Path>,
    outline: kurbo::BezPath,
    tolerance: f64,
}

struct PdfText {
    text: String,
    x: f32,
    y: f32,
    size: f32,
    fill: Fill,
}

trait PdfObjectExt {
    fn to_shape(&self, doc: &Document, tolerance: f64) -> Option<PdfShape>;
    fn to_text(&self, doc: &Document, output: &OutputColour) -> Option<PdfText>;
}

fn pdf_opacity(colour: dtool_core::paint::Colour, opacity: f32) -> NormalizedF32 {
    let value = (f32::from(colour.a) / 255.0 * opacity).clamp(0.0, 1.0);
    NormalizedF32::new(value).unwrap_or(NormalizedF32::ONE)
}

impl PdfObjectExt for dtool_core::Object {
    fn to_shape(&self, doc: &Document, tolerance: f64) -> Option<PdfShape> {
        let path = dtool_core::outline::object_outline_world_with_tolerance(self, tolerance)?;
        let path = dtool_core::commands::warp::live_warp_outline_with_tolerance(doc, self.id, &path, tolerance).unwrap_or(path);
        let (outline, fill_path) = crate::vector_curve_paths(path);
        Some(PdfShape { path: pdf_bez_path(&outline)?, fill_path: pdf_bez_path(&fill_path), outline, tolerance })
    }

    fn to_text(&self, doc: &Document, output: &OutputColour) -> Option<PdfText> {
        if !self.is_text_frame() && !matches!(self.kind, dtool_core::ObjectKind::ArtText { .. }) {
            return None;
        }
        let text = doc.object_text(self)?;
        if text.trim().is_empty() {
            return None;
        }
        let size = self.font_size().unwrap_or(12.0);
        let colour = crate::paint_colour(&self.fill).unwrap_or(dtool_core::paint::Colour::new(0, 0, 0, 255));
        Some(PdfText {
            text,
            x: self.x(),
            y: self.y() + size,
            size,
            fill: Fill {
                paint: output.colour(colour).into(),
                opacity: pdf_opacity(colour, self.opacity),
                rule: FillRule::NonZero,
            },
        })
    }
}

fn text_font() -> Option<Font> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("DTOOL_PDF_FONT") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(windir) = std::env::var_os("WINDIR") {
        let fonts = PathBuf::from(windir).join("Fonts");
        for name in ["malgun.ttf", "arial.ttf", "segoeui.ttf"] {
            candidates.push(fonts.join(name));
        }
    }
    for path in [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        candidates.push(PathBuf::from(path));
    }
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Some(font) = Font::new(bytes.into(), 0) {
                return Some(font);
            }
        }
    }
    None
}

fn draw_pdf_text(surface: &mut PageBuilder<'_>, font: &Font, text: &PdfText, outlined: bool) {
    surface.set_stroke(None);
    surface.set_fill(Some(text.fill.clone()));
    let line_height = text.size * 1.2;
    for (index, line) in text.text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        surface.draw_text(
            Point::from_xy(text.x, text.y + line_height * index as f32),
            font.clone(),
            text.size,
            line,
            outlined,
            TextDirection::Auto,
        );
    }
}

fn pdf_note_text(doc: &Document, output: &OutputColour) -> std::collections::HashMap<ObjectId, Vec<PdfText>> {
    let mut text = std::collections::HashMap::<ObjectId, Vec<PdfText>>::new();
    let black = dtool_core::paint::Colour::new(0, 0, 0, 255);
    let fill = |frame| Fill {
        paint: output.colour(black).into(),
        opacity: pdf_opacity(black, doc.get_object(frame).map_or(1.0, |object| object.opacity)),
        rule: FillRule::NonZero,
    };
    for line in dtool_text::notes_layout::output_note_lines(doc) {
        text.entry(line.frame).or_default().push(PdfText {
            text: line.text,
            x: line.baseline[0],
            y: line.baseline[1],
            size: line.size,
            fill: fill(line.frame),
        });
    }
    text
}

fn draw_layout_text(
    surface: &mut PageBuilder<'_>,
    doc: &Document,
    object: &dtool_core::Object,
    opts: &PdfOptions,
    output: &OutputColour,
    fallback: Option<&Font>,
    fonts: &mut std::collections::HashMap<dtool_text::FontId, Option<Font>>,
) -> bool {
    use dtool_text::canvas_text::{TextExportLayout, text_export_layout};
    let library = dtool_text::FontLibrary::shared();
    let layout = if matches!(object.kind, dtool_core::ObjectKind::PathText(_)) {
        // Per-glyph rotations and curved baselines are already applied to these outlines.
        let outline_ink = dtool_text::path_text::path_text_ink(&library, doc, object);
        let contours = if outline_ink.is_some() { Vec::new() } else {
            dtool_text::path_text::path_text_contours(&library, doc, object)
        };
        TextExportLayout {
            spans: Vec::new(),
            contours,
            outline_ink,
            paint: dtool_text::path_text::path_text_paint(doc, object),
            matrix: dtool_core::geometry::IDENTITY,
        }
    } else {
        let Some(layout) = text_export_layout(&library, doc, object, opts.fonts == PdfFonts::TextAsCurves || fallback.is_none()) else {
            return false;
        };
        layout
    };
    let Some(mut fill) = crate::pdf_paint::fill_for_output(surface, &layout.paint, object.opacity, output, doc) else {
        return false;
    };
    let transformed = layout.matrix != dtool_core::geometry::IDENTITY;
    if transformed {
        let [a, b, c, d, x, y] = layout.matrix;
        surface.push_transform(&Transform::from_row(a, b, c, d, x, y));
    }
    surface.set_stroke(None);
    fill.rule = FillRule::EvenOdd;
    let contour_path = |contours: &[Vec<[f32; 2]>]| region_path(&contours.iter().map(|contour| {
        contour.iter().map(|point| point.map(f64::from)).collect()
    }).collect::<Vec<_>>());
    if let Some(ink) = layout.outline_ink {
        for run in ink {
            if let Some(mut paint) = crate::pdf_paint::fill_for_output(surface, &run.paint, object.opacity, output, doc)
                && let Some(path) = contour_path(&run.contours)
            {
                paint.rule = FillRule::EvenOdd;
                surface.set_fill(Some(paint));
                surface.draw_path(&path);
            }
        }
    } else if let Some(path) = contour_path(&layout.contours) {
        surface.set_fill(Some(fill.clone()));
        surface.draw_path(&path);
    }
    for span in layout.spans {
        let paint = span.style.fill.as_ref().unwrap_or(&layout.paint);
        let span_fill = if paint == &layout.paint {
            fill.clone()
        } else {
            let Some(fill) = crate::pdf_paint::fill_for_output(surface, paint, object.opacity, output, doc) else { continue };
            fill
        };
        let font = fonts.entry(span.font).or_insert_with(|| {
            let face = library.face(span.font)?;
            Font::new(std::fs::read(&face.path).ok()?.into(), face.index)
        });
        if let Some(font) = font.as_ref().filter(|_| !span.glyphs.is_empty()) {
            draw_layout_span(surface, &span, font, paint, &span_fill, object.opacity, output, doc);
        } else if let Some(font) = font.as_ref().or(fallback) {
            surface.set_fill(Some(span_fill));
            surface.draw_text(
                Point::from_xy(span.origin[0], span.origin[1]),
                font.clone(),
                span.style.size_pt,
                &span.text,
                false,
                TextDirection::Auto,
            );
        }
    }
    if transformed {
        surface.pop();
    }
    true
}

fn draw_layout_span(
    surface: &mut PageBuilder<'_>, span: &dtool_text::canvas_text::ExportTextSpan, font: &Font,
    paint: &dtool_core::paint::Paint, fill: &Fill, opacity: f32, output: &OutputColour, doc: &Document,
) {
    use krilla::text::{GlyphId, KrillaGlyph};
    let size = span.style.size_pt;
    if !size.is_finite() || size <= 0.0 {
        return;
    }

    // The layout already applied baseline shifts and positioned each glyph.
    // Apply the same outline geometry while keeping the original Unicode text.
    let [sx, _, shear, sy, _, _] = dtool_text::GlyphTransform::from_style(&span.style).matrix();
    let transformed = sx != 1.0 || sy != 1.0 || shear != 0.0;
    let [x, y] = span.origin;
    if transformed {
        let matrix = kurbo::Affine::new([sx.into(), 0.0, (-shear).into(), sy.into(), x.into(), y.into()]);
        let span_fill = transform_pdf_paint(paint, matrix.inverse().as_coeffs())
            .and_then(|paint| crate::pdf_paint::fill_for_output(surface, &paint, opacity, output, doc));
        surface.set_fill(Some(span_fill.unwrap_or_else(|| fill.clone())));
        surface.push_transform(&Transform::from_row(sx, 0.0, -shear, sy, x, y));
    } else {
        surface.set_fill(Some(fill.clone()));
    }

    let mut pen = 0.0;
    let glyphs: Vec<_> = span.glyphs.iter().map(|glyph| {
        let dy = (glyph.origin[1] - y) / sy;
        let dx = (glyph.origin[0] - x + shear * dy) / sx;
        let advance = glyph.advance / sx;
        let placed = KrillaGlyph::new(
            GlyphId::new(u32::from(glyph.id)), advance / size, (dx - pen) / size, -dy / size, 0.0,
            glyph.text_range.clone(), None,
        );
        pen += advance;
        placed
    }).collect();
    let start = if transformed { Point::from_xy(0.0, 0.0) } else { Point::from_xy(x, y) };
    surface.draw_glyphs(start, &glyphs, font.clone(), &span.text, size, false);
    if transformed {
        surface.pop();
    }
}

pub fn export_pdf(
    doc: &Document,
    req: &ExportRequest,
    opts: &PdfOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    let (bytes, warnings) = encode_pdf(doc, req, opts, control, None)?;
    crate::write_atomic(&req.path, &bytes).map_err(crate::export_io_error)?;
    Ok(ExportReceipt { path: req.path.clone(), bytes: bytes.len() as u64, warnings })
}

enum PdfLayer<'a> {
    OpenClip(krilla::geom::Path),
    CloseClip,
    OpenGroup(&'a Object),
    Draw(&'a Object, Option<[f32; 4]>),
    CloseGroup(&'a Object),
}

fn pdf_layers<'a>(
    doc: &'a Document, area: &ExportArea, page_scoped: bool,
    page_members: Option<&std::collections::HashSet<ObjectId>>,
    master_clips: &std::collections::HashMap<ObjectId, [f32; 4]>,
) -> Vec<PdfLayer<'a>> {
    use std::collections::HashMap;
    fn append<'a>(
        layers: &mut Vec<PdfLayer<'a>>, doc: &Document, page_scoped: bool,
        children: &HashMap<Option<ObjectId>, Vec<&'a Object>>, parent: Option<ObjectId>,
        master_clips: &HashMap<ObjectId, [f32; 4]>,
    ) -> bool {
        let mut blends = false;
        for &object in children.get(&parent).into_iter().flatten() {
            let start = layers.len();
            let blended = !object.blend_mode.is_normal();
            let board = dtool_core::artboard::is_artboard(object);
            if object.is_group() || board {
                let child_blends = append(layers, doc, page_scoped, children, Some(object.id), master_clips);
                if start < layers.len() && (board || object.opacity < 1.0 || blended || object.blend_ranges.is_some()
                    || !matches!(object.transparency, dtool_core::paint::Transparency::None)
                    || dtool_core::raster_source::compositing_mask(doc, object).is_some()
                    || object.expanded_stroke_effects && object.effects.iter().any(|effect| effect.enabled)
                    || crate::effect_image::has_group_effect(object)
                    || object.blend_mode == BlendMode::Normal && child_blends)
                {
                    layers.insert(start, PdfLayer::OpenGroup(object));
                    layers.push(PdfLayer::CloseGroup(object));
                }
                blends |= blended || object.blend_mode == BlendMode::PassThrough && child_blends;
            } else {
                layers.push(PdfLayer::Draw(object, page_scoped.then(|| crate::vector_object_bounds(doc, object))));
                blends |= blended;
            }
            if start < layers.len()
                && let Some(clip) = master_clips.get(&object.id).copied().and_then(crop_clip_path)
            {
                layers.insert(start, PdfLayer::OpenClip(clip));
                layers.push(PdfLayer::CloseClip);
            }
        }
        blends
    }

    let mut children = HashMap::<Option<ObjectId>, Vec<&Object>>::new();
    let members = crate::artboard_members(doc, area);
    for object in doc.objects() {
        if members.as_ref().is_some_and(|ids| !ids.contains(&object.id))
            || page_members.is_some_and(|ids| !ids.contains(&object.id))
            || dtool_core::commands::masters::owns_source(doc, object.id)
        {
            continue;
        }
        let visible = if object.is_group() || dtool_core::artboard::is_artboard(object) {
            dtool_core::scene::is_drawn(doc, object.id) && object.opacity.is_finite() && object.opacity > 0.0
        } else {
            crate::export_visible(doc, object)
        };
        if !visible || dtool_core::clip_mask::is_mask(doc, object) || dtool_core::tone::attached(doc, object) {
            continue;
        }
        let mut parent = object.parent_id.and_then(|id| doc.get_object(id));
        while let Some(owner) = parent.filter(|owner| !owner.is_group() && !dtool_core::artboard::is_artboard(owner)) {
            parent = owner.parent_id.and_then(|id| doc.get_object(id));
        }
        children.entry(parent.map(|owner| owner.id)).or_default().push(object);
    }
    let mut layers = Vec::new();
    append(&mut layers, doc, page_scoped, &children, None, master_clips);
    layers
}

type RgbToCmyk = lcms2::Transform<[u8; 3], [u8; 4]>;

struct CmykOutput {
    profile: dtool_core::colour::IccProfile,
    rgb: RgbToCmyk,
    cmyk: Option<lcms2::Transform<[u8; 4], [u8; 4]>>,
}

pub(crate) struct OutputColour {
    cmyk: Option<CmykOutput>,
    convert_images: bool,
}

impl OutputColour {
    pub(crate) fn new(doc: Option<&Document>, opts: &PdfOptions) -> Result<Self, CommandError> {
        use dtool_core::colour::ColourModel;
        use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};

        if !opts.uses_cmyk() {
            return Ok(Self { cmyk: None, convert_images: false });
        }
        let unavailable = || crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable");
        let ctx = dtool_core::paint::DisplayColourCtx::for_optional(doc);
        let name = opts.profile.as_deref().unwrap_or(&ctx.cmyk);
        let services = dtool_core::colour::services().read().map_err(|_| crate::export_error("EXPORT_FAILED", "error.export.failed"))?;
        let profile = services.profiles.find(name).filter(|p| p.model == ColourModel::Cmyk)
            .cloned().ok_or_else(unavailable)?;
        let source = services.profiles.find(&ctx.cmyk).filter(|p| p.model == ColourModel::Cmyk && p.bytes != profile.bytes).cloned();
        drop(services);

        let output = Profile::new_icc(&profile.bytes).map_err(|_| unavailable())?;
        let (intent, flags) = (Intent::RelativeColorimetric, Flags::BLACKPOINT_COMPENSATION);
        let rgb = Transform::new_flags(&Profile::new_srgb(), PixelFormat::RGB_8, &output, PixelFormat::CMYK_8, intent, flags)
            .map_err(|_| unavailable())?;
        let cmyk = source.map(|source| {
            let input = Profile::new_icc(&source.bytes).map_err(|_| unavailable())?;
            Transform::new_flags(
                &input, PixelFormat::CMYK_8, &output, PixelFormat::CMYK_8,
                Intent::PreserveKOnlyRelativeColorimetric, flags,
            ).map_err(|_| unavailable())
        }).transpose()?;
        Ok(Self {
            cmyk: Some(CmykOutput { profile, rgb, cmyk }),
            convert_images: opts.convert_image_colour_spaces,
        })
    }

    fn image_transform(&self) -> Option<&RgbToCmyk> {
        self.cmyk.as_ref().filter(|_| self.convert_images).map(|output| &output.rgb)
    }

    pub(crate) fn bitmap_image(&self, doc: &Document, id: &str) -> Option<Image> {
        let transform = self.image_transform()?;
        let ([width, height], pixels) = dtool_core::scene::bitmap_source(doc, id)?;
        pixel_image(pixels, width as u32, height as u32, Some(transform)).ok()
    }

    pub(crate) fn colour(&self, colour: dtool_core::paint::Colour) -> Color {
        let Some(output) = &self.cmyk else { return rgb::Color::new(colour.r, colour.g, colour.b).into(); };
        let mut ink = [0; 4];
        if colour.model == dtool_core::colour::ColourModel::Cmyk {
            let input = colour.values.map(|value| (value.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8);
            ink = input;
            if let Some(transform) = &output.cmyk {
                transform.transform_pixels(&[input], std::slice::from_mut(&mut ink));
            }
        } else {
            output.rgb.transform_pixels(&[[colour.r, colour.g, colour.b]], std::slice::from_mut(&mut ink));
        }
        let [c, m, y, k] = ink;
        cmyk::Color::new(c, m, y, k).into()
    }
}

#[derive(Clone, Hash)]
struct CmykImage {
    size: (u32, u32),
    colour: Arc<[u8]>,
    alpha: Option<Arc<[u8]>>,
}

impl CustomImage for CmykImage {
    fn color_channel(&self) -> &[u8] { &self.colour }
    fn alpha_channel(&self) -> Option<&[u8]> { self.alpha.as_deref() }
    fn bits_per_component(&self) -> BitsPerComponent { BitsPerComponent::Eight }
    fn size(&self) -> (u32, u32) { self.size }
    fn icc_profile(&self) -> Option<&[u8]> { None }
    fn color_space(&self) -> ImageColorspace { ImageColorspace::Cmyk }
}

fn pixel_image(pixels: Vec<u8>, width: u32, height: u32, transform: Option<&RgbToCmyk>) -> Result<Image, CommandError> {
    let Some(transform) = transform else { return Ok(Image::from_rgba8(pixels, width, height)); };
    let mut colour = Vec::with_capacity(pixels.len());
    let mut input = [[0; 3]; 1024];
    let mut output = [[0; 4]; 1024];
    for chunk in pixels.chunks(input.len() * 4) {
        let count = chunk.len() / 4;
        for (rgb, pixel) in input.iter_mut().zip(chunk.chunks_exact(4)) {
            rgb.copy_from_slice(&pixel[..3]);
        }
        transform.transform_pixels(&input[..count], &mut output[..count]);
        colour.extend_from_slice(output[..count].as_flattened());
    }
    let alpha = pixels.iter().skip(3).step_by(4).any(|&value| value != 255)
        .then(|| pixels.iter().skip(3).step_by(4).copied().collect());
    Image::from_custom(CmykImage { size: (width, height), colour: colour.into(), alpha }, false)
        .map_err(|_| CommandError::new("error.export.write_failed"))
}

pub(crate) fn encode_pdf(
    doc: &Document,
    req: &ExportRequest,
    opts: &PdfOptions,
    control: &ExportControl,
    fitted_size: Option<[f32; 2]>,
) -> Result<(Vec<u8>, Vec<String>), CommandError> {
    opts.validate()?;
    if matches!(opts.compat, PdfCompat::X1a2003 | PdfCompat::X3_2003 | PdfCompat::X4) {
        return Err(crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.pdf.pdfx_unavailable"));
    }
    let mut warnings = Vec::new();
    if opts.password.is_some() {
        warnings.push("warning.pdf.password_not_applied".to_string());
    }

    dtool_text::canvas_text::install();
    let mut settings = krilla::SerializeSettings::default();
    settings.no_device_cs = opts.embed_icc;
    let output = OutputColour::new(Some(doc), opts)?;
    let image_transform = output.image_transform();
    let intent_profile = output.cmyk.as_ref().filter(|_| opts.embed_icc).map(|output| &output.profile);
    if let Some(profile) = intent_profile {
        settings.cmyk_profile = Some(krilla::icc::ICCProfile::<4>::new(&profile.bytes)
            .ok_or_else(|| crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable"))?);
    } else if opts.embed_icc && !opts.uses_cmyk()
        && opts.profile.as_deref().is_some_and(|profile| dtool_core::colour::ProfileStore::alias(profile) != Some(dtool_core::colour::ProfileStore::SRGB)) {
        return Err(crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable"));
    }
    let mut krilla_doc = KrillaDocument::new_with(settings);
    if opts.embed_metadata {
        let meta = &doc.metadata;
        let description = if meta.subject.is_empty() { meta.description.clone() } else { meta.subject.clone() };
        let metadata = krilla::metadata::Metadata::new()
            .title(meta.title.clone())
            .authors(if meta.author.is_empty() { Vec::new() } else { vec![meta.author.clone()] })
            .description(description)
            .keywords(meta.keywords.clone())
            .creator("DTool".to_string());
        krilla_doc.set_metadata(metadata);
    }

    let crop_marks = opts.printers_marks && opts.marks.crop;
    let fold_marks = opts.printers_marks && opts.marks.fold;
    if ((opts.include_bleed || crop_marks) && !req.area.is_page_based())
        || (fold_marks && !req.area.limits_objects_to_page())
    {
        return Err(crate::export_error(crate::raster::ERROR_UNAVAILABLE, "error.export.option_unavailable"));
    }
    let batches = crate::output_page_batches(doc, &req.area, req.pages.as_deref());
    let page_rects: Vec<(f32, f32, f32, f32)> = match &req.area {
        ExportArea::WholeDocument => {
            vec![doc.area_bounds(&req.area).ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.export.bad_area"))?]
        }
        ExportArea::CurrentPage | ExportArea::AllPages | ExportArea::CurrentSpread | ExportArea::AllSpreads | ExportArea::Page(_) | ExportArea::Spread(_) | ExportArea::PageRun { .. } => {
            crate::pdf_regions_for_batches(doc, &batches).ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.export.bad_area"))?
        }
        ExportArea::Rect([x, y, w, h]) => {
            if *w <= 0.0 || *h <= 0.0 {
                return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
            }
            vec![(*x, *y, *w, *h)]
        }
        ExportArea::SelectionWithBackground | ExportArea::SelectionWithoutBackground => {
            if let Some(sel) = doc.selection_bounds() {
                let w = sel.2;
                let h = sel.3;
                if w <= 0.0 || h <= 0.0 {
                    return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
                }
                vec![(sel.0, sel.1, w, h)]
            } else {
                return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
            }
        }
        ExportArea::Artboard(id) => {
            if let Some(ab) = crate::raster::artboard_region(doc, *id) {
                let w = ab.2;
                let h = ab.3;
                if w <= 0.0 || h <= 0.0 {
                    return Err(crate::export_error("INVALID_PARAMS", "error.export.bad_area"));
                }
                vec![(ab.0, ab.1, w, h)]
            } else {
                return Err(crate::export_error("NOT_FOUND", "error.export.bad_area"));
            }
        }
    };

    let has_text = doc.objects().iter().any(|obj| obj.is_text_frame() || matches!(obj.kind, dtool_core::ObjectKind::ArtText { .. } | dtool_core::ObjectKind::Dimension(_)));
    let font = if has_text && opts.fonts != PdfFonts::None { text_font() } else { None };
    let note_text = font.as_ref().map(|_| pdf_note_text(doc, &output)).unwrap_or_default();
    let mut tab_fonts = std::collections::HashMap::new();
    if has_text && opts.fonts != PdfFonts::None && font.is_none() {
        warnings.push("warning.pdf.text_font_missing".to_string());
    }
    let page_scoped = req.area.limits_objects_to_page();

    let scale = crate::raster::document_unit_points(doc) * opts.scale;
    let vector_tolerance = crate::vector_export_tolerance(f64::from(scale));
    let effect_pixels_per_unit = opts.raster_dpi_override.unwrap_or_else(|| dtool_core::effect_raster::pdf_dpi(doc))
        * scale / 72.0;
    let mask_pixels_per_unit = opts.raster_dpi_override.unwrap_or(opts.raster_dpi as f32) * scale / 72.0;
    let bleed = doc.setup.bleed;
    let valid_bleed = |value: f32| if opts.include_bleed && bleed.enabled && value.is_finite() && value > 0.0 { value } else { 0.0 };
    let (left, right, top, bottom) = (valid_bleed(bleed.left), valid_bleed(bleed.right), valid_bleed(bleed.top), valid_bleed(bleed.bottom));
    let page_geometry: Vec<_> = page_rects.iter().enumerate().map(|(index, &(x, y, w, h))| {
        let folds = if fold_marks {
            batches.get(index).map_or_else(Vec::new, |pages| fold_mark_segments(
                doc, pages, [x, y, x + w, y + h],
                [x - left, y - top, x + w + right, y + h + bottom], 1.0 / scale,
            ))
        } else {
            Vec::new()
        };
        let margin = if crop_marks || !folds.is_empty() { 18.0 / scale } else { 0.0 };
        let offset = fitted_size.map_or([left + margin, top + margin], |[width, height]| {
            [((width / scale - w) * 0.5).max(0.0), ((height / scale - h) * 0.5).max(0.0)]
        });
        (crate::pdf_links::Placement { trim: [x, y, w, h], offset, scale }, margin, folds)
    }).collect();
    let include_links = opts.include_hyperlinks && opts.fonts != PdfFonts::None
        && (!doc.hyperlinks().is_empty() || !doc.toc.tocs.is_empty());
    let include_bookmarks = opts.include_bookmarks && opts.fonts != PdfFonts::None && !doc.toc.spec.styles.is_empty();
    let links = if include_links || include_bookmarks {
        let placements: Vec<_> = page_geometry.iter().map(|(placement, _, _)| *placement).collect();
        if include_bookmarks {
            krilla_doc.set_outline(crate::pdf_links::bookmarks(doc, &placements, &batches, &req.area));
        }
        include_links.then(|| crate::pdf_links::Links::new(doc, &placements, &batches, &req.area))
    } else {
        None
    };
    let effect_scene = doc.objects().iter().any(|object| object.effects.iter().any(|effect| effect.enabled)).then(|| {
        dtool_render::scene::install();
        dtool_core::scene::build_scene(doc, &dtool_core::CanvasFlags::default())
    });
    let master_clips = dtool_core::commands::masters::fill_clips(doc);
    for (index, (placement, mark_margin, folds)) in page_geometry.into_iter().enumerate() {
        let [x, y, w, h] = placement.trim;
        if control.is_cancelled() {
            return Err(CommandError::new("error.export.cancelled"));
        }
        let pages = batches.get(index);
        let members = pages.map(|pages| crate::page_members(doc, pages));
        let ranges = crate::blend_range_mask::RangeMasks::for_members(doc, members.as_ref());
        let page_effect_scene = effect_scene.as_ref().map(|scene| {
            let mut scene = scene.clone();
            if let (Some(pages), Some(members)) = (pages, members.as_ref()) {
                crate::retain_page_scene(&mut scene, pages, members);
            }
            scene
        });
        let effect_sources = page_effect_scene.as_ref().map_or_else(Default::default, |scene| {
            dtool_core::layer_style::raster::export_effect_sources(doc, scene)
        });
        let bitmap_blurs = dtool_core::layer_style::raster::export_bitmap_blurs(doc, &effect_sources);

        let facing_bleed = pages.and_then(|pages| crate::facing_bleed_regions(doc, pages, [left, top, right, bottom]))
            .and_then(|regions| regions.into_iter().map(|(page, (x, y, width, height))| {
                Some((page, crop_clip_path([x, y, width, height])?))
            }).collect::<Option<Vec<_>>>());
        let layers = if let Some(regions) = facing_bleed {
            let mut layers = Vec::new();
            for (page, clip) in regions {
                let members = crate::page_members(doc, &[page]);
                layers.push(PdfLayer::OpenClip(clip));
                layers.extend(pdf_layers(doc, &req.area, page_scoped, Some(&members), &master_clips));
                layers.push(PdfLayer::CloseClip);
            }
            layers
        } else {
            pdf_layers(doc, &req.area, page_scoped, members.as_ref(), &master_clips)
        };
        let [outer_x, outer_y] = placement.offset;
        let [media_w, media_h] = fitted_size.unwrap_or([
            (w + left + right + 2.0 * mark_margin) * scale,
            (h + top + bottom + 2.0 * mark_margin) * scale,
        ]);
        let mut settings = PageSettings::from_wh(media_w, media_h)
            .ok_or_else(|| crate::export_error("INVALID_PARAMS", "error.export.bad_area"))?;
        if fitted_size.is_some() {
            let frame = krilla::geom::Rect::from_xywh(0.0, 0.0, media_w, media_h);
            settings = settings.with_trim_box(frame).with_bleed_box(frame);
        } else {
            settings = settings
                .with_trim_box(krilla::geom::Rect::from_xywh(outer_x * scale, outer_y * scale, w * scale, h * scale))
                .with_bleed_box(krilla::geom::Rect::from_xywh(mark_margin * scale, mark_margin * scale, (w + left + right) * scale, (h + top + bottom) * scale));
        }
        if !doc.sections().is_empty() && let Some(pages) = pages
            && let Some((&first, &last)) = pages.first().zip(pages.last())
        {
            let label_at = |index| dtool_core::longdoc::sections::section_page_label(doc, index);
            let label = if first == last { label_at(first) } else { format!("{}–{}", label_at(first), label_at(last)) };
            // Literal labels also preserve the application's alphabetic, Hangul and Hanja numbering.
            settings = settings.with_page_label(PageLabel::new(None, Some(label), None));
        }
        let mut page = krilla_doc.start_page_with(settings);
        let mut annotations = Vec::new();
        {
            let mut surface = page.surface();
            surface.push_transform(&Transform::from_scale(scale, scale));
            surface.push_transform(&Transform::from_translate(outer_x - x, outer_y - y));

            let region = (x - left, y - top, w + left + right, h + top + bottom);
            let range_area = [region.0, region.1, region.2, region.3];
            let range_scale = mask_pixels_per_unit;
            let page_clip = (page_scoped || crop_marks || fitted_size.is_some())
                .then(|| krilla::geom::Rect::from_xywh(region.0, region.1, region.2, region.3))
                .flatten()
                .and_then(|rect| {
                    let mut path = PathBuilder::new();
                    path.push_rect(rect);
                    path.finish()
                });
            if let Some(clip) = &page_clip {
                surface.push_clip_path(clip, &FillRule::NonZero);
            }

            let mut group_stack = Vec::new();
            let mut effects_rasterised = false;
            for layer in &layers {
                let (obj, bounds) = match *layer {
                    PdfLayer::OpenClip(ref clip) => {
                        surface.push_clip_path(clip, &FillRule::NonZero);
                        continue;
                    }
                    PdfLayer::CloseClip => {
                        surface.pop();
                        continue;
                    }
                    PdfLayer::OpenGroup(group) => {
                        let blur = if opts.rasterize == RasterizeMode::Nothing {
                            None
                        } else {
                            crate::effect_image::pdf_group_blur(doc, group, page_effect_scene.as_ref(), effect_pixels_per_unit, scale)
                        };
                        let board_clip = dtool_core::artboard::is_artboard(group)
                            .then(|| region_path(&[crate::artboard_ring(group)])).flatten();
                        if let Some(path) = &board_clip {
                            surface.push_clip_path(path, &FillRule::NonZero);
                        }
                        if !group.blend_mode.is_normal() {
                            surface.push_blend_mode(effect_blend(group.blend_mode));
                        }
                        let isolate = group.blend_mode != BlendMode::PassThrough
                            || blur.is_some()
                            || group.blend_ranges.as_ref().is_some_and(|ranges| ranges.is_valid());
                        if group.opacity < 1.0 {
                            surface.push_opacity(NormalizedF32::new(group.opacity.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE));
                        } else if isolate {
                            surface.push_isolated();
                        }
                        let (x, y, w, h) = region;
                        let pixel_mask = push_pixel_mask(&mut surface, doc, group);
                        let transparency = push_object_transparency(&mut surface, doc, group, [x, y, w, h], mask_pixels_per_unit);
                        let range = ranges.as_ref().and_then(|ranges| ranges.sample(group, range_area, range_scale))
                            .is_some_and(|mask| push_blend_range_mask(&mut surface, mask));
                        let blur = blur.and_then(|blur| {
                            let clip = blur.clip_to_bounds.then(|| crop_clip_path(blur.raster.bounds)).flatten();
                            push_blend_range_mask(&mut surface, blur.source_mask).then_some((blur.raster, clip))
                        });
                        let group_effects = crate::effect_image::group_effect_images(doc, group, page_effect_scene.as_ref(), effect_pixels_per_unit);
                        for (blend, raster) in group_effects.behind {
                            effects_rasterised |= draw_effect_image(&mut surface, raster, blend, image_transform)?;
                        }
                        let (bounds, behind, above) = effect_sources.get(&group.id).map_or_else(Default::default, |prims| {
                            let bounds = dtool_core::layer_style::raster::effect_bounds(doc, &[], group.id, prims);
                            let (behind, above) = dtool_core::layer_style::raster::export_effect_layers(doc, group.id, &prims.iter().collect::<Vec<_>>());
                            (bounds, behind, above)
                        });
                        effects_rasterised |= draw_effect_images(&mut surface, doc, bounds, &behind, effect_pixels_per_unit, image_transform)?;
                        let pushes = usize::from(board_clip.is_some()) + usize::from(pixel_mask) + usize::from(transparency) + usize::from(range)
                            + usize::from(group.opacity < 1.0 || isolate) + usize::from(!group.blend_mode.is_normal());
                        group_stack.push((pushes, bounds, above, group_effects.above, blur));
                        continue;
                    }
                    PdfLayer::CloseGroup(_) => {
                        if let Some((pushes, bounds, above, group_above, blur)) = group_stack.pop() {
                            effects_rasterised |= draw_effect_images(&mut surface, doc, bounds, &above, effect_pixels_per_unit, image_transform)?;
                            for (blend, raster) in group_above {
                                effects_rasterised |= draw_effect_image(&mut surface, raster, blend, image_transform)?;
                            }
                            if let Some((raster, clip)) = blur {
                                surface.pop();
                                if let Some(clip) = &clip {
                                    surface.push_clip_path(clip, &FillRule::NonZero);
                                }
                                effects_rasterised |= draw_effect_image(&mut surface, raster, BlendMode::Normal, image_transform)?;
                                if clip.is_some() {
                                    surface.pop();
                                }
                            }
                            for _ in 0..pushes {
                                surface.pop();
                            }
                        }
                        continue;
                    }
                    PdfLayer::Draw(obj, bounds) => (obj, bounds),
                };
                let bitmap_blur = bitmap_blurs.get(&obj.id);
                let primitives = effect_sources.get(&obj.id);
                let effect_bounds = primitives.and_then(|prims|
                    dtool_core::layer_style::raster::effect_bounds(doc, &[], obj.id, prims));
                if bounds.is_some_and(|bounds| !crate::vector_bounds_intersect(bounds, region))
                    && !effect_bounds.or_else(|| bitmap_blur.map(|(bounds, _)| *bounds)).is_some_and(|[x, y, w, h]| {
                        crate::vector_bounds_intersect([x, y, x + w, y + h], region)
                    })
                {
                    continue;
                }
                // K08: clip children / vector masks clip by the true path (W* n); fully clipped objects are dropped.
                let k08 = match dtool_core::clip_mask::layer_clip_region(doc, obj.id) {
                    Some(rings) => match region_path(&rings) {
                        Some(path) => Some(path),
                        None => continue,
                    },
                    None => None,
                };
                if k08.is_none()
                    && obj.blend_mode.is_normal()
                    && matches!(obj.transparency, dtool_core::paint::Transparency::None)
                    && obj.blend_ranges.is_none() && obj.tone_blend.is_none()
                    && dtool_core::raster_source::compositing_mask(doc, obj).is_none()
                    && opts.rasterize != RasterizeMode::Nothing
                    && !matches!(opts.compat, PdfCompat::X1a2003 | PdfCompat::X3_2003)
                    && let Some((source, _)) = crate::effect_image::solid_leaf_blur(obj, primitives.map(Vec::as_slice))
                    && let Some(bounds) = effect_bounds
                    && draw_solid_blur(&mut surface, doc, obj, source, bounds, effect_pixels_per_unit, &output)
                {
                    effects_rasterised = true;
                    continue;
                }
                let (behind, above) = primitives.map_or_else(Default::default, |prims|
                    dtool_core::layer_style::raster::export_effect_layers(doc, obj.id, &prims.iter().collect::<Vec<_>>()));
                // Effects use the cropped source geometry and may extend beyond it.
                let mut effect_drawn = draw_effect_images(&mut surface, doc, effect_bounds, &behind, effect_pixels_per_unit, image_transform)?;
                let crop = dtool_core::scene::vector_crop_quad(obj)
                    .and_then(|quad| region_path(&[quad.into_iter().map(|point| point.map(f64::from)).collect()]));
                if let Some(path) = crop.as_ref() {
                    surface.push_clip_path(path, &FillRule::NonZero);
                }
                if let Some(path) = k08.as_ref() {
                    surface.push_clip_path(path, &FillRule::EvenOdd);
                }
                let blended = !obj.blend_mode.is_normal();
                if blended {
                    surface.push_blend_mode(effect_blend(obj.blend_mode));
                    surface.push_isolated();
                }
                let clear_text = ranges.as_ref().and_then(|ranges| ranges.clear_text_background(obj));
                let range = clear_text.is_none() && ranges.as_ref().and_then(|ranges| ranges.sample(obj, range_area, range_scale))
                    .is_some_and(|mask| push_blend_range_mask(&mut surface, mask));
                let obj = clear_text.as_ref().unwrap_or(obj);
                let pixel_mask = push_pixel_mask(&mut surface, doc, obj);
                let mask = if matches!(obj.transparency, dtool_core::paint::Transparency::None) {
                    false
                } else {
                    let [left, top, right, bottom] = crate::vector_object_bounds(doc, obj);
                    push_object_transparency(&mut surface, doc, obj, [left, top, right - left, bottom - top], mask_pixels_per_unit)
                };
                let warped_text = dtool_core::scene::has_live_text_warp(doc, obj.id).then(|| {
                    let lines = dtool_core::text_lines::text_lines(doc, obj.id);
                    dtool_core::scene::warped_text_ink_with_tolerance(doc, obj, &lines, obj.opacity, vector_tolerance)
                }).flatten();
                let decorations = if warped_text.is_some() { Vec::new() } else { dtool_core::text_lines::text_decorations(doc, obj.id) };
                let text_matrix = dtool_core::geometry::text_matrix(obj);
                draw_decorations(&mut surface, &text_matrix, &decorations, true, obj.opacity, &output);
                if let Some(geometry) = dtool_core::outline::dimension_geometry(obj) {
                    draw_dimension(&mut surface, obj, &geometry, font.as_ref(), &output, opts);
                }
                let brush = dtool_core::path_brush::Ink::new(doc, obj);
                let brush_under = matches!(obj.paint_order, dtool_core::paint::PaintOrder::StrokeUnder);
                if let Some(ink) = brush.as_ref().filter(|_| brush_under) {
                    effect_drawn |= draw_path_brush(&mut surface, doc, ink, obj, effect_pixels_per_unit, image_transform)?;
                }
                let base = brush.as_ref().map(|_| {
                    let mut base = obj.clone();
                    base.path_brush = None;
                    base.stroke.paint = dtool_core::paint::Paint::None;
                    base
                });
                let obj = base.as_ref().unwrap_or(obj);
                let text_container = crate::text_container(obj);
                let mut text_stroke = None;
                let image_drawn = if let Some((bounds, blur)) = bitmap_blur {
                    effect_drawn |= draw_effect_images(&mut surface, doc, Some(*bounds), blur, effect_pixels_per_unit, image_transform)?;
                    true
                } else {
                    draw_raster_layer(&mut surface, doc, obj, &output, image_transform, opts.outline_strokes)?
                };
                let appearance = !image_drawn && dtool_core::commands::appearance::has_layers(obj);
                if appearance && let Some(paths) = dtool_core::commands::appearance::vector_paths_with_tolerance(
                    doc, obj, &obj.fill, !opts.outline_strokes, vector_tolerance,
                ) {
                    let isolated = obj.opacity < 1.0 || paths.iter().any(|path| !path.blend.is_normal());
                    let opacity = if isolated { 1.0 } else { obj.opacity };
                    let group_opacity = isolated && obj.opacity < 1.0;
                    if group_opacity {
                        surface.push_opacity(NormalizedF32::new(obj.opacity.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE));
                    }
                    if isolated {
                        surface.push_isolated();
                    }
                    // Each eraser masks precisely the prefix underneath it. Open those
                    // groups from the last eraser back, then close them in paint order.
                    let erase_bounds = paths.iter().any(|path| path.blend.erases())
                        .then(|| crate::vector_object_bounds(doc, obj));
                    let mut masks = Vec::new();
                    if let Some(bounds) = erase_bounds {
                        for (index, path) in paths.iter().enumerate().rev().filter(|(_, path)| path.blend.erases()) {
                            if let Some(geometry) = pdf_bez_path(&path.path)
                                && let Some(mask) = crate::pdf_paint::appearance_erase_mask(
                                    &mut surface, &geometry, &path.paint, path.even_odd, bounds, &output, doc,
                                )
                            {
                                surface.push_mask(mask);
                                masks.push(index);
                            }
                        }
                    }
                    let bounds = crate::vector_object_bounds(doc, obj);
                    for (index, path) in paths.into_iter().enumerate() {
                        use dtool_core::commands::appearance::RowBlend;
                        let RowBlend::Standard(mode) = path.blend else {
                            if masks.last() == Some(&index) {
                                masks.pop();
                                surface.pop();
                            }
                            continue;
                        };
                        let Some(geometry) = pdf_bez_path(&path.path) else { continue; };
                        if let Some(mut style) = path.stroke {
                            if let std::borrow::Cow::Owned(paint) = crate::pdf_paint::bounded_gradient(&style.paint, bounds) {
                                style.paint = paint;
                            }
                            let Some(stroke) = crate::pdf_paint::stroke_for_output(&mut surface, &style, opacity, &output, doc) else { continue; };
                            surface.set_fill(None);
                            surface.set_stroke(Some(stroke));
                        } else {
                            let paint = crate::pdf_paint::bounded_gradient(&path.paint, bounds);
                            let Some(mut fill) = crate::pdf_paint::fill_for_output(&mut surface, &paint, opacity, &output, doc) else { continue; };
                            fill.rule = if path.even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
                            surface.set_stroke(None);
                            surface.set_fill(Some(fill));
                        }
                        if !mode.is_normal() {
                            surface.push_blend_mode(effect_blend(mode));
                        }
                        surface.draw_path(&geometry);
                        if !mode.is_normal() {
                            surface.pop();
                        }
                    }
                    if isolated {
                        surface.pop();
                    }
                    if group_opacity {
                        surface.pop();
                    }
                } else if let Some(shape) = (!image_drawn).then(|| {
                    text_container.as_ref().unwrap_or(obj).to_shape(doc, vector_tolerance)
                }).flatten() {
                    use dtool_core::paint::PaintOrder;
                    let transform = pdf_paint_transform(obj).as_coeffs();
                    let bounds = crate::vector_object_bounds(doc, obj);
                    let fill_paint = transform_pdf_paint(&obj.fill, transform);
                    let fill_paint = crate::pdf_paint::bounded_gradient(fill_paint.as_ref().unwrap_or(&obj.fill), bounds);
                    let mut stroke_style = transform_pdf_paint(&obj.stroke.paint, transform)
                        .map(|paint| StrokeStyle { paint, ..obj.stroke.clone() });
                    if let std::borrow::Cow::Owned(paint) = crate::pdf_paint::bounded_gradient(
                        &stroke_style.as_ref().unwrap_or(&obj.stroke).paint, bounds,
                    ) {
                        stroke_style = Some(StrokeStyle { paint, ..obj.stroke.clone() });
                    }
                    let fill = crate::pdf_paint::fill_for_output(&mut surface, &fill_paint, obj.opacity, &output, doc);
                    let stroke = crate::pdf_paint::stroke_for_output(&mut surface, stroke_style.as_ref().unwrap_or(&obj.stroke), obj.opacity, &output, doc);
                    let stroke_under = matches!(obj.paint_order, PaintOrder::StrokeUnder);
                    if let Some(stroke) = stroke.as_ref().filter(|_| stroke_under) {
                        draw_shape_stroke(&mut surface, doc, obj, &shape, stroke, opts.outline_strokes);
                    }
                    if let (Some(mut fill), Some(path)) = (fill, shape.fill_path.as_ref()) {
                        fill.rule = if obj.fill_rule.is_even_odd() { FillRule::EvenOdd } else { FillRule::NonZero };
                        surface.set_stroke(None);
                        surface.set_fill(Some(fill));
                        surface.draw_path(path);
                    }
                    if let Some(stroke) = stroke.filter(|_| !stroke_under) {
                        if text_container.is_some() {
                            text_stroke = Some((shape, stroke));
                        } else {
                            draw_shape_stroke(&mut surface, doc, obj, &shape, &stroke, opts.outline_strokes);
                        }
                    }
                }
                if let Some(ink) = brush.as_ref().filter(|_| !brush_under) {
                    effect_drawn |= draw_path_brush(&mut surface, doc, ink, obj, effect_pixels_per_unit, image_transform)?;
                }
                if matches!(obj.kind, dtool_core::ObjectKind::Table(_)) {
                    draw_vector_ink(&mut surface, doc, dtool_render::scene::table_prims(doc, obj), &output);
                }
                let layout_drawn = if let Some(ink) = warped_text {
                    if opts.fonts != PdfFonts::None {
                        draw_vector_ink(&mut surface, doc, ink, &output);
                    }
                    true
                } else { opts.fonts != PdfFonts::None
                    && draw_layout_text(&mut surface, doc, obj, opts, &output, font.as_ref(), &mut tab_fonts) };
                if let Some(font) = font.as_ref().filter(|_| !layout_drawn) {
                    if let Some(text) = obj.to_text(doc, &output) {
                        draw_pdf_text(&mut surface, font, &text, opts.fonts == PdfFonts::TextAsCurves);
                    }
                }
                if layout_drawn && let Some(links) = &links {
                    annotations.extend(links.annotations(doc, placement, obj));
                }
                draw_decorations(&mut surface, &text_matrix, &decorations, false, obj.opacity, &output);
                if let Some((shape, stroke)) = text_stroke {
                    draw_shape_stroke(&mut surface, doc, obj, &shape, &stroke, opts.outline_strokes);
                }
                if let (Some(font), Some(notes)) = (font.as_ref(), note_text.get(&obj.id)) {
                    for note in notes {
                        draw_pdf_text(&mut surface, font, note, opts.fonts == PdfFonts::TextAsCurves);
                    }
                }
                if mask {
                    surface.pop();
                }
                if pixel_mask {
                    surface.pop();
                }
                if range {
                    surface.pop();
                }
                if blended {
                    surface.pop();
                    surface.pop();
                }
                if k08.is_some() {
                    surface.pop();
                }
                if crop.is_some() {
                    surface.pop();
                }
                effect_drawn |= draw_effect_images(&mut surface, doc, effect_bounds, &above, effect_pixels_per_unit, image_transform)?;
                effects_rasterised |= effect_drawn;
            }

            if effects_rasterised && !warnings.iter().any(|w| w == "warning.pdf.effects_rasterised") {
                warnings.push("warning.pdf.effects_rasterised".to_string());
            }
            if page_clip.is_some() {
                surface.pop();
            }
            let crop = crop_marks.then(|| trim_mark_segments(
                [x, y, x + w, y + h], [x - left, y - top, x + w + right, y + h + bottom], 1.0 / scale,
            ));
            if crop.is_some() || !folds.is_empty() {
                draw_print_marks(&mut surface, crop.into_iter().flatten().chain(folds), 1.0 / scale, opts.uses_cmyk());
            }

            surface.pop();
            surface.pop();
            surface.finish();
        }
        for annotation in annotations {
            page.add_annotation(annotation);
        }
        page.finish();
    }

    let mut bytes_out = krilla_doc
        .finish()
        .map_err(|_| CommandError::new("error.export.write_failed"))?;
    if let Some(profile) = intent_profile {
        bytes_out = with_output_intent(bytes_out, &profile.name, &profile.bytes).ok_or_else(|| CommandError::new("error.export.write_failed"))?;
    }
    if opts.uses_cmyk() {
        bytes_out = fix_cmyk_calculator_ranges(bytes_out)
            .map_err(|_| CommandError::new("error.export.write_failed"))?;
    }

    Ok((bytes_out, warnings))
}

fn fix_cmyk_calculator_ranges(bytes: Vec<u8>) -> lopdf::Result<Vec<u8>> {
    use lopdf::{Document, IncrementalDocument, Object};

    let pdf = Document::load_mem(&bytes)?;
    let mut updates = std::collections::BTreeSet::new();
    for object in pdf.objects.values() {
        let Ok(shading) = object.as_dict() else { continue };
        if shading.get(b"ShadingType").and_then(Object::as_i64).ok() != Some(1) {
            continue;
        }
        let Ok(space) = shading.get_deref(b"ColorSpace", &pdf) else { continue };
        let cmyk = match space {
            Object::Name(name) => name == b"DeviceCMYK",
            Object::Array(parts) if parts.len() == 2 && parts[0].as_name().ok() == Some(b"ICCBased") => {
                pdf.dereference(&parts[1]).and_then(|(_, profile)| profile.as_stream())
                    .and_then(|profile| profile.dict.get(b"N")).and_then(Object::as_i64).ok() == Some(4)
            }
            _ => false,
        };
        if !cmyk {
            continue;
        }
        let id = shading.get(b"Function")?.as_reference()?;
        let function = pdf.get_object(id)?.as_stream()?;
        // Krilla 0.8.2 emits four ink values but declares only three output ranges.
        if function.dict.get(b"FunctionType")?.as_i64()? == 4
            && function.dict.get(b"Range")?.as_array()?.len() == 6
        {
            updates.insert(id);
        }
    }
    if updates.is_empty() {
        return Ok(bytes);
    }
    let mut incremental = IncrementalDocument::create_from(bytes, pdf);
    for id in updates {
        incremental.opt_clone_object_to_new_document(id)?;
        let function = incremental.new_document.get_object_mut(id)?.as_stream_mut()?;
        let range = function.dict.get_mut(b"Range")?.as_array_mut()?;
        range.extend([Object::Integer(0), Object::Integer(1)]);
    }
    let mut output = Vec::new();
    incremental.save_to(&mut output)?;
    Ok(output)
}

fn draw_vector_ink(surface: &mut PageBuilder<'_>, doc: &Document, ink: Vec<dtool_core::scene::Prim>, output: &OutputColour) {
    for prim in ink {
        let dtool_core::scene::Prim::Fill { contours, paint, opacity, .. } = prim else { continue };
        let rings: Vec<_> = contours.into_iter().map(|contour| contour.into_iter().map(|point| point.map(f64::from)).collect()).collect();
        let Some(path) = region_path(&rings) else { continue };
        let Some(mut fill) = crate::pdf_paint::fill_for_output(surface, &paint, opacity, output, doc) else { continue };
        fill.rule = FillRule::EvenOdd;
        surface.set_stroke(None);
        surface.set_fill(Some(fill));
        surface.draw_path(&path);
    }
}

fn push_blend_range_mask(surface: &mut PageBuilder<'_>, mask: crate::blend_range_mask::RangeMask) -> bool {
    let [x, y, w, h] = mask.bounds;
    let Some(extent) = krilla::geom::Size::from_wh(w, h) else { return false; };
    let pixels = mask.alpha.into_iter().flat_map(|alpha| [255, 255, 255, alpha]).collect();
    let mut stream = surface.stream_builder();
    {
        let mut layer = stream.surface();
        layer.push_transform(&Transform::from_translate(x, y));
        layer.draw_image(Image::from_rgba8(pixels, mask.size[0], mask.size[1]), extent);
        layer.pop();
        layer.finish();
    }
    let stream = stream.finish();
    surface.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Alpha));
    true
}

fn push_pixel_mask(surface: &mut PageBuilder<'_>, doc: &Document, object: &Object) -> bool {
    let Some(mask) = dtool_core::raster_source::compositing_mask(doc, object) else { return false };
    let [w, h] = object.size.map(f32::abs);
    let Some(extent) = krilla::geom::Size::from_wh(w, h) else { return false };
    let transform = pdf_paint_transform(object);
    let [a, b, c, d, e, f] = transform.as_coeffs().map(|v| v as f32);
    if dtool_core::geometry::invert_matrix(&[a, b, c, d, e, f]).is_none() {
        return false;
    }
    if let Some(outline) = doc.layer_masks.iter().find(|layer| layer.object == object.id)
        .and_then(|layer| layer.vector_outline.as_ref())
    {
        let to_world = transform * kurbo::Affine::scale_non_uniform(f64::from(w), f64::from(h));
        if let Some(path) = pdf_bez_path(&(to_world * outline.to_kurbo())) {
            surface.push_clip_path(&path, &FillRule::EvenOdd);
            return true;
        }
    }
    if mask.coverage.as_slice() == [255] {
        // A single opaque sample covers exactly the transformed object rectangle.
        // Use its path so PDF image sampling cannot move the clipping edge.
        let quad = dtool_core::geometry::object_world_quad(object).map(|point| point.map(f64::from));
        if let Some(path) = region_path(&[quad.to_vec()]) {
            surface.push_clip_path(&path, &FillRule::NonZero);
            return true;
        }
    }
    let pixels = mask.coverage.iter().flat_map(|alpha| [255, 255, 255, *alpha]).collect();
    let image = Image::from_rgba8(pixels, mask.width, mask.height);
    let mut stream = surface.stream_builder();
    {
        let mut mask_surface = stream.surface();
        mask_surface.push_transform(&Transform::from_row(a, b, c, d, e, f));
        mask_surface.draw_image(image, extent);
        mask_surface.pop();
        mask_surface.finish();
    }
    let stream = stream.finish();
    surface.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Alpha));
    true
}

fn push_object_transparency(surface: &mut PageBuilder<'_>, doc: &Document, object: &Object, bounds: [f32; 4], pixels_per_unit: f32) -> bool {
    use dtool_core::paint::{GradientKind, Paint, Transparency};
    let transform = pdf_paint_transform(object);
    if let Transparency::InverseBitmap(bitmap) = &object.transparency {
        let Some(([width, height], pixels)) = crate::effect_image::bitmap_mask_tile(doc, bitmap) else { return false };
        let image = Image::from_rgba8(pixels, width, height);
        let matrix = (transform * kurbo::Affine::new(bitmap.matrix)).as_coeffs();
        return crate::pdf_paint::push_bitmap_transparency(surface, image, matrix, bounds);
    }
    let Transparency::Gradient { kind, stops, geom } = &object.transparency else { return false };
    let push_gradient = |surface: &mut PageBuilder<'_>, stops: &[dtool_core::paint::GradientStop], geom: &dtool_core::paint::GradientGeom| {
        let mut geom = *geom;
        if matches!(kind, GradientKind::Radial | GradientKind::Conical) {
            geom.ratio = 1.0;
        }
        let matrix = (transform * kurbo::Affine::new(geom.to_matrix())).as_coeffs();
        crate::pdf_paint::push_gradient_transparency(surface, kind, stops, &geom, matrix, bounds)
    };
    if *kind != GradientKind::Conical && geom.spread.is_pad() {
        return push_gradient(surface, stops, geom);
    }
    if *kind == GradientKind::Conical && !stops.is_empty() {
        let [dx, dy, ..] = geom.to_matrix();
        let axis = dx.hypot(dy);
        let determinant = transform.determinant();
        if axis.is_finite() && axis > 1e-14 && determinant.is_finite() && determinant != 0.0
            && push_gradient(surface, stops, geom)
        {
            return true;
        }
    }
    if *kind != GradientKind::Conical && transform.determinant() != 0.0 {
        let paint = Paint::Gradient { kind: *kind, stops: stops.clone(), geom: *geom };
        let inverse = transform.inverse().as_coeffs().map(|value| value as f32);
        let [x, y, w, h] = dtool_core::scene::clip_bounds(bounds, &inverse);
        // PDF calculator shadings with repeated stops can fail in a grayscale mask.
        // A bounded list of ordinary axial or radial stops remains vector data.
        if let std::borrow::Cow::Owned(Paint::Gradient { stops, geom, .. }) =
            crate::pdf_paint::bounded_gradient(&paint, [x, y, x + w, y + h])
        {
            return push_gradient(surface, &stops, &geom);
        }
    }
    let [x, y, w, h] = bounds;
    let Some(extent) = krilla::geom::Size::from_wh(w, h) else { return false };
    let Some(([width, height], pixels)) = crate::effect_image::transparency_pixels(object, bounds, pixels_per_unit) else {
        return false;
    };
    let mut stream = surface.stream_builder();
    {
        let mut mask = stream.surface();
        mask.push_transform(&Transform::from_translate(x, y));
        mask.draw_image(Image::from_rgba8(pixels, width, height), extent);
        mask.pop();
        mask.finish();
    }
    let stream = stream.finish();
    surface.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Alpha));
    true
}

fn pdf_paint_transform(object: &dtool_core::Object) -> kurbo::Affine {
    let [x, y] = object.pos.map(f64::from);
    kurbo::Affine::new(dtool_core::geometry::object_matrix(object).map(f64::from))
        * kurbo::Affine::translate((x, y))
}

fn transform_pdf_paint(paint: &dtool_core::paint::Paint, matrix: [f64; 6]) -> Option<dtool_core::paint::Paint> {
    use dtool_core::paint::Paint;
    match paint {
        Paint::Gradient { kind, stops, geom } => Some(Paint::Gradient { kind: *kind, stops: stops.clone(), geom: geom.transform(matrix) }),
        Paint::Bitmap(bitmap) => {
            let mut bitmap = bitmap.clone();
            bitmap.matrix = (kurbo::Affine::new(matrix) * kurbo::Affine::new(bitmap.matrix)).as_coeffs();
            Some(Paint::Bitmap(bitmap))
        }
        _ => None,
    }
}

fn draw_decorations(surface: &mut PageBuilder<'_>, matrix: &[f32; 6], decorations: &[dtool_core::text_lines::TextDecoration], under_text: bool, opacity: f32, output: &OutputColour) {
    for decoration in decorations.iter().filter(|d| d.under_text == under_text) {
        let Some(colour) = crate::paint_colour(&decoration.paint) else { continue };
        let [x, y, w, h] = decoration.rect;
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
            continue;
        }
        let mut builder = PathBuilder::new();
        let [first, rest @ ..] = dtool_core::geometry::rect_quad(decoration.rect, matrix);
        builder.move_to(first[0], first[1]);
        for [x, y] in rest {
            builder.line_to(x, y);
        }
        builder.close();
        let Some(path) = builder.finish() else { continue };
        surface.set_stroke(None);
        surface.set_fill(Some(Fill { paint: output.colour(colour).into(), opacity: pdf_opacity(colour, opacity), rule: FillRule::NonZero }));
        surface.draw_path(&path);
    }
}

fn add_line(page: &mut PageBuilder<'_>, from: (f32, f32), to: (f32, f32)) {
    let mut builder = PathBuilder::new();
    builder.move_to(from.0, from.1);
    builder.line_to(to.0, to.1);
    if let Some(path) = builder.finish() {
        page.draw_path(&path);
    }
}

fn draw_raster_layer(
    surface: &mut PageBuilder<'_>, doc: &Document, obj: &dtool_core::Object, output: &OutputColour,
    transform: Option<&RgbToCmyk>, outline_strokes: bool,
) -> Result<bool, CommandError> {
    use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix, object_world_quad};
    use dtool_core::paint::PaintOrder;

    let Some(([width, height], pixels)) = dtool_core::raster_source::rgba(doc, obj) else {
        return Ok(false);
    };
    let world = object_matrix(obj);
    let frame = dtool_core::commands::picture_frame::frame_geometry(doc, obj);
    let [a, b, _, d] = frame
        .map(|geometry| geometry.content_corners().map(|point| apply_matrix(&world, point)))
        .unwrap_or_else(|| object_world_quad(obj));
    let placement = [b[0] - a[0], b[1] - a[1], d[0] - a[0], d[1] - a[1], a[0], a[1]];
    if invert_matrix(&placement).is_none() {
        return Ok(false);
    }
    let image = pixel_image(pixels, width as u32, height as u32, transform)?;
    let outline = frame.map(|geometry| geometry.outline().into_iter().map(|point| apply_matrix(&world, point)).collect())
        .unwrap_or_else(|| object_world_quad(obj).to_vec());
    let mut stroke_path = kurbo::BezPath::new();
    if let Some((&first, rest)) = outline.split_first() {
        let [x, y] = first.map(f64::from);
        stroke_path.move_to((x, y));
        for point in rest {
            let [x, y] = point.map(f64::from);
            stroke_path.line_to((x, y));
        }
        stroke_path.close_path();
    }
    let Some(outline) = pdf_bez_path(&stroke_path) else {
        return Ok(false);
    };
    let fill = crate::pdf_paint::fill_for_output(surface, &obj.fill, obj.opacity, output, doc);
    let stroke = crate::pdf_paint::stroke_for_output(surface, &obj.stroke, obj.opacity, output, doc);
    let stroke_under = matches!(obj.paint_order, PaintOrder::StrokeUnder);
    let draw_border = |surface: &mut PageBuilder<'_>, stroke: &Stroke| {
        if (outline_strokes || !obj.stroke.pressure.is_empty() || obj.stroke.cap == dtool_core::paint::Cap::RoundInset)
            && draw_outlined_stroke(surface, &stroke_path, stroke, &obj.stroke, dtool_core::boolean::DEFAULT_TOLERANCE)
        {
            return;
        }
        surface.set_fill(None);
        surface.set_stroke(Some(stroke.clone()));
        surface.draw_path(&outline);
    };
    if let Some(stroke) = stroke.as_ref().filter(|_| stroke_under) {
        draw_border(surface, stroke);
    }
    if let Some(fill) = fill {
        surface.set_stroke(None);
        surface.set_fill(Some(fill));
        surface.draw_path(&outline);
    }
    if frame.is_some() {
        surface.push_clip_path(&outline, &FillRule::NonZero);
    }
    let opacity = NormalizedF32::new(obj.opacity.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE);
    surface.push_opacity(opacity);
    let [a, b, c, d, x, y] = placement;
    surface.push_transform(&Transform::from_row(a, b, c, d, x, y));
    surface.draw_image(image, krilla::geom::Size::from_wh(1.0, 1.0).expect("unit square"));
    surface.pop();
    surface.pop();
    if frame.is_some() {
        surface.pop();
    }
    if let Some(stroke) = stroke.as_ref().filter(|_| !stroke_under) {
        draw_border(surface, stroke);
    }
    Ok(true)
}

fn draw_solid_blur(
    surface: &mut PageBuilder<'_>, doc: &Document, object: &Object, source: &dtool_core::scene::Prim,
    bounds: [f32; 4], pixels_per_unit: f32, output: &OutputColour,
) -> bool {
    use dtool_core::scene::Prim;

    let Prim::Fill { contours, paint, .. } = source else { return false };
    let rings: Vec<_> = contours.iter()
        .map(|ring| ring.iter().map(|point| point.map(f64::from)).collect())
        .collect();
    let Some(path) = region_path(&rings) else { return false };
    let Some(mut fill) = crate::pdf_paint::fill_for_output(surface, paint, 1.0, output, doc) else { return false };
    let Some(layer) = dtool_core::layer_style::raster::blurred_base(source, &object.effects) else { return false };
    let Some(size) = crate::effect_image::size(bounds, std::slice::from_ref(&layer), pixels_per_unit) else { return false };
    let Some(raster) = crate::effect_image::rgba(doc, bounds, size, &layer) else { return false };
    let mask = crate::blend_range_mask::RangeMask {
        bounds: raster.bounds,
        size: raster.size,
        alpha: raster.pixels.chunks_exact(4).map(|pixel| pixel[3]).collect(),
    };
    if !push_blend_range_mask(surface, mask) {
        return false;
    }
    // Extend the flat paint under the soft edge. The mask alone controls its opacity.
    let stroke = Stroke {
        paint: fill.paint.clone(),
        opacity: NormalizedF32::ONE,
        width: 2.0 * (layer.raster_padding() + 2.0),
        line_join: krilla::paint::LineJoin::Round,
        ..Stroke::default()
    };
    fill.opacity = NormalizedF32::ONE;
    fill.rule = FillRule::EvenOdd;
    surface.set_fill(Some(fill));
    surface.set_stroke(Some(stroke));
    surface.draw_path(&path);
    surface.set_stroke(None);
    surface.pop();
    true
}

fn draw_effect_images(
    surface: &mut PageBuilder<'_>, doc: &Document, bounds: Option<[f32; 4]>,
    layers: &[dtool_core::layer_style::raster::EffectLayer], pixels_per_unit: f32, transform: Option<&RgbToCmyk>,
) -> Result<bool, CommandError> {
    let Some(bounds) = bounds else { return Ok(false) };
    let Some([w, h]) = crate::effect_image::size(bounds, layers, pixels_per_unit) else { return Ok(false) };
    let mut drawn = false;
    for layer in layers {
        let Some(raster) = crate::effect_image::rgba(doc, bounds, [w, h], layer) else { continue };
        drawn |= draw_effect_image(surface, raster, layer.blend, transform)?;
    }
    Ok(drawn)
}

fn draw_effect_image(
    surface: &mut PageBuilder<'_>, raster: crate::effect_image::EffectImage,
    blend: BlendMode, transform: Option<&RgbToCmyk>,
) -> Result<bool, CommandError> {
    let [x, y, width, height] = raster.bounds;
    let Some(size) = krilla::geom::Size::from_wh(width, height) else { return Ok(false) };
    let image = pixel_image(raster.pixels, raster.size[0], raster.size[1], transform)?;
    surface.push_transform(&Transform::from_translate(x, y));
    surface.push_blend_mode(effect_blend(blend));
    surface.draw_image(image, size);
    surface.pop();
    surface.pop();
    Ok(true)
}

fn draw_path_brush(
    surface: &mut PageBuilder<'_>, doc: &Document, ink: &dtool_core::path_brush::Ink,
    object: &dtool_core::Object, pixels_per_unit: f32, transform: Option<&RgbToCmyk>,
) -> Result<bool, CommandError> {
    let [x, y, width, height] = ink.bounds;
    if let Some((bounds, layer)) = crate::effect_image::brush_blur(object, ink) {
        return draw_effect_images(surface, doc, Some(bounds), &[layer], pixels_per_unit, transform);
    }
    let Some(extent) = krilla::geom::Size::from_wh(width, height) else { return Ok(false); };
    let size = ink.pixel_size(pixels_per_unit);
    let image = pixel_image(ink.render(size), size[0] as u32, size[1] as u32, transform)?;
    surface.push_transform(&Transform::from_translate(x, y));
    surface.push_opacity(NormalizedF32::new(object.opacity.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE));
    surface.draw_image(image, extent);
    surface.pop();
    surface.pop();
    Ok(false)
}

fn effect_blend(mode: dtool_core::blend::BlendMode) -> krilla::blend::BlendMode {
    use dtool_core::blend::BlendMode as D;
    use krilla::blend::BlendMode as K;
    match mode {
        D::Normal | D::PassThrough => K::Normal,
        D::Multiply => K::Multiply, D::Screen => K::Screen, D::Overlay => K::Overlay,
        D::Darken => K::Darken, D::Lighten => K::Lighten, D::ColorDodge => K::ColorDodge,
        D::ColorBurn => K::ColorBurn, D::HardLight => K::HardLight, D::SoftLight => K::SoftLight,
        D::Difference => K::Difference, D::Exclusion => K::Exclusion, D::Hue => K::Hue,
        D::Saturation => K::Saturation, D::Color => K::Color, D::Luminosity => K::Luminosity,
    }
}

fn draw_outlined_stroke(
    surface: &mut PageBuilder<'_>, path: &kurbo::BezPath, stroke: &Stroke, style: &StrokeStyle,
    tolerance: f64,
) -> bool {
    let geometry = dtool_core::outline::styled_stroke_path_with_tolerance(path, style, tolerance)
        .or_else(|| {
            if tolerance < dtool_core::boolean::DEFAULT_TOLERANCE {
                dtool_core::outline::styled_stroke_path(path, style)
            } else {
                None
            }
        });
    let Some((outline, even_odd)) = geometry else { return false; };
    if outline.elements().is_empty() { return true; }
    let rule = if even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
    let Some(path) = pdf_bez_path(&outline) else { return false; };
    surface.set_stroke(None);
    surface.set_fill(Some(Fill { paint: stroke.paint.clone(), opacity: stroke.opacity, rule }));
    surface.draw_path(&path);
    true
}

fn draw_shape_stroke(surface: &mut PageBuilder<'_>, doc: &Document, obj: &dtool_core::Object, shape: &PdfShape, stroke: &Stroke, outline: bool) {
    if !((outline || !obj.stroke.pressure.is_empty() || obj.stroke.cap == dtool_core::paint::Cap::RoundInset)
        && draw_outlined_stroke(surface, &shape.outline, stroke, &obj.stroke, shape.tolerance))
    {
        surface.set_fill(None);
        surface.set_stroke(Some(stroke.clone()));
        surface.draw_path(&shape.path);
    }
    draw_arrowheads(surface, doc, obj, stroke.clone(), outline);
}

fn pdf_bez_path(path: &kurbo::BezPath) -> Option<krilla::geom::Path> {
    let mut builder = PathBuilder::new();
    for element in path.elements() {
        match *element {
            kurbo::PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            kurbo::PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            kurbo::PathEl::QuadTo(c, p) => builder.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32),
            kurbo::PathEl::CurveTo(a, b, p) => builder.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32),
            kurbo::PathEl::ClosePath => builder.close(),
        }
    }
    builder.finish()
}

fn draw_arrowheads(surface: &mut PageBuilder, doc: &Document, object: &dtool_core::Object, mut stroke: Stroke, outline: bool) {
    stroke.dash = None;
    for head in crate::arrowheads_for(Some(doc), object) {
        let Some((&[x, y], rest)) = head.points.split_first() else { continue; };
        let mut geometry = kurbo::BezPath::new();
        geometry.move_to((f64::from(x), f64::from(y)));
        for &[x, y] in rest {
            geometry.line_to((f64::from(x), f64::from(y)));
        }
        if head.filled {
            geometry.close_path();
            surface.set_stroke(None);
            surface.set_fill(Some(Fill { paint: stroke.paint.clone(), opacity: stroke.opacity, rule: FillRule::NonZero }));
        } else {
            stroke.width = head.line_width;
            let style = StrokeStyle {
                width: head.line_width,
                cap: object.stroke.cap,
                join: object.stroke.join.clone(),
                ..Default::default()
            };
            if outline && draw_outlined_stroke(surface, &geometry, &stroke, &style, dtool_core::boolean::DEFAULT_TOLERANCE) {
                continue;
            }
            surface.set_fill(None);
            surface.set_stroke(Some(stroke.clone()));
        }
        if let Some(path) = pdf_bez_path(&geometry) {
            surface.draw_path(&path);
        }
    }
}

fn region_path(rings: &[Vec<[f64; 2]>]) -> Option<krilla::geom::Path> {
    let mut builder = PathBuilder::new();
    let mut any = false;
    for ring in rings.iter().filter(|r| r.len() >= 3) {
        builder.move_to(ring[0][0] as f32, ring[0][1] as f32);
        for p in &ring[1..] {
            builder.line_to(p[0] as f32, p[1] as f32);
        }
        builder.close();
        any = true;
    }
    if any { builder.finish() } else { None }
}

fn crop_clip_path([x, y, w, h]: [f32; 4]) -> Option<krilla::geom::Path> {
    let mut builder = PathBuilder::new();
    builder.move_to(x, y);
    builder.line_to(x + w, y);
    builder.line_to(x + w, y + h);
    builder.line_to(x, y + h);
    builder.close();
    builder.finish()
}

fn draw_dimension(surface: &mut PageBuilder<'_>, obj: &dtool_core::Object, geometry: &dtool_core::outline::DimensionGeometry, font: Option<&Font>, output: &OutputColour, opts: &PdfOptions) {
    let colour = crate::stroke_colour(&obj.stroke)
        .or_else(|| crate::paint_colour(&obj.fill))
        .unwrap_or(dtool_core::paint::Colour::new(0, 0, 0, 255));
    let width = if obj.stroke.width.is_finite() && obj.stroke.width > 0.0 { obj.stroke.width } else { 1.0 };
    let stroke = Stroke {
        paint: output.colour(colour).into(),
        width,
        opacity: pdf_opacity(colour, obj.opacity),
        ..Stroke::default()
    };
    let mut path = kurbo::BezPath::new();
    for &[start, end] in std::iter::once(&geometry.line).chain(geometry.extensions.iter()).chain(geometry.ticks.iter()) {
        path.move_to((start[0], start[1]));
        path.line_to((end[0], end[1]));
    }
    let style = StrokeStyle { width, ..Default::default() };
    if !(opts.outline_strokes && draw_outlined_stroke(surface, &path, &stroke, &style, dtool_core::boolean::DEFAULT_TOLERANCE))
        && let Some(path) = pdf_bez_path(&path)
    {
        surface.set_fill(None);
        surface.set_stroke(Some(stroke));
        surface.draw_path(&path);
    }
    if let Some(font) = font {
        let size = dtool_core::outline::DIMENSION_LABEL_SIZE as f32;
        let text = PdfText {
            text: geometry.label.clone(),
            x: geometry.label_at[0] as f32 - geometry.label.chars().count() as f32 * size * 0.25,
            y: geometry.label_at[1] as f32 + size * 0.35,
            size,
            fill: Fill {
                paint: output.colour(colour).into(),
                opacity: pdf_opacity(colour, obj.opacity),
                rule: FillRule::NonZero,
            },
        };
        draw_pdf_text(surface, font, &text, opts.fonts == PdfFonts::TextAsCurves);
    }
}

fn pdf_name_text(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').map(|c| if matches!(c, '(' | ')' | '\\') { format!("\\{c}") } else { c.to_string() }).collect()
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

fn trailer_ref(trailer: &str, key: &str) -> Option<String> {
    let at = trailer.find(key)? + key.len();
    let rest = trailer[at..].trim_start();
    let end = rest.find('R')?;
    Some(rest[..=end].trim().to_string())
}

pub fn with_output_intent(bytes: Vec<u8>, name: &str, profile: &[u8]) -> Option<Vec<u8>> {
    let trailer_at = find_last(&bytes, b"trailer")?;
    let startxref_at = find_last(&bytes, b"startxref")?;
    let trailer = String::from_utf8_lossy(&bytes[trailer_at..startxref_at]).to_string();
    let prev: usize = String::from_utf8_lossy(&bytes[startxref_at + 9..]).split_whitespace().next()?.parse().ok()?;
    let size: u32 = trailer.split("/Size").nth(1)?.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    let root = trailer_ref(&trailer, "/Root")?;
    let root_num: u32 = root.split_whitespace().next()?.parse().ok()?;
    let header = format!("\n{root_num} 0 obj");
    let obj_at = find_last(&bytes[..trailer_at], header.as_bytes())? + header.len();
    let end_at = obj_at + bytes[obj_at..].windows(6).position(|window| window == b"endobj")?;
    let catalog = String::from_utf8_lossy(&bytes[obj_at..end_at]).trim().to_string();
    if catalog.contains("/OutputIntents") || !catalog.ends_with(">>") {
        return None;
    }
    let (icc_num, intent_num) = (size, size + 1);
    let label = pdf_name_text(name);
    let mut out = bytes;
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut offsets = Vec::new();
    offsets.push((icc_num, out.len()));
    out.extend_from_slice(format!("{icc_num} 0 obj\n<< /N 4 /Length {} >>\nstream\n", profile.len()).as_bytes());
    out.extend_from_slice(profile);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    offsets.push((intent_num, out.len()));
    out.extend_from_slice(format!("{intent_num} 0 obj\n<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier ({label}) /Info ({label}) /DestOutputProfile {icc_num} 0 R >>\nendobj\n").as_bytes());
    offsets.push((root_num, out.len()));
    out.extend_from_slice(format!("{root_num} 0 obj\n{} /OutputIntents [{intent_num} 0 R] >>\nendobj\n", &catalog[..catalog.len() - 2]).as_bytes());
    let xref_at = out.len();
    let mut xref = String::from("xref\n");
    offsets.sort_by_key(|(num, _)| *num);
    for (num, offset) in &offsets {
        xref.push_str(&format!("{num} 1\n{offset:010} 00000 n\r\n"));
    }
    let mut extra = String::new();
    if let Some(info) = trailer_ref(&trailer, "/Info") {
        extra.push_str(&format!(" /Info {info}"));
    }
    if let Some(id_at) = trailer.find("/ID") {
        if let Some(close) = trailer[id_at..].find(']') {
            extra.push_str(&format!(" {}", &trailer[id_at..=id_at + close]));
        }
    }
    xref.push_str(&format!("trailer\n<< /Size {} /Root {root}{extra} /Prev {prev} >>\nstartxref\n{xref_at}\n%%EOF\n", intent_num + 1));
    out.extend_from_slice(xref.as_bytes());
    Some(out)
}

fn draw_print_marks(
    page: &mut PageBuilder<'_>, marks: impl IntoIterator<Item = [[f32; 2]; 2]>, unit: f32, cmyk: bool,
) {
    let paint = if cmyk { cmyk::Color::new(0, 0, 0, 255).into() } else { rgb::Color::black().into() };
    page.set_fill(None);
    page.set_stroke(Some(Stroke { paint, width: 0.25 * unit, ..Stroke::default() }));
    for [start, end] in marks {
        add_line(page, (start[0], start[1]), (end[0], end[1]));
    }
}

/// Crop marks in document coordinates. A point on paper corresponds to `unit` document units.
pub fn trim_mark_segments(
    [left, top, right, bottom]: [f32; 4],
    [bleed_left, bleed_top, bleed_right, bleed_bottom]: [f32; 4],
    unit: f32,
) -> [[[f32; 2]; 2]; 8] {
    let (gap, length) = (3.0 * unit, 10.0 * unit);
    let horizontal = |y| [
        [[bleed_left - gap - length, y], [bleed_left - gap, y]],
        [[bleed_right + gap, y], [bleed_right + gap + length, y]],
    ];
    let vertical = |x| [
        [[x, bleed_top - gap - length], [x, bleed_top - gap]],
        [[x, bleed_bottom + gap], [x, bleed_bottom + gap + length]],
    ];
    let [a, b] = horizontal(top);
    let [c, d] = horizontal(bottom);
    let [e, f] = vertical(left);
    let [g, h] = vertical(right);
    [a, b, c, d, e, f, g, h]
}



/// Short fold ticks outside the sheet's bleed. Coordinates share the page trim's axes.
pub fn fold_mark_segments(
    doc: &Document, pages: &[usize], trim: [f32; 4], bleed: [f32; 4], unit: f32,
) -> Vec<[[f32; 2]; 2]> {

    let mut marks = Vec::new();
    if !pages.iter().any(|&index| doc.pages.get(index).is_some_and(|page| page.fold.is_some())) {
        return marks;
    }
    let rects = dtool_core::scene::page_rects(doc);
    let gap = unit;
    let length = 2.5 * unit;
    for &index in pages {
        let Some(fold) = doc.pages.get(index).and_then(|page| page.fold) else { continue };
        let Some(&rect) = rects.get(index) else { continue };
        let Some(segments) = fold.segments(rect) else { continue };
        let axis = fold.axis.index();
        let perpendicular = 1 - axis;
        for [start, end] in segments {
            let position = start[axis];
            for (side, endpoint) in [start, end].into_iter().enumerate() {
                // Only extend an edge that reaches the outside of the sheet on this axis.
                // A page at a spine must not mark the opposite page's outer edge.
                if (endpoint[perpendicular] - trim[perpendicular + side * 2]).abs() > unit * 0.01 {
                    continue;
                }
                let edge = bleed[perpendicular + side * 2];
                let direction = if side == 0 { -1.0 } else { 1.0 };
                let mut a = start;
                let mut b = start;
                a[axis] = position;
                b[axis] = position;
                a[perpendicular] = edge + direction * gap;
                b[perpendicular] = edge + direction * (gap + length);
                marks.push([a, b]);
            }
        }
    }
    marks
}
