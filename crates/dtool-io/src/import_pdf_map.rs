use std::{cell::OnceCell, collections::HashMap, rc::Rc};

use dtool_core::colour::Colour as ManagedColour;
use dtool_core::blend::BlendMode;
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId};
use dtool_core::boolean::FillRule;
use dtool_core::object::{ClipRole, Object, ObjectId, ObjectKind, VectorCrop};
use dtool_core::paint::{Cap, Colour, Join, Paint, StrokeStyle, paint_to_json_value};
use dtool_core::resources::{ImageFormat, ImageResource};
use kurbo::{Affine, Shape};
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as PdfDocument, Object as PdfObject, ObjectId as PdfId, Stream};
use serde_json::json;
use skrifa::{MetadataProvider, outline::OutlinePen};

use crate::import_pdf::{ImportAs, ImportOptions};
use crate::import_report::{ImportReport, LossKind};
use crate::{IoError, IoErrorKind};

mod icc;
mod image;
mod shading;
mod text_outline;

type Matrix = [f64; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_FORM_DEPTH: usize = 8;

fn invalid() -> IoError {
    IoError::new(IoErrorKind::InvalidFormat)
}

fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn apply(m: &Matrix, p: [f64; 2]) -> [f64; 2] {
    [
        m[0] * p[0] + m[2] * p[1] + m[4],
        m[1] * p[0] + m[3] * p[1] + m[5],
    ]
}

fn scale_of(m: &Matrix) -> f64 {
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
}

fn axis_aligned(m: &Matrix) -> bool {
    (m[1].abs() < 1e-9 && m[2].abs() < 1e-9) || (m[0].abs() < 1e-9 && m[3].abs() < 1e-9)
}

fn num(object: &PdfObject) -> Option<f64> {
    match object {
        PdfObject::Integer(v) => Some(*v as f64),
        PdfObject::Real(v) if v.is_finite() => Some(*v as f64),
        _ => None,
    }
}

fn pdf_blend_mode(object: &PdfObject) -> Option<BlendMode> {
    Some(match object.as_name().ok()? {
        b"Normal" | b"Compatible" => BlendMode::Normal,
        b"Multiply" => BlendMode::Multiply,
        b"Screen" => BlendMode::Screen,
        b"Overlay" => BlendMode::Overlay,
        b"Darken" => BlendMode::Darken,
        b"Lighten" => BlendMode::Lighten,
        b"ColorDodge" => BlendMode::ColorDodge,
        b"ColorBurn" => BlendMode::ColorBurn,
        b"HardLight" => BlendMode::HardLight,
        b"SoftLight" => BlendMode::SoftLight,
        b"Difference" => BlendMode::Difference,
        b"Exclusion" => BlendMode::Exclusion,
        b"Hue" => BlendMode::Hue,
        b"Saturation" => BlendMode::Saturation,
        b"Color" => BlendMode::Color,
        b"Luminosity" => BlendMode::Luminosity,
        _ => return None,
    })
}

fn nums(objects: &[PdfObject]) -> Vec<f64> {
    objects.iter().filter_map(num).collect()
}

fn matrix_from(objects: &[PdfObject]) -> Option<Matrix> {
    let values = nums(objects);
    values.try_into().ok()
}

fn deref<'a>(pdf: &'a PdfDocument, object: &'a PdfObject) -> &'a PdfObject {
    match object {
        PdfObject::Reference(id) => pdf.get_object(*id).unwrap_or(object),
        _ => object,
    }
}

fn dict_of<'a>(pdf: &'a PdfDocument, object: &'a PdfObject) -> Option<&'a Dictionary> {
    match deref(pdf, object) {
        PdfObject::Dictionary(dict) => Some(dict),
        PdfObject::Stream(stream) => Some(&stream.dict),
        _ => None,
    }
}

fn name_of(pdf: &PdfDocument, dict: &Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key)
        .ok()
        .map(|o| deref(pdf, o))
        .and_then(|o| o.as_name().ok())
        .map(|name| String::from_utf8_lossy(name).into_owned())
}

fn inherited<'a>(pdf: &'a PdfDocument, page: PdfId, key: &[u8]) -> Option<&'a PdfObject> {
    let mut dict = pdf.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(object) = dict.get(key) {
            return Some(deref(pdf, object));
        }
        dict = dict_of(pdf, dict.get(b"Parent").ok()?)?;
    }
    None
}

fn rect_of(pdf: &PdfDocument, object: &PdfObject) -> Option<[f64; 4]> {
    let items = deref(pdf, object).as_array().ok()?;
    let values: Vec<_> = items
        .iter()
        .map(|o| num(deref(pdf, o)))
        .collect::<Option<_>>()?;
    let [x0, y0, x1, y1]: [f64; 4] = values.try_into().ok()?;
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

#[derive(Clone, Copy)]
pub(super) struct PageGeometry {
    media: [f64; 4],
    rotate: i32,
    unit: f64,
}

impl PageGeometry {
    pub(super) fn size(self) -> [f32; 2] {
        let w = ((self.media[2] - self.media[0]) * self.unit) as f32;
        let h = ((self.media[3] - self.media[1]) * self.unit) as f32;
        if self.rotate % 180 == 0 {
            [w, h]
        } else {
            [h, w]
        }
    }

    fn map(self, p: [f64; 2]) -> [f64; 2] {
        let x = p[0] - self.media[0];
        let y = p[1] - self.media[1];
        let w = self.media[2] - self.media[0];
        let h = self.media[3] - self.media[1];
        let p = match self.rotate {
            90 => [y, x],
            180 => [w - x, y],
            270 => [h - y, w - x],
            _ => [x, h - y],
        };
        [p[0] * self.unit, p[1] * self.unit]
    }
}

pub(super) fn page_geometry(pdf: &PdfDocument, page: PdfId) -> Result<PageGeometry, IoError> {
    let mut media =
        rect_of(pdf, inherited(pdf, page, b"MediaBox").ok_or_else(invalid)?).ok_or_else(invalid)?;
    if let Some(crop) = inherited(pdf, page, b"CropBox") {
        let crop = rect_of(pdf, crop).ok_or_else(invalid)?;
        media = [
            media[0].max(crop[0]),
            media[1].max(crop[1]),
            media[2].min(crop[2]),
            media[3].min(crop[3]),
        ];
    }
    let rotate = match inherited(pdf, page, b"Rotate") {
        Some(value) => num(value).ok_or_else(invalid)?,
        None => 0.0,
    };
    if rotate % 90.0 != 0.0 {
        return Err(invalid());
    }
    let unit = match pdf
        .get_dictionary(page)
        .map_err(|_| invalid())?
        .get(b"UserUnit")
    {
        Ok(value) => num(deref(pdf, value)).ok_or_else(invalid)?,
        Err(_) => 1.0,
    };
    let geometry = PageGeometry {
        media,
        rotate: rotate.rem_euclid(360.0) as i32,
        unit,
    };
    if unit <= 0.0 || geometry.size().iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(invalid());
    }
    Ok(geometry)
}

fn stream_bytes(stream: &Stream) -> Option<Vec<u8>> {
    if stream.dict.has(b"Filter") {
        let filters = stream.filters().ok()?;
        if let Ok(params) = stream.dict.get(b"DecodeParms") {
            if !matches!(params, PdfObject::Dictionary(_) | PdfObject::Null) {
                return None;
            }
        }
        if filters.is_empty() {
            return (stream.content.len() <= MAX_BYTES).then(|| stream.content.clone());
        }
        stream.decompressed_content_with_limit(MAX_BYTES).ok()
    } else if stream.content.len() <= MAX_BYTES {
        Some(stream.content.clone())
    } else {
        None
    }
}

fn append_content(
    pdf: &PdfDocument,
    object: &PdfObject,
    out: &mut Vec<u8>,
    depth: usize,
) -> Result<(), IoError> {
    if depth > 64 {
        return Err(invalid());
    }
    match deref(pdf, object) {
        PdfObject::Null => {}
        PdfObject::Array(items) => {
            for item in items {
                append_content(pdf, item, out, depth + 1)?;
            }
        }
        PdfObject::Stream(stream) => {
            let bytes = stream_bytes(stream)
                .ok_or_else(|| IoError::new(IoErrorKind::UnsupportedProfile))?;
            if out.len().saturating_add(bytes.len()).saturating_add(1) > MAX_BYTES {
                return Err(IoError::new(IoErrorKind::Other));
            }
            out.extend(bytes);
            out.push(b'\n');
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

fn managed(colour: ManagedColour) -> Paint {
    Paint::Solid {
        colour: Colour::from_managed(colour),
    }
}
fn black() -> Paint {
    managed(ManagedColour::grey(0.0, 1.0))
}

#[derive(Clone, Copy, PartialEq)]
enum Space {
    Grey,
    Rgb,
    Cmyk,
    Icc { index: usize, channels: usize },
    Pattern,
    Other,
}

impl Space {
    fn channels(self) -> Option<usize> {
        match self {
            Self::Grey => Some(1),
            Self::Rgb => Some(3),
            Self::Cmyk => Some(4),
            Self::Icc { channels, .. } => Some(channels),
            Self::Pattern | Self::Other => None,
        }
    }
}

struct FontInfo {
    family: String,
    style: String,
    decode: Dictionary,
    unicode_map: Option<lopdf::Encoding<'static>>,
    two_byte: bool,
    first_char: i64,
    widths: Vec<Option<f64>>,
    cid_widths: Vec<CidWidthRange>,
    default_width: f64,
    vertical: bool,
    reference_only: bool,
    embedded_cid: Option<Dictionary>,
    embedded: OnceCell<Option<text_outline::EmbeddedFont>>,
}

struct CidWidthRange {
    start: u16,
    end: u16,
    widths: Vec<f64>,
}

impl FontInfo {
    fn width(&self, code: &[u8]) -> f64 {
        let width = if self.two_byte {
            let Ok(bytes) = <[u8; 2]>::try_from(code) else { return self.default_width };
            let cid = u16::from_be_bytes(bytes);
            let next = self.cid_widths.partition_point(|range| range.start <= cid);
            next.checked_sub(1).and_then(|index| {
                let range = &self.cid_widths[index];
                (cid <= range.end).then(|| {
                    let offset = usize::from(cid - range.start);
                    range.widths[offset.min(range.widths.len() - 1)]
                })
            })
        } else {
            i64::from(code[0]).checked_sub(self.first_char)
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| self.widths.get(index).copied().flatten())
        };
        width.unwrap_or(self.default_width)
    }
}

fn cid_widths(pdf: &PdfDocument, font: &Dictionary) -> Option<(f64, Vec<CidWidthRange>)> {
    let cid = text_outline::descendant(pdf, font)?;
    let default_width = match cid.get(b"DW") {
        Ok(value) => num(deref(pdf, value)).filter(|width| *width >= 0.0)?,
        Err(_) => 1000.0,
    };
    let mut ranges = Vec::new();
    if let Ok(value) = cid.get(b"W") {
        let mut entries = deref(pdf, value).as_array().ok()?.iter().map(|item| deref(pdf, item));
        let mut covered = 0usize;
        while let Some(start) = entries.next() {
            let start = u16::try_from(start.as_i64().ok()?).ok()?;
            let spec = entries.next()?;
            let (end, widths) = if let Ok(list) = spec.as_array() {
                let last_offset = u16::try_from(list.len().checked_sub(1)?).ok()?;
                let end = start.checked_add(last_offset)?;
                covered += list.len();
                if covered > 65_536 { return None; }
                let widths = list.iter().map(|item| num(deref(pdf, item)).filter(|width| *width >= 0.0))
                    .collect::<Option<Vec<_>>>()?;
                (end, widths)
            } else {
                let end = u16::try_from(spec.as_i64().ok()?).ok()?;
                covered += usize::from(end.checked_sub(start)?) + 1;
                if covered > 65_536 { return None; }
                let width = num(entries.next()?).filter(|width| *width >= 0.0)?;
                (end, vec![width])
            };
            ranges.push(CidWidthRange { start, end, widths });
        }
        ranges.sort_unstable_by_key(|range| range.start);
        if ranges.windows(2).any(|pair| pair[0].end >= pair[1].start) { return None; }
    }
    Some((default_width, ranges))
}

#[derive(Clone)]
struct GState {
    ctm: Matrix,
    fill: Paint,
    stroke: Paint,
    fill_space: Space,
    stroke_space: Space,
    line_width: f64,
    dash: Vec<f64>,
    dash_offset: f64,
    cap: Cap,
    join: Join,
    fill_alpha: f32,
    stroke_alpha: f32,
    blend_mode: BlendMode,
    clip: Option<[f64; 4]>,
    parent: Option<ObjectId>,
    font: Option<Rc<FontInfo>>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: i64,
    text_knockout: bool,
}

impl GState {
    fn new(parent: ObjectId) -> Self {
        Self {
            ctm: IDENTITY,
            fill: black(),
            stroke: black(),
            fill_space: Space::Grey,
            stroke_space: Space::Grey,
            line_width: 1.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            cap: Cap::Butt,
            join: Join::Miter { limit: 10.0 },
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            blend_mode: BlendMode::Normal,
            clip: None,
            parent: Some(parent),
            font: None,
            font_size: 12.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
            text_knockout: true,
        }
    }
}

#[derive(Clone)]
struct SubPath {
    points: Vec<PathPoint>,
    closed: bool,
    rect: bool,
}

#[derive(Clone)]
struct Run {
    text: String,
    origin: [f64; 2],
    size: f64,
    advance: f64,
    family: String,
    style: String,
    fill: Paint,
    opacity: f32,
    blend_mode: BlendMode,
    parent: Option<ObjectId>,
}

struct OutlinedRun {
    path: Vec<SubPath>,
    glyph_ends: Vec<usize>,
    gs: GState,
    fills: bool,
    strokes: bool,
    translucent: bool,
    name: Option<String>,
}

#[derive(Default)]
struct TextBlock {
    runs: Vec<Run>,
    outlines: Vec<OutlinedRun>,
    outline_points: usize,
    outline_budget_exceeded: bool,
    clip: Vec<SubPath>,
    clip_points: usize,
    clip_failed: bool,
}

impl TextBlock {
    fn add_clip(&mut self, path: &[SubPath]) {
        if self.clip_failed { return; }
        let point_count = path.iter().map(|sub| sub.points.len()).sum::<usize>();
        self.clip_points += point_count;
        if self.clip_points > 65_536 {
            self.clip.clear();
            self.clip_failed = true;
        } else {
            self.clip.extend_from_slice(path);
        }
    }
}

pub(super) struct PdfMapper<'a> {
    pdf: &'a PdfDocument,
    doc: &'a mut Document,
    report: &'a mut ImportReport,
    opts: &'a ImportOptions,
    geometry: PageGeometry,
    origin: [f32; 2],
    point_id: u64,
    paths: u32,
    operations: usize,
    image_bytes: usize,
    profiles: icc::Profiles,
    cmyk_output_space: Option<Space>,
    fonts: HashMap<PdfId, Rc<FontInfo>>,
}

impl<'a> PdfMapper<'a> {
    pub(super) fn new(
        pdf: &'a PdfDocument,
        doc: &'a mut Document,
        report: &'a mut ImportReport,
        opts: &'a ImportOptions,
    ) -> Self {
        let mut profiles = icc::Profiles::default();
        let (cmyk_output_space, has_output_intent) = icc::output_intent(pdf, &mut profiles);
        if has_output_intent && cmyk_output_space.is_none() {
            report.add_loss(
                LossKind::Icc,
                1,
                false,
                "No usable embedded PDF/X CMYK output profile; source device colours retained",
            );
        }
        Self {
            pdf,
            doc,
            report,
            opts,
            geometry: PageGeometry {
                media: [0.0; 4],
                rotate: 0,
                unit: 1.0,
            },
            origin: [0.0; 2],
            point_id: 0,
            paths: 0,
            operations: 0,
            image_bytes: 0,
            profiles,
            cmyk_output_space,
            fonts: HashMap::new(),
        }
    }

    fn loss(&mut self, kind: LossKind, detail: impl Into<String>) {
        let detail = detail.into();
        let required = kind != LossKind::Annotation;
        if let Some(loss) = self
            .report
            .losses
            .iter_mut()
            .find(|l| l.kind == kind && l.detail == detail && l.required == required)
        {
            loss.count += 1;
        } else {
            self.report.add_loss(kind, 1, required, detail);
        }
    }

    fn to_doc(&self, p: [f64; 2]) -> [f64; 2] {
        let p = self.geometry.map(p);
        [p[0] + self.origin[0] as f64, p[1] + self.origin[1] as f64]
    }

    fn user_to_doc(&self, gs: &GState, p: [f64; 2]) -> [f64; 2] {
        self.to_doc(apply(&gs.ctm, p))
    }
    fn next_point(&mut self) -> PointId {
        self.point_id += 1;
        PointId(self.point_id)
    }

    pub(super) fn map_page(
        &mut self,
        page: PdfId,
        index: usize,
        origin: [f32; 2],
        geometry: PageGeometry,
        structured: bool,
    ) -> Result<(), IoError> {
        self.geometry = geometry;
        self.origin = origin;
        let dict = self.pdf.get_dictionary(page).map_err(|_| invalid())?;
        let mut bytes = Vec::new();
        if let Ok(contents) = dict.get(b"Contents") {
            append_content(self.pdf, contents, &mut bytes, 0)?;
        }
        let content = Content::decode_strict(&bytes).map_err(|_| invalid())?;
        let label = if self.opts.import_as == ImportAs::Pages {
            "Page"
        } else {
            "Artboard"
        };
        let artboard = structured && self.opts.import_as == ImportAs::Artboards;
        let first_object = self.doc.objects.len();
        let page_id = self.doc.pages[index].id;
        if !structured && self.opts.import_as == ImportAs::Artboards {
            self.report.add_loss(
                LossKind::Other,
                1,
                false,
                "artboards represented by page groups; core has no artboard type",
            );
        }
        let layer = self.doc.create_object(Object {
            name: format!("{} {}", label, index + 1),
            pos: origin,
            size: geometry.size(),
            kind: if artboard { ObjectKind::Artboard } else { ObjectKind::Group },
            vector_crop: Some(VectorCrop {
                rect: [0.0, 0.0, geometry.size()[0], geometry.size()[1]],
            }),
            ..Object::default()
        });
        if let Ok(annots) = dict.get(b"Annots") {
            match deref(self.pdf, annots) {
                PdfObject::Array(items) => self.report.add_loss(
                    LossKind::Annotation,
                    items.len() as u32,
                    false,
                    "annotations skipped",
                ),
                _ => self.loss(LossKind::Annotation, "invalid annotations skipped"),
            }
        }
        if dict.has(b"Group") {
            self.loss(LossKind::Blend, "page transparency group flattened");
        }
        let resources = inherited(self.pdf, page, b"Resources")
            .and_then(|o| dict_of(self.pdf, o))
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        self.run(&content.operations, &resources, GState::new(layer), 0);
        if structured {
            for object in &mut self.doc.objects[first_object..] {
                object.page_id = Some(page_id);
            }
        }
        if artboard {
            self.doc.pages[index].artboard_id = Some(layer);
            self.doc.artboard_backing_pages.insert(page_id);
        }
        Ok(())
    }

    fn lookup(&self, resources: &[Dictionary], category: &[u8], name: &[u8]) -> Option<PdfObject> {
        for dict in resources {
            if let Ok(group) = dict.get(category) {
                if let Some(group) = dict_of(self.pdf, group) {
                    if let Ok(object) = group.get(name) {
                        return Some(deref(self.pdf, object).clone());
                    }
                }
            }
        }
        None
    }

    fn run(&mut self, ops: &[Operation], resources: &[Dictionary], mut gs: GState, depth: usize) {
        let mut stack = Vec::new();
        let mut path: Vec<SubPath> = Vec::new();
        let mut current = [0.0; 2];
        let mut sub_start = current;
        let mut pending_clip = None;
        let mut in_text = false;
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        let mut text = TextBlock::default();
        for op in ops {
            self.operations += 1;
            if self.operations > 1_000_000 {
                self.loss(LossKind::Other, "content operation resource limit exceeded");
                break;
            }
            let operands = &op.operands;
            let v = nums(operands);
            match op.operator.as_str() {
                "q" => stack.push(gs.clone()),
                "Q" => match stack.pop() {
                    Some(saved) => gs = saved,
                    None => self.loss(LossKind::Other, "unbalanced graphics state restore"),
                },
                "cm" => match matrix_from(operands) {
                    Some(m) => gs.ctm = mul(&m, &gs.ctm),
                    None => self.loss(LossKind::Other, "invalid transform"),
                },
                "w" => gs.line_width = v.first().copied().unwrap_or(1.0),
                "J" => {
                    gs.cap = match v.first().copied().unwrap_or(0.0) as i64 {
                        1 => Cap::Round,
                        2 => Cap::Square,
                        _ => Cap::Butt,
                    }
                }
                "j" => {
                    gs.join = match v.first().copied().unwrap_or(0.0) as i64 {
                        1 => Join::Round,
                        2 => Join::Bevel,
                        _ => Join::Miter { limit: 10.0 },
                    }
                }
                "M" => {
                    gs.join = Join::Miter {
                        limit: v.first().copied().unwrap_or(10.0) as f32,
                    }
                }
                "d" => self.set_dash_pattern(operands, &mut gs),
                "ri" | "i" => self.loss(LossKind::Icc, "rendering intent or flatness hint ignored"),
                "gs" => self.ext_gstate(operands, resources, &mut gs),
                "g" | "rg" | "k" | "G" | "RG" | "K" => {
                    let space = match op.operator.as_str() {
                        "g" | "G" => Space::Grey,
                        "rg" | "RG" => Space::Rgb,
                        _ => Space::Cmyk,
                    };
                    let space = self.device_space(space, resources);
                    let paint = self.colour_from(space, &v);
                    if op.operator.chars().next().unwrap_or('g').is_lowercase() {
                        gs.fill_space = space;
                        gs.fill = paint;
                    } else {
                        gs.stroke_space = space;
                        gs.stroke = paint;
                    }
                }
                "cs" | "CS" => {
                    let space = operands
                        .first()
                        .map(|o| self.space_of(o, resources))
                        .unwrap_or(Space::Other);
                    let paint = self.colour_from(
                        space,
                        if space == Space::Cmyk {
                            &[0.0, 0.0, 0.0, 1.0]
                        } else {
                            &[0.0, 0.0, 0.0]
                        },
                    );
                    if op.operator == "cs" {
                        gs.fill_space = space;
                        gs.fill = paint;
                    } else {
                        gs.stroke_space = space;
                        gs.stroke = paint;
                    }
                }
                "sc" | "scn" | "SC" | "SCN" => {
                    let fill = op.operator.starts_with('s');
                    let space = if fill { gs.fill_space } else { gs.stroke_space };
                    let paint = if matches!(operands.last(), Some(PdfObject::Name(_))) {
                        self.loss(LossKind::Pattern, "pattern colour dropped");
                        Paint::None
                    } else {
                        self.colour_from(space, &v)
                    };
                    if fill {
                        gs.fill = paint;
                    } else {
                        gs.stroke = paint;
                    }
                }
                "m" if v.len() == 2 => {
                    current = [v[0], v[1]];
                    sub_start = current;
                    let id = self.next_point();
                    path.push(SubPath {
                        points: vec![PathPoint::sharp(id, self.user_to_doc(&gs, current))],
                        closed: false,
                        rect: false,
                    });
                }
                "l" if v.len() == 2 => {
                    current = [v[0], v[1]];
                    let id = self.next_point();
                    if let Some(sub) = path.last_mut() {
                        sub.points
                            .push(PathPoint::sharp(id, self.user_to_doc(&gs, current)));
                    } else {
                        self.loss(LossKind::Other, "line without current path skipped");
                    }
                }
                "c" | "v" | "y" => {
                    let (c1, c2, end) = match (op.operator.as_str(), v.len()) {
                        ("c", 6) => ([v[0], v[1]], [v[2], v[3]], [v[4], v[5]]),
                        ("v", 4) => (current, [v[0], v[1]], [v[2], v[3]]),
                        ("y", 4) => ([v[0], v[1]], [v[2], v[3]], [v[2], v[3]]),
                        _ => {
                            self.loss(LossKind::Other, "invalid curve operands");
                            continue;
                        }
                    };
                    let id = self.next_point();
                    if let Some(sub) = path.last_mut() {
                        if let Some(last) = sub.points.last_mut() {
                            last.handle_out = Some(self.user_to_doc(&gs, c1));
                        }
                        let mut point = PathPoint::sharp(id, self.user_to_doc(&gs, end));
                        point.handle_in = Some(self.user_to_doc(&gs, c2));
                        sub.points.push(point);
                    } else {
                        self.loss(LossKind::Other, "curve without current path skipped");
                    }
                    current = end;
                }
                "h" => {
                    Self::close_last(&mut path);
                    current = sub_start;
                }
                "re" if v.len() == 4 => {
                    let (x, y, w, h) = (v[0], v[1], v[2], v[3]);
                    let mut points = Vec::new();
                    for corner in [[x, y], [x + w, y], [x + w, y + h], [x, y + h]] {
                        let id = self.next_point();
                        points.push(PathPoint::sharp(id, self.user_to_doc(&gs, corner)));
                    }
                    path.push(SubPath {
                        points,
                        closed: true,
                        rect: axis_aligned(&gs.ctm),
                    });
                    current = [x, y];
                    sub_start = current;
                }
                "W" => pending_clip = Some(false),
                "W*" => pending_clip = Some(true),
                "n" | "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" => {
                    let operator = op.operator.as_str();
                    if matches!(operator, "s" | "b" | "b*") {
                        Self::close_last(&mut path);
                    }
                    self.flush_text_block(&mut text);
                    let fill = matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                    let stroke = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*");
                    if (fill || stroke) && !path.is_empty() {
                        self.emit_path(&path, &gs, fill, stroke, operator.ends_with('*'), None);
                    }
                    if let Some(even_odd) = pending_clip.take() {
                        self.apply_clip(&path, even_odd, &mut gs);
                    }
                    path.clear();
                }
                "sh" => {
                    self.flush_text_block(&mut text);
                    if let Some(PdfObject::Name(name)) = operands.first() {
                        self.shade(name, resources, &gs);
                    }
                }
                "BI" | "ID" | "EI" => self.loss(LossKind::Image, "inline image skipped"),
                "Do" => {
                    if in_text {
                        self.flush_text_block(&mut text);
                    }
                    if let Some(PdfObject::Name(name)) = operands.first() {
                        self.draw_xobject(name, resources, &gs, depth);
                    } else {
                        self.loss(LossKind::Other, "invalid XObject operand");
                    }
                }
                "BT" => {
                    self.flush_text_block(&mut text);
                    text = TextBlock::default();
                    in_text = true;
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                "ET" => {
                    self.flush_text_block(&mut text);
                    if text.clip_failed {
                        self.loss(LossKind::Mask, "text clipping could not be reproduced with the embedded font");
                    } else if !text.clip.is_empty() {
                        self.apply_clip(&text.clip, false, &mut gs);
                    }
                    text = TextBlock::default();
                    in_text = false;
                }
                "Tc" => gs.char_spacing = v.first().copied().unwrap_or(0.0),
                "Tw" => gs.word_spacing = v.first().copied().unwrap_or(0.0),
                "Tz" => gs.h_scale = v.first().copied().unwrap_or(100.0) / 100.0,
                "TL" => gs.leading = v.first().copied().unwrap_or(0.0),
                "Ts" => gs.rise = v.first().copied().unwrap_or(0.0),
                "Tr" => gs.render_mode = v.first().copied().unwrap_or(0.0) as i64,
                "Tf" => {
                    gs.font = match operands.first() {
                        Some(PdfObject::Name(name)) => self.font_info(name, resources),
                        _ => None,
                    };
                    if gs.font.is_none() {
                        self.loss(LossKind::Font, "text font resource missing");
                    }
                    gs.font_size = v.last().copied().unwrap_or(12.0);
                }
                "Td" | "TD" if v.len() == 2 => {
                    if op.operator == "TD" {
                        gs.leading = -v[1];
                    }
                    tlm = mul(&[1.0, 0.0, 0.0, 1.0, v[0], v[1]], &tlm);
                    tm = tlm;
                }
                "Tm" => {
                    if let Some(m) = matrix_from(operands) {
                        tm = m;
                        tlm = m;
                    } else {
                        self.loss(LossKind::Other, "invalid text matrix");
                    }
                }
                "T*" => {
                    tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], &tlm);
                    tm = tlm;
                }
                "Tj" | "'" | "\"" | "TJ" => {
                    if !in_text {
                        self.loss(LossKind::Other, "text outside text object skipped");
                        continue;
                    }
                    if op.operator == "'" || op.operator == "\"" {
                        if op.operator == "\"" && v.len() == 2 {
                            gs.word_spacing = v[0];
                            gs.char_spacing = v[1];
                        }
                        tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], &tlm);
                        tm = tlm;
                    }
                    let parts = match operands.last() {
                        Some(PdfObject::Array(items)) if op.operator == "TJ" => items.clone(),
                        Some(object @ PdfObject::String(..)) => vec![object.clone()],
                        _ => Vec::new(),
                    };
                    if let Some(run) = self.show_text(&parts, &gs, &mut tm, &mut text) {
                        self.flush_outlines(&mut text);
                        text.runs.push(run);
                    }
                }
                "BMC" | "BDC" | "EMC" | "MP" | "DP" => self.loss(
                    LossKind::Other,
                    "marked-content semantics or optional visibility not preserved",
                ),
                "BX" | "EX" => {}
                other => self.loss(
                    LossKind::Other,
                    format!("unsupported or malformed operator {}", other),
                ),
            }
        }
        self.flush_text_block(&mut text);
        if in_text || !stack.is_empty() {
            self.loss(LossKind::Other, "unterminated text or graphics state");
        }
    }

    fn close_last(path: &mut [SubPath]) {
        if let Some(sub) = path.last_mut() {
            Self::join_closed_endpoints(&mut sub.points);
            sub.closed = true;
        }
    }

    fn join_closed_endpoints(points: &mut Vec<PathPoint>) {
        if points.len() > 1 && points.first().map(|p| p.pos) == points.last().map(|p| p.pos) {
            if let Some(last) = points.pop() {
                points[0].handle_in = last.handle_in;
            }
        }
    }

    fn colour_from(&mut self, space: Space, values: &[f64]) -> Paint {
        let v = |i: usize| values.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        match space {
            Space::Grey => managed(ManagedColour::grey(v(0) * 100.0, 1.0)),
            Space::Rgb => managed(ManagedColour::rgb(v(0), v(1), v(2), 1.0)),
            Space::Cmyk => managed(ManagedColour::cmyk(
                v(0) * 100.0,
                v(1) * 100.0,
                v(2) * 100.0,
                v(3) * 100.0,
                1.0,
            )),
            Space::Icc { .. } => {
                let conversion = self.profiles.conversion(space).expect("resolved ICC colour space");
                Paint::Solid { colour: icc::colour(conversion.rgb(values)) }
            }
            Space::Pattern => {
                self.loss(LossKind::Pattern, "pattern colour dropped");
                Paint::None
            }
            Space::Other => {
                self.loss(
                    LossKind::Icc,
                    "unsupported colour space approximated as black",
                );
                black()
            }
        }
    }

    fn device_space(&mut self, space: Space, resources: &[Dictionary]) -> Space {
        let key: &[u8] = match space {
            Space::Grey => b"DefaultGray",
            Space::Rgb => b"DefaultRGB",
            Space::Cmyk => b"DefaultCMYK",
            _ => return space,
        };
        if let Some(default) = self.lookup(resources, b"ColorSpace", key) {
            let resolved = self.read_space(&default, resources, false, 1);
            if resolved.channels() == space.channels() { return resolved; }
            self.loss(LossKind::Icc, "invalid default colour space; device colour used");
        }
        if space == Space::Cmyk { self.cmyk_output_space.unwrap_or(space) } else { space }
    }

    fn space_of(&mut self, object: &PdfObject, resources: &[Dictionary]) -> Space {
        self.read_space(object, resources, true, 0)
    }

    fn read_space(&mut self, object: &PdfObject, resources: &[Dictionary], use_default: bool, depth: usize) -> Space {
        if depth >= 8 { return Space::Other; }
        let object = deref(self.pdf, object);
        match object {
            PdfObject::Name(name) => {
                let space = match name.as_slice() {
                b"DeviceGray" | b"G" => Space::Grey,
                b"DeviceRGB" | b"RGB" => Space::Rgb,
                b"DeviceCMYK" | b"CMYK" => Space::Cmyk,
                b"Pattern" => Space::Pattern,
                _ => match self.lookup(resources, b"ColorSpace", name) {
                    Some(object) => return self.read_space(&object, resources, use_default, depth + 1),
                    None => return Space::Other,
                },
                };
                if use_default { self.device_space(space, resources) } else { space }
            }
            PdfObject::Array(items) => {
                match items.first().and_then(|o| o.as_name().ok()).unwrap_or(b"") {
                    b"ICCBased" => {
                        let Some(profile) = items.get(1) else { return Space::Other; };
                        let dict = dict_of(self.pdf, profile);
                        let alternate = match dict
                            .and_then(|d| d.get(b"N").ok())
                            .and_then(|value| num(deref(self.pdf, value)))
                        {
                            Some(1.0) => Space::Grey,
                            Some(3.0) => Space::Rgb,
                            Some(4.0) => Space::Cmyk,
                            _ => Space::Other,
                        };
                        if let Some(space) = alternate.channels()
                            .and_then(|channels| self.profiles.load(self.pdf, profile, channels))
                        {
                            return space;
                        }
                        self.loss(LossKind::Icc, "invalid or unsupported embedded ICC profile; alternate colour used");
                        if let Some(explicit) = dict.and_then(|d| d.get(b"Alternate").ok()) {
                            let resolved = self.read_space(explicit, resources, use_default, depth + 1);
                            if resolved.channels() == alternate.channels() { return resolved; }
                        }
                        if use_default { self.device_space(alternate, resources) } else { alternate }
                    }
                    b"CalGray" => {
                        self.loss(LossKind::Icc, "calibrated grey approximated as device grey");
                        Space::Grey
                    }
                    b"CalRGB" => {
                        self.loss(LossKind::Icc, "calibrated RGB approximated as device RGB");
                        Space::Rgb
                    }
                    b"Pattern" => Space::Pattern,
                    _ => {
                        self.loss(
                            LossKind::Icc,
                            "Lab, indexed or spot colour space unsupported",
                        );
                        Space::Other
                    }
                }
            }
            _ => Space::Other,
        }
    }

    fn set_dash_pattern(&mut self, operands: &[PdfObject], gs: &mut GState) {
        if let [array, phase] = operands {
            let phase = num(deref(self.pdf, phase));
            let dash = deref(self.pdf, array).as_array().ok().and_then(|items| {
                items.iter().map(|entry| num(deref(self.pdf, entry)).filter(|length| *length >= 0.0)).collect::<Option<Vec<_>>>()
            });
            if let (Some(dash), Some(phase)) = (dash, phase) {
                if dash.is_empty() || dash.iter().any(|length| *length > 0.0) {
                    gs.dash_offset = if dash.is_empty() { 0.0 } else { phase };
                    gs.dash = dash;
                    return;
                }
            }
        }
        self.loss(LossKind::Other, "invalid stroke dash pattern ignored");
    }

    fn ext_gstate(&mut self, operands: &[PdfObject], resources: &[Dictionary], gs: &mut GState) {
        let Some(PdfObject::Name(name)) = operands.first() else {
            self.loss(LossKind::Blend, "graphics state name missing");
            return;
        };
        let Some(object) = self.lookup(resources, b"ExtGState", name) else {
            self.loss(LossKind::Blend, "graphics state resource missing");
            return;
        };
        let Some(dict) = dict_of(self.pdf, &object).cloned() else {
            self.loss(LossKind::Blend, "invalid graphics state");
            return;
        };
        for (key, value) in dict.iter() {
            match key.as_slice() {
                b"Type" => {}
                b"ca" => {
                    gs.fill_alpha =
                        num(deref(self.pdf, value)).unwrap_or(1.0).clamp(0.0, 1.0) as f32
                }
                b"CA" => {
                    gs.stroke_alpha =
                        num(deref(self.pdf, value)).unwrap_or(1.0).clamp(0.0, 1.0) as f32
                }
                b"TK" => {
                    if let PdfObject::Boolean(value) = deref(self.pdf, value) {
                        gs.text_knockout = *value;
                    } else {
                        self.loss(LossKind::Blend, "invalid text knockout flag ignored");
                    }
                }
                b"LW" => gs.line_width = num(deref(self.pdf, value)).unwrap_or(1.0),
                b"D" => {
                    if let Ok(operands) = deref(self.pdf, value).as_array() {
                        self.set_dash_pattern(operands, gs);
                    } else {
                        self.loss(LossKind::Other, "invalid stroke dash pattern ignored");
                    }
                }
                b"BM" => {
                    let value = deref(self.pdf, value);
                    let mode = match value {
                        PdfObject::Array(modes) => modes.iter()
                            .find_map(|entry| pdf_blend_mode(deref(self.pdf, entry))),
                        _ => pdf_blend_mode(value),
                    };
                    gs.blend_mode = mode.unwrap_or(BlendMode::Normal);
                    if mode.is_none() && !matches!(value, PdfObject::Array(_)) {
                        self.loss(LossKind::Blend, "unsupported PDF blend mode replaced by Normal");
                    }
                }
                b"SMask" if matches!(deref(self.pdf, value), PdfObject::Name(n) if n == b"None") => {
                }
                b"SMask" => self.loss(LossKind::Mask, "graphics state soft mask dropped"),
                _ => self.loss(
                    LossKind::Blend,
                    format!(
                        "graphics state {} approximated or ignored",
                        String::from_utf8_lossy(key)
                    ),
                ),
            }
        }
    }

    fn bounds(points: impl IntoIterator<Item = [f64; 2]>) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in points {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
        b
    }

    fn path_bounds(path: &[SubPath]) -> [f64; 4] {
        Self::bounds(path.iter().flat_map(|sub| {
            let b = Contour {
                points: sub.points.clone(),
                closed: sub.closed,
            }
            .bounds();
            [[b[0] as f64, b[1] as f64], [b[2] as f64, b[3] as f64]]
        }))
    }

    fn apply_clip(&mut self, path: &[SubPath], even_odd: bool, gs: &mut GState) {
        self.apply_group_clip(path, even_odd, gs, "clip".into(), 1.0, BlendMode::PassThrough);
    }

    fn apply_group_clip(
        &mut self, path: &[SubPath], even_odd: bool, gs: &mut GState,
        name: String, opacity: f32, blend_mode: BlendMode,
    ) {
        if path.is_empty() {
            self.loss(LossKind::Mask, "empty clipping path ignored");
            return;
        }
        let b = Self::path_bounds(path);
        if b.iter().any(|v| !v.is_finite()) {
            self.loss(LossKind::Mask, "invalid clip skipped");
            return;
        }
        let rect = match gs.clip {
            Some(prev) => [
                prev[0].max(b[0]),
                prev[1].max(b[1]),
                prev[2].min(b[2]),
                prev[3].min(b[3]),
            ],
            None => b,
        };
        let size = [
            (rect[2] - rect[0]).max(0.0) as f32,
            (rect[3] - rect[1]).max(0.0) as f32,
        ];
        let id = self.doc.create_object(Object {
            name,
            pos: [rect[0] as f32, rect[1] as f32],
            size,
            kind: ObjectKind::Group,
            opacity,
            blend_mode,
            parent_id: gs.parent,
            ..Object::default()
        });
        // The mask clips every descendant, including groups inside a rectangular clip.
        // Keeping all subpaths together preserves their shared winding rule.
        self.doc.create_object(Object {
            name: "clip path".into(),
            kind: ObjectKind::Curve(Self::curve_from_path(path, [b[0], b[1]], true)),
            pos: [b[0] as f32, b[1] as f32],
            size: [(b[2] - b[0]) as f32, (b[3] - b[1]) as f32],
            parent_id: Some(id),
            clip_role: ClipRole::Mask,
            fill: Paint::Solid { colour: Colour::new(255, 255, 255, 255) },
            fill_rule: if even_odd { FillRule::EvenOdd } else { FillRule::NonZero },
            ..Object::default()
        });
        gs.clip = Some(rect);
        gs.parent = Some(id);
    }

    fn curve_from_path(path: &[SubPath], origin: [f64; 2], close: bool) -> Curve {
        let local = |p: [f64; 2]| [p[0] - origin[0], p[1] - origin[1]];
        let mut contours = path.iter().map(|sub| {
            let mut points = sub.points.iter().map(|p| PathPoint {
                pos: local(p.pos),
                handle_in: p.handle_in.map(local),
                handle_out: p.handle_out.map(local),
                ..p.clone()
            }).collect();
            let closed = sub.closed || close;
            if close && !sub.closed {
                Self::join_closed_endpoints(&mut points);
            }
            Contour { points, closed }
        });
        Curve::from_contour_and_holes(contours.next().unwrap_or_default(), contours.collect())
    }

    fn emit_path(
        &mut self,
        path: &[SubPath],
        gs: &GState,
        fill: bool,
        stroke: bool,
        even_odd: bool,
        name_override: Option<String>,
    ) {
        if (!fill || matches!(gs.fill, Paint::None))
            && (!stroke || matches!(gs.stroke, Paint::None))
        {
            return;
        }
        let b = Self::path_bounds(path);
        if b.iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
        {
            self.loss(LossKind::Other, "path with invalid geometry skipped");
            return;
        }
        let scale = scale_of(&gs.ctm) * self.geometry.unit;
        if stroke {
            if gs.line_width == 0.0 {
                self.loss(
                    LossKind::Other,
                    "device hairline stroke approximated as 0.25 points",
                );
            }
            let a = gs.ctm;
            if ((a[0] * a[0] + a[1] * a[1]) - (a[2] * a[2] + a[3] * a[3])).abs() > 1e-6
                || (a[0] * a[2] + a[1] * a[3]).abs() > 1e-6
            {
                self.loss(
                    LossKind::Other,
                    "non-uniform transformed stroke width approximated",
                );
            }
        }
        let opacity = if fill && stroke {
            if !gs.blend_mode.is_normal() && gs.fill_alpha > 0.0
                && gs.stroke_alpha > 0.0 && gs.stroke_alpha < 1.0
            {
                self.loss(LossKind::Blend, "blended fill and translucent stroke overlap approximated");
            }
            1.0
        } else if fill {
            gs.fill_alpha
        } else {
            gs.stroke_alpha
        };
        let mut fill_paint = if fill { gs.fill.clone() } else { Paint::None };
        let mut stroke_paint = if stroke {
            gs.stroke.clone()
        } else {
            Paint::None
        };
        if fill && stroke {
            if let Paint::Solid { colour } = &mut fill_paint {
                colour.a = (gs.fill_alpha * 255.0).round() as u8;
            }
            if let Paint::Solid { colour } = &mut stroke_paint {
                colour.a = (gs.stroke_alpha * 255.0).round() as u8;
            }
        }
        let stroke_style = StrokeStyle {
            paint: stroke_paint,
            width: if stroke {
                (gs.line_width.max(0.0) * scale).max(if gs.line_width == 0.0 { 0.25 } else { 0.0 })
                    as f32
            } else {
                0.0
            },
            dash: gs.dash.iter().map(|v| (v * scale) as f32).collect(),
            dash_offset: (gs.dash_offset * scale) as f32,
            cap: gs.cap.clone(),
            join: gs.join.clone(),
            ..StrokeStyle::default()
        };
        let (kind, name) = if path.len() == 1 && path[0].rect {
            (
                ObjectKind::Shape,
                format!(
                    "rect_{}_{}",
                    (b[0] - self.origin[0] as f64).round() as i64,
                    (b[1] - self.origin[1] as f64).round() as i64
                ),
            )
        } else {
            if fill && stroke && path.iter().any(|sub| !sub.closed) {
                self.loss(
                    LossKind::Other,
                    "open filled-and-stroked path closed for core; closing stroke may differ",
                );
            }
            self.paths += 1;
            (
                ObjectKind::Curve(Self::curve_from_path(path, [b[0], b[1]], fill)),
                format!("path_{}", self.paths),
            )
        };
        self.doc.create_object(Object {
            name: name_override.unwrap_or(name),
            pos: [b[0] as f32, b[1] as f32],
            size: [(b[2] - b[0]) as f32, (b[3] - b[1]) as f32],
            fill: fill_paint,
            fill_rule: if fill && !even_odd { FillRule::NonZero } else { FillRule::EvenOdd },
            stroke: stroke_style,
            opacity,
            blend_mode: gs.blend_mode,
            kind,
            parent_id: gs.parent,
            ..Object::default()
        });
        self.report.objects += 1;
    }

    fn draw_xobject(&mut self, name: &[u8], resources: &[Dictionary], gs: &GState, depth: usize) {
        let Some(PdfObject::Stream(stream)) = self.lookup(resources, b"XObject", name) else {
            self.loss(
                LossKind::Image,
                format!("XObject missing: {}", String::from_utf8_lossy(name)),
            );
            return;
        };
        match name_of(self.pdf, &stream.dict, b"Subtype").as_deref() {
            Some("Image") => self.draw_image(name, &stream, resources, gs),
            Some("Form") => {
                if depth >= MAX_FORM_DEPTH {
                    self.loss(LossKind::Other, "form nesting resource limit exceeded");
                    return;
                }
                let Some(bytes) = stream_bytes(&stream) else {
                    self.loss(
                        LossKind::Other,
                        "form content codec or resource limit unsupported",
                    );
                    return;
                };
                let Ok(content) = Content::decode_strict(&bytes) else {
                    self.loss(LossKind::Other, "form content could not be parsed");
                    return;
                };
                let mut inner = gs.clone();
                if let Ok(matrix) = stream.dict.get(b"Matrix") {
                    match deref(self.pdf, matrix)
                        .as_array()
                        .ok()
                        .and_then(|items| matrix_from(items))
                    {
                        Some(m) => inner.ctm = mul(&m, &inner.ctm),
                        None => {
                            self.loss(LossKind::Other, "invalid form matrix");
                            return;
                        }
                    }
                }
                let form_resources = stream.dict.get(b"Resources").ok()
                    .and_then(|o| dict_of(self.pdf, o)).cloned()
                    .map(|dict| vec![dict])
                    .unwrap_or_else(|| resources.to_vec());
                let transparency = stream.dict.get(b"Group").ok().and_then(|group| {
                    self.form_transparency(group, &form_resources)
                });
                if let Some(bbox) = stream
                    .dict
                    .get(b"BBox")
                    .ok()
                    .and_then(|o| rect_of(self.pdf, o))
                {
                    let mut points = Vec::new();
                    for p in [
                        [bbox[0], bbox[1]],
                        [bbox[2], bbox[1]],
                        [bbox[2], bbox[3]],
                        [bbox[0], bbox[3]],
                    ] {
                        let id = self.next_point();
                        points.push(PathPoint::sharp(id, self.user_to_doc(&inner, p)));
                    }
                    let clip = SubPath {
                        points,
                        closed: true,
                        rect: axis_aligned(&inner.ctm),
                    };
                    if transparency.is_some() {
                        self.apply_group_clip(
                            &[clip], false, &mut inner,
                            String::from_utf8_lossy(name).into_owned(), gs.fill_alpha,
                            gs.blend_mode,
                        );
                        // A transparency group uses the calling alpha only when compositing its result.
                        inner.fill_alpha = 1.0;
                        inner.stroke_alpha = 1.0;
                        inner.blend_mode = BlendMode::Normal;
                    } else {
                        self.apply_clip(&[clip], false, &mut inner);
                    }
                } else {
                    self.loss(LossKind::Mask, "form bounding box missing or invalid");
                }
                let group_id = inner.parent;
                let start = self.doc.objects.len();
                self.run(&content.operations, &form_resources, inner, depth + 1);
                if transparency == Some(false) && group_id != gs.parent
                    && self.doc.objects[start..].iter().any(|object| !object.blend_mode.is_normal())
                {
                    if gs.blend_mode.is_normal() {
                        if let Some(group) = group_id.and_then(|id| self.doc.find_object_mut(id.0)) {
                            group.blend_mode = BlendMode::PassThrough;
                        }
                    } else {
                        self.loss(LossKind::Blend, "non-isolated form with both internal and external blend modes approximated");
                    }
                }
            }
            _ => self.loss(LossKind::Other, "unsupported XObject subtype skipped"),
        }
    }

    fn form_transparency(&mut self, object: &PdfObject, resources: &[Dictionary]) -> Option<bool> {
        let group = dict_of(self.pdf, object);
        let Some(group) = group.filter(|dict| {
            name_of(self.pdf, dict, b"S").as_deref() == Some("Transparency")
        }) else {
            self.loss(LossKind::Blend, "unknown form group attributes ignored");
            return None;
        };
        if let Ok(knockout) = group.get(b"K") {
            if !matches!(deref(self.pdf, knockout), PdfObject::Boolean(false)) {
                self.loss(LossKind::Blend, "form group knockout approximated as normal compositing");
            }
        }
        if let Ok(space) = group.get(b"CS") {
            if !matches!(self.space_of(space, resources), Space::Rgb) {
                self.loss(LossKind::Icc, "form group blending colour space approximated as RGB");
            }
        }
        Some(group.get(b"I").is_ok_and(|value| {
            matches!(deref(self.pdf, value), PdfObject::Boolean(true))
        }))
    }

    fn image_pixels(
        &mut self,
        stream: &Stream,
        resources: &[Dictionary],
    ) -> Option<(u32, u32, Vec<u8>)> {
        let dict = &stream.dict;
        let Some([width, height]) = image::dimensions(self.pdf, dict) else {
            self.loss(LossKind::Image, "invalid image dimensions; image skipped");
            return None;
        };
        let length = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if length.is_none_or(|n| n > MAX_BYTES) {
            self.loss(
                LossKind::Image,
                "image pixel resource limit exceeded; image skipped",
            );
            return None;
        }
        let length = length.unwrap();
        if self.image_bytes.saturating_add(length) > MAX_BYTES * 2 {
            self.loss(
                LossKind::Image,
                "document image resource limit exceeded; image skipped",
            );
            return None;
        }
        if matches!(
            dict.get(b"ImageMask").ok().map(|o| deref(self.pdf, o)),
            Some(PdfObject::Boolean(true))
        ) {
            self.loss(LossKind::Mask, "stencil image skipped");
            return None;
        }
        if dict
            .get(b"BitsPerComponent")
            .ok()
            .and_then(|o| num(deref(self.pdf, o)))
            != Some(8.0)
        {
            self.loss(
                LossKind::Image,
                "only 8 bit image samples supported; image skipped",
            );
            return None;
        }
        let space = dict
            .get(b"ColorSpace")
            .ok()
            .map(|o| self.space_of(o, resources))
            .unwrap_or(Space::Other);
        let Some(channels) = space.channels() else {
            self.loss(LossKind::Image, "image colour space unsupported; image skipped");
            return None;
        };
        let Some(bytes) = image::samples(self.pdf, stream, [width, height], channels) else {
            let filter = dict
                .get(b"Filter")
                .map(|o| format!("{:?}", o))
                .unwrap_or_default();
            self.loss(
                LossKind::Image,
                format!(
                    "image codec or decompression limit unsupported {}; image skipped",
                    filter
                ),
            );
            return None;
        };
        let count = width as usize * height as usize;
        if bytes.len() != count * channels {
            self.loss(
                LossKind::Image,
                "image sample length or predictor unsupported; image skipped",
            );
            return None;
        }
        let mask = image::mask(self.pdf, dict, [width, height], &bytes, channels);
        if mask.is_none() && [b"SMask".as_slice(), b"Mask"].iter().any(|key| {
            dict.get(key).ok().is_some_and(|object| {
                let value = deref(self.pdf, object);
                !matches!(value, PdfObject::Null) && !matches!(value, PdfObject::Name(name) if name == b"None")
            })
        }) {
            self.loss(LossKind::Mask, "invalid or unsupported image mask dropped");
        }
        let conversion = self.profiles.conversion(space);
        let decode = match dict.get(b"Decode") {
            Ok(object) => match deref(self.pdf, object).as_array().ok().map(|a| nums(a)) {
                Some(values) if values.len() == channels * 2 => values,
                _ => {
                    self.loss(LossKind::Image, "invalid image Decode array; image skipped");
                    return None;
                }
            },
            Err(_) => conversion.map_or_else(
                || (0..channels).flat_map(|_| [0.0, 1.0]).collect(),
                |conversion| conversion.input_range(channels),
            ),
        };
        let sample = |pixel: usize, channel: usize| {
            let t = f64::from(bytes[pixel * channels + channel]) / 255.0;
            let value = decode[channel * 2] + (decode[channel * 2 + 1] - decode[channel * 2]) * t;
            if let Some((mask, matte)) = mask.as_ref().and_then(|mask| Some((mask, mask.matte.as_ref()?))) {
                let alpha = f64::from(mask.alpha[pixel]);
                if alpha > 0.0 { matte[channel] + (value - matte[channel]) / alpha } else { 0.0 }
            } else {
                value
            }
        };
        let mut rgba = Vec::with_capacity(count * 4);
        if let Some(conversion) = conversion {
            conversion.append_image(count, sample, &mut rgba);
        } else {
            for pixel in 0..count {
                let sample = |i| sample(pixel, i).clamp(0.0, 1.0);
                let ch = |v: f64| (v * 255.0).round() as u8;
                let rgb = match space {
                    Space::Grey => [ch(sample(0)); 3],
                    Space::Cmyk => [
                        ch((1.0 - sample(0)) * (1.0 - sample(3))),
                        ch((1.0 - sample(1)) * (1.0 - sample(3))),
                        ch((1.0 - sample(2)) * (1.0 - sample(3))),
                    ],
                    _ => [ch(sample(0)), ch(sample(1)), ch(sample(2))],
                };
                rgba.extend([rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        if space == Space::Cmyk {
            self.loss(
                LossKind::Icc,
                "CMYK image converted to RGB preview without an ICC transform",
            );
        }
        if let Some(mask) = mask {
            for (pixel, alpha) in rgba.chunks_exact_mut(4).zip(mask.alpha) {
                pixel[3] = (alpha * 255.0).round() as u8;
            }
        }
        if dict.has(b"SMaskInData") {
            self.loss(LossKind::Mask, "embedded image alpha unsupported");
        }
        self.image_bytes += length;
        Some((width, height, rgba))
    }

    fn draw_image(&mut self, name: &[u8], stream: &Stream, resources: &[Dictionary], gs: &GState) {
        let Some((width, height, rgba)) = self.image_pixels(stream, resources) else {
            return;
        };
        let corners =
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]].map(|p| self.user_to_doc(gs, p));
        let b = Self::bounds(corners);
        if b.iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
            || b[2] <= b[0]
            || b[3] <= b[1]
        {
            self.loss(LossKind::Image, "invalid image placement skipped");
            return;
        }
        if corners[1][0] <= corners[0][0]
            || corners[1][1] != corners[0][1]
            || corners[3][0] != corners[0][0]
            || corners[3][1] >= corners[0][1]
        {
            self.loss(
                LossKind::Image,
                "rotated, mirrored or skewed image placed by its bounds",
            );
        }
        let name = format!("image_{}", String::from_utf8_lossy(name));
        let (format, data) = if rgba.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            use ::image::ImageEncoder;
            let mut png = Vec::new();
            if ::image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&rgba, width, height, ::image::ExtendedColorType::Rgba8).is_err()
            {
                self.loss(LossKind::Image, "could not encode image transparency; image skipped");
                return;
            }
            (ImageFormat::Png, png)
        } else {
            (ImageFormat::Bmp, encode_bmp(width, height, &rgba))
        };
        let mut resource = ImageResource::new_embedded(format, [width, height], self.doc.dpi, data);
        resource.name = name.clone();
        resource.colour_space = "RGBA".into();
        let resource_id = self.doc.add_image_resource(resource);
        let id = self.doc.create_object(Object {
            name,
            pos: [b[0] as f32, b[1] as f32],
            size: [(b[2] - b[0]) as f32, (b[3] - b[1]) as f32],
            opacity: gs.fill_alpha,
            blend_mode: gs.blend_mode,
            parent_id: gs.parent,
            raster_size: Some((width, height)),
            raster_data: Some(rgba.clone()),
            ..Object::default()
        });
        self.doc
            .place_image(id, resource_id)
            .expect("new image resource");
        self.doc.pixel_layers.insert(
            id,
            dtool_core::pixel::PixelLayer::from_rgba(width, height, &rgba),
        );
        self.report.images += 1;
    }

    fn font_info(&mut self, name: &[u8], resources: &[Dictionary]) -> Option<Rc<FontInfo>> {
        let object = resources.iter().find_map(|dict| {
            let fonts = dict_of(self.pdf, dict.get(b"Font").ok()?)?;
            fonts.get(name).ok()
        })?;
        let reference = object.as_reference().ok();
        if let Some(font) = reference.and_then(|id| self.fonts.get(&id)) {
            return Some(Rc::clone(font));
        }
        let font = dict_of(self.pdf, object)?.clone();
        let font = Rc::new(self.read_font(name, font));
        if let Some(id) = reference {
            self.fonts.insert(id, Rc::clone(&font));
        }
        Some(font)
    }

    fn read_font(&mut self, name: &[u8], font: Dictionary) -> FontInfo {
        let base = name_of(self.pdf, &font, b"BaseFont")
            .unwrap_or_else(|| String::from_utf8_lossy(name).into_owned());
        let mut family = match base.split_once('+') {
            Some((prefix, rest))
                if prefix.len() == 6 && prefix.chars().all(|c| c.is_ascii_uppercase()) =>
            {
                rest.to_owned()
            }
            _ => base,
        };
        let subtype = name_of(self.pdf, &font, b"Subtype").unwrap_or_default();
        let two_byte = subtype == "Type0";
        let encoding = name_of(self.pdf, &font, b"Encoding").unwrap_or_default();
        let mut style = String::new();
        if let Some(replacement) = self.opts.fonts.iter().find(|r| r.missing == family) {
            family = replacement.family.clone();
            style = replacement.style.clone();
            self.loss(
                LossKind::Font,
                "font replacement applied; original glyph appearance not preserved",
            );
        }
        if !matches!(subtype.as_str(), "Type1" | "TrueType" | "Type0") {
            self.loss(
                LossKind::Font,
                format!("font subtype {} approximated", subtype),
            );
        }
        let standard = [
            "Times-Roman",
            "Times-Bold",
            "Times-Italic",
            "Times-BoldItalic",
            "Helvetica",
            "Helvetica-Bold",
            "Helvetica-Oblique",
            "Helvetica-BoldOblique",
            "Courier",
            "Courier-Bold",
            "Courier-Oblique",
            "Courier-BoldOblique",
            "Symbol",
            "ZapfDingbats",
        ];
        let reference_only = !standard.contains(&family.as_str());
        let mut decode = font.clone();
        if font.has(b"ToUnicode") {
            decode.remove(b"Encoding");
            if let Ok(stream) = font.get_deref(b"ToUnicode", self.pdf).and_then(PdfObject::as_stream)
                && let Ok(bytes) = stream.get_plain_content_with_limit(MAX_BYTES)
                && bytes.windows(5).any(|window| window == b">>def")
            {
                // lopdf requires whitespace after this dictionary delimiter.
                let mut normalized = Vec::with_capacity(bytes.len());
                for (index, &byte) in bytes.iter().enumerate() {
                    if bytes.get(index.saturating_sub(2)..index + 3) == Some(b">>def") {
                        normalized.push(b' ');
                    }
                    normalized.push(byte);
                }
                decode.set(b"ToUnicode", lopdf::Stream::new(Dictionary::new(), normalized));
            }
        }
        if two_byte
            && (!font.has(b"ToUnicode") || !(encoding == "Identity-H" || encoding == "Identity-V"))
        {
            self.loss(
                LossKind::Glyph,
                "CID encoding or missing Unicode map may not decode correctly",
            );
        }
        if matches!(
            font.get(b"Encoding").ok().map(|o| deref(self.pdf, o)),
            Some(PdfObject::Dictionary(_))
        ) {
            self.loss(
                LossKind::Glyph,
                "custom font encoding depends on partial lopdf glyph-name support",
            );
        }
        let first_char = font.get(b"FirstChar").ok().and_then(num).unwrap_or(0.0) as i64;
        let widths = font
            .get(b"Widths")
            .ok()
            .map(|o| deref(self.pdf, o))
            .and_then(|o| o.as_array().ok())
            .map(|items| {
                items
                    .iter()
                    .map(|o| num(deref(self.pdf, o)).filter(|width| *width >= 0.0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let cid = (two_byte && encoding == "Identity-H").then(|| cid_widths(self.pdf, &font)).flatten();
        if (two_byte && cid.is_none()) || (!two_byte && (widths.is_empty() || widths.iter().any(Option::is_none))) {
            self.loss(LossKind::Font, "text advance uses fallback font metrics");
        }
        let (default_width, cid_widths) = cid.unwrap_or_else(|| {
            (if two_byte { 1000.0 } else { 500.0 }, Vec::new())
        });
        let embedded_cid = (two_byte && encoding == "Identity-H")
            .then(|| text_outline::descendant(self.pdf, &font)).flatten().cloned();
        let unicode_map = if decode.has(b"ToUnicode") {
            match decode.get_font_encoding_with_limit(self.pdf, MAX_BYTES) {
                Ok(lopdf::Encoding::UnicodeMapEncoding(map)) => Some(lopdf::Encoding::UnicodeMapEncoding(map)),
                _ => {
                    self.loss(LossKind::Glyph, "ToUnicode map could not be parsed; fallback encoding used");
                    decode.remove(b"ToUnicode");
                    if let Ok(encoding) = font.get(b"Encoding") {
                        decode.set(b"Encoding", encoding.clone());
                    }
                    None
                }
            }
        } else {
            None
        };
        FontInfo {
            family,
            style,
            decode,
            unicode_map,
            two_byte,
            first_char,
            widths,
            cid_widths,
            default_width,
            vertical: encoding.ends_with("-V"),
            reference_only,
            embedded_cid,
            embedded: OnceCell::new(),
        }
    }

    fn decode_string(&mut self, font: Option<&FontInfo>, bytes: &[u8]) -> String {
        if let Some(font) = font {
            if let Some(encoding) = &font.unicode_map
                && let Ok(text) = PdfDocument::decode_text(encoding, bytes)
            {
                if text.contains('\u{fffd}') {
                    self.loss(LossKind::Glyph, "text contains unmapped Unicode glyphs");
                }
                return text;
            }
            if let Ok(encoding) = font.decode.get_font_encoding_with_limit(self.pdf, MAX_BYTES) {
                if matches!(
                    encoding,
                    lopdf::Encoding::OneByteEncoding(_) | lopdf::Encoding::Differences(_)
                ) {
                    let mut text = String::new();
                    for byte in bytes {
                        let glyph =
                            PdfDocument::decode_text(&encoding, &[*byte]).unwrap_or_default();
                        if glyph.is_empty() {
                            self.loss(LossKind::Glyph, "unmapped single-byte glyph replaced");
                            text.push('\u{fffd}');
                        } else {
                            text.push_str(&glyph);
                        }
                    }
                    return text;
                }
                if let Ok(text) = PdfDocument::decode_text(&encoding, bytes) {
                    if !text.is_empty() || bytes.is_empty() {
                        if text.contains('\u{fffd}') {
                            self.loss(LossKind::Glyph, "text contains unmapped Unicode glyphs");
                        }
                        return text;
                    }
                }
            }
        }
        self.loss(
            LossKind::Glyph,
            "text decoding failed; ASCII retained with replacement glyphs",
        );
        bytes
            .iter()
            .map(|b| if b.is_ascii() { *b as char } else { '\u{fffd}' })
            .collect()
    }

    fn show_text(
        &mut self, parts: &[PdfObject], gs: &GState, tm: &mut Matrix, block: &mut TextBlock,
    ) -> Option<Run> {
        let font = gs.font.as_deref();
        let start = *tm;
        let clips = matches!(gs.render_mode, 4..=7);
        let mode = if clips { gs.render_mode - 4 } else { gs.render_mode };
        let fills = matches!(mode, 0 | 2);
        let strokes = matches!(mode, 1 | 2);
        let translucent = (fills && gs.fill_alpha < 1.0) || (strokes && gs.stroke_alpha < 1.0);
        let prefer_embedded = self.opts.outline_embedded_text
            || (!self.opts.favour_editable_text
                && font.is_some_and(|font| font.embedded_cid.is_some()));
        let outlines_paint = prefer_embedded && (fills || strokes);
        let program = font.filter(|_| outlines_paint || clips).and_then(|font| {
            font.embedded.get_or_init(|| {
                text_outline::EmbeddedFont::read(self.pdf, font.embedded_cid.as_ref()?)
            }).as_ref()
        });
        let outlines = program.and_then(|program| skrifa::FontRef::new(&program.data).ok())
            .map(|font| font.outline_glyphs());
        let mut pen = outlines.as_ref().zip(program)
            .filter(|_| gs.font_size.is_finite() && gs.font_size > 0.0 && gs.font_size <= f64::from(f32::MAX))
            .map(|(outlines, program)| (text_outline::GlyphPen::new(gs.h_scale), outlines, program));
        let mut text = String::new();
        let mut advance = 0.0;
        let mut glyph_ends = Vec::new();
        let mut last_end = 0;
        let mut has_codes = false;
        for part in parts {
            match part {
                PdfObject::String(bytes, _) => {
                    has_codes |= !bytes.is_empty();
                    if !clips || mode != 3 {
                        text.push_str(&self.decode_string(font, bytes));
                    }
                    let step = if font.is_some_and(|f| f.two_byte) {
                        2
                    } else {
                        1
                    };
                    for code in bytes.chunks(step) {
                        if let Some((glyph_pen, outlines, embedded)) = pen.as_mut() {
                            glyph_pen.x = advance;
                            let glyph = <[u8; 2]>::try_from(code).ok()
                                .and_then(|bytes| embedded.glyph_id(u16::from_be_bytes(bytes)))
                                .and_then(|id| outlines.get(id));
                            let draw = skrifa::outline::DrawSettings::unhinted(
                                skrifa::instance::Size::new(gs.font_size as f32), skrifa::instance::LocationRef::default(),
                            );
                            if glyph.is_none_or(|glyph| glyph.draw(draw, glyph_pen).is_err())
                                || glyph_pen.exceeded_budget()
                            {
                                pen = None;
                            } else {
                                glyph_pen.close();
                                if outlines_paint && glyph_pen.path.len() > last_end {
                                    last_end = glyph_pen.path.len();
                                    glyph_ends.push(last_end);
                                }
                            }
                        }
                        let width = font.map_or(500.0, |font| font.width(code));
                        advance += (width / 1000.0 * gs.font_size
                            + gs.char_spacing
                            + if code == b" " { gs.word_spacing } else { 0.0 })
                            * gs.h_scale;
                    }
                }
                other => {
                    if let Some(adjust) = num(other) {
                        advance -= adjust / 1000.0 * gs.font_size * gs.h_scale;
                    }
                }
            }
        }
        *tm = mul(&[1.0, 0.0, 0.0, 1.0, advance, 0.0], tm);
        let line = mul(&start, &gs.ctm);
        if clips && has_codes && pen.is_none() {
            block.clip_failed = true;
        }
        if let Some((mut pen, _, _)) = pen {
            if !pen.path.is_empty() {
                let map = |p: [f64; 2]| self.to_doc(apply(&line, [p[0], p[1] + gs.rise]));
                for point in pen.path.iter_mut().flat_map(|sub| &mut sub.points) {
                    point.pos = map(point.pos);
                    point.handle_in = point.handle_in.map(map);
                    point.handle_out = point.handle_out.map(map);
                }
                for point in pen.path.iter_mut().flat_map(|sub| &mut sub.points) {
                    point.id = self.next_point();
                }
                let bounds = Self::path_bounds(&pen.path);
                if clips {
                    if bounds.iter().any(|v| !v.is_finite() || v.abs() > f32::MAX as f64) {
                        block.clip_failed = true;
                    } else {
                        block.add_clip(&pen.path);
                    }
                }
                if outlines_paint {
                    self.flush_text(std::mem::take(&mut block.runs));
                    let points = pen.path.iter().map(|sub| sub.points.len()).sum::<usize>();
                    if block.outline_points + points > 65_536 || block.outlines.len() == 512 {
                        self.flush_outlines(block);
                        block.outline_budget_exceeded = true;
                    }
                    if block.outline_budget_exceeded && translucent && gs.text_knockout {
                        self.loss(LossKind::Blend, "text object exceeded the outline compositing budget");
                    }
                    block.outline_points += points;
                    block.outlines.push(OutlinedRun {
                        path: pen.path,
                        glyph_ends,
                        gs: gs.clone(),
                        fills,
                        strokes,
                        translucent,
                        name: (!text.is_empty()).then_some(text),
                    });
                    return None;
                }
            }
            if outlines_paint { return None; }
        }
        if clips && mode == 3 { return None; }
        if mode == 3 {
            self.loss(LossKind::Other, "invisible text skipped");
            return None;
        }
        if text.is_empty() {
            return None;
        }
        if outlines_paint {
            self.loss(LossKind::Font, "embedded text could not be outlined; editable text retained");
        }
        if !gs.text_knockout && translucent {
            self.loss(LossKind::Blend, "editable text does not preserve compositing of overlapping glyphs");
        }
        if let Some(font) = font.filter(|font| font.reference_only) {
            self.loss(
                LossKind::Font,
                format!("font program not registered; family reference only: {}", font.family),
            );
        }
        if font.is_some_and(|f| f.vertical) {
            self.loss(
                LossKind::VerticalText,
                "vertical text laid out horizontally",
            );
        }
        if mode != 0 {
            self.loss(
                LossKind::Other,
                "text stroke rendering approximated as fill",
            );
        }
        let origin = self.to_doc(apply(&line, [0.0, gs.rise]));
        let end = self.to_doc(apply(&line, [1.0, gs.rise]));
        let top = self.to_doc(apply(&line, [0.0, gs.rise + 1.0]));
        if end[0] <= origin[0]
            || (end[1] - origin[1]).abs() > 1e-6
            || (top[0] - origin[0]).abs() > 1e-6
            || top[1] >= origin[1]
            || gs.h_scale != 1.0
            || ((end[0] - origin[0]).hypot(end[1] - origin[1])
                - (top[0] - origin[0]).hypot(top[1] - origin[1]))
            .abs()
                > 1e-6
        {
            self.loss(
                LossKind::Other,
                "transformed text orientation or horizontal scale approximated",
            );
        }
        if gs.char_spacing != 0.0
            || gs.word_spacing != 0.0
            || parts.iter().any(|o| num(o).is_some())
        {
            self.loss(
                LossKind::Other,
                "PDF text spacing retained in bounds, not per-glyph layout",
            );
        }
        let scale = scale_of(&line) * self.geometry.unit;
        let size = gs.font_size.abs() * scale;
        if !size.is_finite()
            || size <= 0.0
            || size > f32::MAX as f64
            || origin.iter().any(|v| !v.is_finite())
            || !advance.is_finite()
        {
            self.loss(LossKind::Other, "invalid text geometry skipped");
            return None;
        }
        self.loss(
            LossKind::Font,
            "text ink bounds approximated from baseline and font size",
        );
        Some(Run {
            text,
            origin,
            size,
            advance: advance * scale,
            family: font.map(|f| f.family.clone()).unwrap_or_default(),
            style: font.map(|f| f.style.clone()).unwrap_or_default(),
            fill: if mode == 1 {
                gs.stroke.clone()
            } else {
                gs.fill.clone()
            },
            opacity: if mode == 1 {
                gs.stroke_alpha
            } else {
                gs.fill_alpha
            },
            blend_mode: gs.blend_mode,
            parent: gs.parent,
        })
    }

    fn outline_ink(&mut self, run: &OutlinedRun, split: bool) -> Option<Vec<Vec<text_outline::TextInk>>> {
        let gs = &run.gs;
        let [x, y] = self.user_to_doc(gs, [0.0, 0.0]);
        let axis_x = self.user_to_doc(gs, [1.0, 0.0]);
        let axis_y = self.user_to_doc(gs, [0.0, 1.0]);
        let user_to_doc = Affine::new([axis_x[0] - x, axis_x[1] - y, axis_y[0] - x, axis_y[1] - y, x, y]);
        let width = if run.strokes && gs.line_width == 0.0 {
            self.loss(LossKind::Other, "device hairline stroke approximated as 0.25 points");
            0.25 / (scale_of(&gs.ctm) * self.geometry.unit)
        } else {
            gs.line_width
        };
        let style = StrokeStyle {
            width: width as f32,
            dash: gs.dash.iter().map(|dash| *dash as f32).collect(),
            dash_offset: gs.dash_offset as f32,
            cap: gs.cap.clone(),
            join: gs.join.clone(),
            ..StrokeStyle::default()
        };
        let mut glyphs = Vec::new();
        let mut start = 0;
        let ends = run.glyph_ends.iter().copied().filter(|_| split)
            .chain((!split).then_some(run.path.len()));
        for end in ends {
            let outline = Self::curve_from_path(&run.path[start..end], [0.0, 0.0], true).to_kurbo();
            let ink = if run.strokes {
                text_outline::expand_ink(outline, &style, user_to_doc, run.fills)?
            } else {
                vec![text_outline::TextInk { path: outline, is_stroke: false }]
            };
            glyphs.push(ink);
            start = end;
        }
        Some(glyphs)
    }

    fn emit_text_ink(
        &mut self, ink: impl Iterator<Item = text_outline::TextInk>, gs: &GState, name: Option<&str>,
    ) {
        let ink: Vec<_> = ink.filter(|ink| {
            !ink.path.elements().is_empty() && if ink.is_stroke {
                gs.stroke != Paint::None && gs.stroke_alpha > 0.0
            } else {
                gs.fill != Paint::None && gs.fill_alpha > 0.0
            }
        }).collect();
        let bounds: Vec<_> = ink.iter().map(|ink| ink.path.bounding_box()).collect();
        let grouped = ink.len() > 1;
        let parent = if grouped {
            let bound = bounds.iter().copied().reduce(|left, right| left.union(right)).unwrap();
            Some(self.doc.create_object(Object {
                name: name.unwrap_or("PDF text").into(),
                pos: [bound.x0 as f32, bound.y0 as f32],
                size: [bound.width() as f32, bound.height() as f32],
                kind: ObjectKind::Group,
                parent_id: gs.parent,
                blend_mode: if gs.blend_mode.is_normal() { BlendMode::Normal } else { BlendMode::PassThrough },
                ..Object::default()
            }))
        } else {
            gs.parent
        };
        for (ink, bound) in ink.into_iter().zip(bounds) {
            let path = Affine::translate((-bound.x0, -bound.y0)) * ink.path;
            let mut curve = Curve::from_kurbo(&path);
            for point in std::iter::once(&mut curve.contour).chain(&mut curve.holes)
                .flat_map(|contour| &mut contour.points)
            {
                point.id = self.next_point();
            }
            let (label, fill, opacity) = if ink.is_stroke {
                ("stroke", gs.stroke.clone(), gs.stroke_alpha)
            } else {
                ("fill", gs.fill.clone(), gs.fill_alpha)
            };
            self.doc.create_object(Object {
                name: if grouped { label } else { name.unwrap_or(label) }.into(),
                pos: [bound.x0 as f32, bound.y0 as f32],
                size: [bound.width() as f32, bound.height() as f32],
                fill,
                fill_rule: FillRule::NonZero,
                opacity,
                blend_mode: gs.blend_mode,
                kind: ObjectKind::Curve(curve),
                parent_id: parent,
                ..Object::default()
            });
            self.report.objects += 1;
        }
    }

    fn flush_text_block(&mut self, block: &mut TextBlock) {
        self.flush_outlines(block);
        self.flush_text(std::mem::take(&mut block.runs));
    }

    fn flush_outlines(&mut self, block: &mut TextBlock) {
        let runs = std::mem::take(&mut block.outlines);
        block.outline_points = 0;
        if runs.iter().skip(1).any(|run| run.translucent && run.gs.text_knockout) {
            match self.composite_text_object(&runs) {
                Some(true) => return,
                Some(false) => {}
                None => self.loss(LossKind::Blend, "overlapping text could not be composited across showing operators"),
            }
        }
        for run in runs {
            let split = (run.fills && run.strokes) || (!run.gs.text_knockout && run.translucent);
            if run.strokes && run.translucent {
                let expanded = self.outline_ink(&run, split).and_then(|mut glyphs| {
                    if run.gs.text_knockout {
                        text_outline::knock_out(&mut glyphs)?;
                    }
                    text_outline::valid_ink(&glyphs).then_some(glyphs)
                });
                if let Some(glyphs) = expanded {
                    self.emit_text_ink(glyphs.into_iter().flatten(), &run.gs, run.name.as_deref());
                    continue;
                }
                self.loss(LossKind::Blend, "translucent glyph stroke expansion failed; fill and stroke compositing approximated");
            }
            if split && run.glyph_ends.len() >= 2 {
                let bounds = Self::path_bounds(&run.path);
                self.emit_glyph_group(&run.path, &run.glyph_ends, bounds, &run.gs, run.fills, run.strokes, run.name);
            } else {
                self.emit_path(&run.path, &run.gs, run.fills, run.strokes, false, run.name);
            }
        }
    }

    fn composite_text_object(&mut self, runs: &[OutlinedRun]) -> Option<bool> {
        let parent = runs[0].gs.parent;
        let mut glyphs = Vec::new();
        let mut ends = Vec::with_capacity(runs.len());
        let mut previous = Vec::with_capacity(runs.len());
        let mut overlaps = false;
        for run in runs {
            if !run.gs.text_knockout || run.gs.parent != parent {
                return None;
            }
            let ink = self.outline_ink(run, true)?;
            if !text_outline::valid_ink(&ink) {
                return None;
            }
            let bounds = ink.iter().flatten().map(|ink| ink.path.bounding_box())
                .reduce(|left, right| left.union(right));
            if let Some(bounds) = bounds {
                overlaps |= run.translucent && previous.iter()
                    .any(|prior| bounds.intersect(*prior).area() > 0.0);
                previous.push(bounds);
            }
            glyphs.extend(ink);
            ends.push(glyphs.len());
        }
        if !overlaps {
            return Some(false);
        }
        text_outline::knock_out(&mut glyphs)?;
        if !text_outline::valid_ink(&glyphs) {
            return None;
        }
        let mut glyphs = glyphs.into_iter();
        let mut start = 0;
        for (run, end) in runs.iter().zip(ends) {
            self.emit_text_ink(
                glyphs.by_ref().take(end - start).flatten(), &run.gs, run.name.as_deref(),
            );
            start = end;
        }
        Some(true)
    }

    fn emit_glyph_group(
        &mut self, path: &[SubPath], ends: &[usize], bounds: [f64; 4], gs: &GState,
        fill: bool, stroke: bool, name: Option<String>,
    ) {
        if bounds.iter().any(|v| !v.is_finite() || v.abs() > f32::MAX as f64) {
            self.loss(LossKind::Other, "text outline with invalid geometry skipped");
            return;
        }
        let mut glyph_state = gs.clone();
        glyph_state.parent = Some(self.doc.create_object(Object {
            name: name.unwrap_or_else(|| "PDF text".into()),
            pos: [bounds[0] as f32, bounds[1] as f32],
            size: [(bounds[2] - bounds[0]) as f32, (bounds[3] - bounds[1]) as f32],
            kind: ObjectKind::Group,
            parent_id: gs.parent,
            blend_mode: if gs.blend_mode.is_normal() { BlendMode::Normal } else { BlendMode::PassThrough },
            ..Object::default()
        }));
        let mut start = 0;
        for &end in ends {
            self.emit_path(&path[start..end], &glyph_state, fill, stroke, false, None);
            start = end;
        }
    }

    fn flush_text(&mut self, runs: Vec<Run>) {
        if !self.opts.favour_editable_text {
            for run in runs {
                self.emit_lines(vec![vec![run]]);
            }
            return;
        }
        let mut lines: Vec<Vec<Run>> = Vec::new();
        for run in runs {
            if !run.blend_mode.is_normal() {
                self.emit_text_lines(std::mem::take(&mut lines));
                self.emit_lines(vec![vec![run]]);
                continue;
            }
            let joins = lines.last().and_then(|l| l.last()).is_some_and(|prev| {
                (prev.origin[1] - run.origin[1]).abs() < prev.size * 0.3
                    && prev.family == run.family
                    && prev.style == run.style
                    && (prev.size - run.size).abs() < 0.01
                    && prev.parent == run.parent
            });
            if joins {
                lines.last_mut().unwrap().push(run);
            } else {
                lines.push(vec![run]);
            }
        }
        self.emit_text_lines(lines);
    }

    fn emit_text_lines(&mut self, lines: Vec<Vec<Run>>) {
        if self.opts.group_text_lines {
            self.emit_lines(lines);
        } else {
            for line in lines {
                self.emit_lines(vec![line]);
            }
        }
    }

    fn emit_lines(&mut self, lines: Vec<Vec<Run>>) {
        let Some(first) = lines.first().and_then(|l| l.first()).cloned() else {
            return;
        };
        let mut text = String::new();
        let mut ranges = Vec::new();
        let mut points = Vec::new();
        let mut scalar = 0;
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                text.push('\n');
                scalar += 1;
            }
            for run in line {
                if run.parent != first.parent {
                    self.loss(
                        LossKind::Other,
                        "grouped text spans different clips; first clip retained",
                    );
                }
                if run.opacity != first.opacity {
                    self.loss(
                        LossKind::Blend,
                        "grouped text run opacity differs; first opacity retained",
                    );
                }
                let end = scalar + run.text.chars().count();
                ranges.push(json!({"start": scalar, "end": end, "size_pt": run.size, "family": run.family, "style": run.style, "fill": paint_to_json_value(&run.fill)}));
                scalar = end;
                text.push_str(&run.text);
                points.push([run.origin[0], run.origin[1] - run.size * 0.8]);
                points.push([run.origin[0] + run.advance, run.origin[1] + run.size * 0.2]);
            }
        }
        if lines.len() > 1 || lines.iter().any(|l| l.len() > 1) {
            self.loss(
                LossKind::Other,
                "grouped editable text reflows original run positions",
            );
        }
        let bounds = Self::bounds(points);
        if bounds
            .iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
        {
            self.loss(
                LossKind::Other,
                "text bounds outside document coordinate range skipped",
            );
            return;
        }
        let story_id = StoryId(dtool_core::text_cmd::next_story_id(self.doc));
        self.doc.stories.push(Story {
            id: story_id,
            text: text.clone(),
            ranges,
            ..Story::default()
        });
        let framed = self.opts.favour_editable_text && self.opts.group_text_lines;
        let id = self.doc.create_object(Object {
            name: text,
            pos: [bounds[0] as f32, bounds[1] as f32],
            size: [
                (bounds[2] - bounds[0]).max(0.0) as f32,
                (bounds[3] - bounds[1]).max(0.0) as f32,
            ],
            opacity: first.opacity,
            blend_mode: first.blend_mode,
            kind: if framed {
                ObjectKind::FrameText { story: story_id }
            } else {
                ObjectKind::ArtText { story: story_id }
            },
            parent_id: first.parent,
            ..Object::default()
        });
        // PDF positions a line by its baseline. Align the editable text using
        // the same font and line metrics used by the canvas.
        if !framed && dtool_core::text_lines::text_lines_ready()
            && let Some(line) = dtool_core::text_lines::text_lines(self.doc, id).first()
            && let Some(object) = self.doc.find_object_mut(id.0)
        {
            let top = object.pos[1] + (first.origin[1] as f32 - line.origin[1]);
            if top.is_finite() {
                object.pos[1] = top;
            }
        }
        self.report.texts += 1;
    }

    pub(super) fn finish(self) {
        loop {
            let used: Vec<_> = self
                .doc
                .objects
                .iter()
                .filter_map(|o| o.parent_id)
                .collect();
            let before = self.doc.objects.len();
            self.doc
                .objects
                .retain(|o| o.name != "clip" || !o.is_group() || used.contains(&o.id));
            if before == self.doc.objects.len() {
                break;
            }
        }
    }
}

fn encode_bmp(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let size = 54 + rgba.len() as u32;
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend(b"BM");
    bytes.extend(size.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(54u32.to_le_bytes());
    bytes.extend(40u32.to_le_bytes());
    bytes.extend(width.to_le_bytes());
    bytes.extend((-(height as i32)).to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(32u16.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((rgba.len() as u32).to_le_bytes());
    bytes.extend([0; 16]);
    for p in rgba.chunks_exact(4) {
        bytes.extend([p[2], p[1], p[0], p[3]]);
    }
    bytes
}
