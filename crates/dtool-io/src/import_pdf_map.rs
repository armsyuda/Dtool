use std::rc::Rc;

use dtool_core::colour::Colour as ManagedColour;
use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectId, ObjectKind, VectorCrop};
use dtool_core::paint::{Cap, Colour, Join, Paint, StrokeStyle, paint_to_json_value};
use dtool_core::resources::{ImageFormat, ImageResource};
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as PdfDocument, Object as PdfObject, ObjectId as PdfId, Stream};
use serde_json::json;

use crate::import_pdf::{ImportAs, ImportOptions};
use crate::import_report::{ImportReport, LossKind};
use crate::{IoError, IoErrorKind};

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
    Pattern,
    Other,
}

#[derive(Clone)]
struct FontInfo {
    family: String,
    style: String,
    decode: Dictionary,
    two_byte: bool,
    first_char: i64,
    widths: Vec<f64>,
    default_width: f64,
    vertical: bool,
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
    cap: Cap,
    join: Join,
    fill_alpha: f32,
    stroke_alpha: f32,
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
            cap: Cap::Butt,
            join: Join::Miter { limit: 10.0 },
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
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
        }
    }
}

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
    parent: Option<ObjectId>,
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
}

impl<'a> PdfMapper<'a> {
    pub(super) fn new(
        pdf: &'a PdfDocument,
        doc: &'a mut Document,
        report: &'a mut ImportReport,
        opts: &'a ImportOptions,
    ) -> Self {
        if pdf
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|o| dict_of(pdf, o))
            .is_some_and(|d| d.has(b"OutputIntents"))
        {
            report.add_loss(
                LossKind::Icc,
                1,
                false,
                "PDF output intent profile not applied; source device colours retained",
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
        if self.opts.import_as == ImportAs::Artboards {
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
            kind: ObjectKind::Group,
            vector_crop: Some(VectorCrop {
                rect: [origin[0], origin[1], geometry.size()[0], geometry.size()[1]],
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
        let mut runs = Vec::new();
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
                "d" => {
                    gs.dash = operands
                        .first()
                        .and_then(|o| o.as_array().ok())
                        .map(|items| nums(items))
                        .unwrap_or_default();
                    if v.last().copied().unwrap_or(0.0) != 0.0 {
                        self.loss(LossKind::Other, "stroke dash phase ignored");
                    }
                }
                "ri" | "i" => self.loss(LossKind::Icc, "rendering intent or flatness hint ignored"),
                "gs" => self.ext_gstate(operands, resources, &mut gs),
                "g" | "rg" | "k" | "G" | "RG" | "K" => {
                    let space = match op.operator.as_str() {
                        "g" | "G" => Space::Grey,
                        "rg" | "RG" => Space::Rgb,
                        _ => Space::Cmyk,
                    };
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
                    let fill = matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                    let stroke = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*");
                    if (fill || stroke) && !path.is_empty() {
                        self.emit_path(&path, &gs, fill, stroke, operator.ends_with('*'));
                    }
                    if let Some(even_odd) = pending_clip.take() {
                        self.apply_clip(&path, even_odd, &mut gs);
                    }
                    path.clear();
                }
                "sh" => {
                    self.loss(LossKind::Gradient, "shading approximated as a solid fill");
                    if let Some(PdfObject::Name(name)) = operands.first() {
                        self.shade_fallback(name, resources, &gs);
                    }
                }
                "BI" | "ID" | "EI" => self.loss(LossKind::Image, "inline image skipped"),
                "Do" => {
                    if in_text {
                        self.flush_text(std::mem::take(&mut runs));
                    }
                    if let Some(PdfObject::Name(name)) = operands.first() {
                        self.draw_xobject(name, resources, &gs, depth);
                    } else {
                        self.loss(LossKind::Other, "invalid XObject operand");
                    }
                }
                "BT" => {
                    self.flush_text(std::mem::take(&mut runs));
                    in_text = true;
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                "ET" => {
                    self.flush_text(std::mem::take(&mut runs));
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
                        Some(PdfObject::Name(name)) => self.font_info(name, resources).map(Rc::new),
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
                    if let Some(run) = self.show_text(&parts, &gs, &mut tm) {
                        runs.push(run);
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
        self.flush_text(runs);
        if in_text || !stack.is_empty() {
            self.loss(LossKind::Other, "unterminated text or graphics state");
        }
    }

    fn close_last(path: &mut [SubPath]) {
        if let Some(sub) = path.last_mut() {
            if sub.points.len() > 1
                && sub.points.first().map(|p| p.pos) == sub.points.last().map(|p| p.pos)
            {
                if let Some(last) = sub.points.pop() {
                    sub.points[0].handle_in = last.handle_in;
                }
            }
            sub.closed = true;
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

    fn space_of(&mut self, object: &PdfObject, resources: &[Dictionary]) -> Space {
        let object = deref(self.pdf, object);
        match object {
            PdfObject::Name(name) => match name.as_slice() {
                b"DeviceGray" | b"G" => Space::Grey,
                b"DeviceRGB" | b"RGB" => Space::Rgb,
                b"DeviceCMYK" | b"CMYK" => Space::Cmyk,
                b"Pattern" => Space::Pattern,
                _ => match self.lookup(resources, b"ColorSpace", name) {
                    Some(object) => self.space_of(&object, &[]),
                    None => Space::Other,
                },
            },
            PdfObject::Array(items) => {
                match items.first().and_then(|o| o.as_name().ok()).unwrap_or(b"") {
                    b"ICCBased" => {
                        self.loss(
                            LossKind::Icc,
                            "embedded ICC profile not applied; alternate device colour used",
                        );
                        match items
                            .get(1)
                            .and_then(|o| dict_of(self.pdf, o))
                            .and_then(|d| d.get(b"N").ok())
                            .and_then(num)
                        {
                            Some(1.0) => Space::Grey,
                            Some(3.0) => Space::Rgb,
                            Some(4.0) => Space::Cmyk,
                            _ => Space::Other,
                        }
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
                b"LW" => gs.line_width = num(deref(self.pdf, value)).unwrap_or(1.0),
                b"BM" if matches!(deref(self.pdf, value), PdfObject::Name(n) if n == b"Normal" || n == b"Compatible") =>
                    {}
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
        if path.is_empty() {
            self.loss(LossKind::Mask, "empty clipping path ignored");
            return;
        }
        if path.len() != 1 || !path[0].rect || (even_odd && path.len() > 1) {
            self.loss(LossKind::Mask, "non-rectangular clip kept as its bounds");
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
            name: "clip".into(),
            pos: [rect[0] as f32, rect[1] as f32],
            size,
            kind: ObjectKind::Group,
            parent_id: gs.parent,
            vector_crop: Some(VectorCrop {
                rect: [rect[0] as f32, rect[1] as f32, size[0], size[1]],
            }),
            ..Object::default()
        });
        gs.clip = Some(rect);
        gs.parent = Some(id);
    }

    fn shade_fallback(&mut self, name: &[u8], resources: &[Dictionary], gs: &GState) {
        let dict = match self.lookup(resources, b"Shading", name) {
            Some(PdfObject::Dictionary(dict)) => dict,
            Some(PdfObject::Stream(stream)) => stream.dict,
            _ => return,
        };
        let space = match dict.get(b"ColorSpace") {
            Ok(object) => {
                let object = object.clone();
                self.space_of(&object, resources)
            }
            Err(_) => return,
        };
        let pdf = self.pdf;
        let values = dict
            .get(b"Function")
            .ok()
            .and_then(|function| dict_of(pdf, function))
            .and_then(|function| function.get(b"C0").ok())
            .map(|c0| match deref(pdf, c0) {
                PdfObject::Array(items) => nums(items),
                _ => Vec::new(),
            })
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| vec![0.5; 4]);
        let fill = self.colour_from(space, &values);
        let rect = gs.clip.unwrap_or_else(|| {
            let size = self.geometry.size();
            let origin = [self.origin[0] as f64, self.origin[1] as f64];
            [origin[0], origin[1], origin[0] + size[0] as f64, origin[1] + size[1] as f64]
        });
        if rect[2] <= rect[0] || rect[3] <= rect[1] {
            return;
        }
        let mut points = Vec::new();
        for corner in [[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]] {
            let id = self.next_point();
            points.push(PathPoint::sharp(id, corner));
        }
        let mut shade = gs.clone();
        shade.fill = fill;
        self.emit_path(&[SubPath { points, closed: true, rect: true }], &shade, true, false, false);
    }

    fn emit_path(
        &mut self,
        path: &[SubPath],
        gs: &GState,
        fill: bool,
        stroke: bool,
        even_odd: bool,
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
            if fill && (path.len() > 1 || even_odd) {
                self.loss(
                    LossKind::Other,
                    "compound or even-odd fill rule is not stored by core; contour geometry retained",
                );
            }
            if fill && stroke && path.iter().any(|sub| !sub.closed) {
                self.loss(
                    LossKind::Other,
                    "open filled-and-stroked path closed for core; closing stroke may differ",
                );
            }
            let contours = path
                .iter()
                .map(|sub| Contour {
                    points: sub
                        .points
                        .iter()
                        .map(|p| PathPoint {
                            id: p.id,
                            pos: [p.pos[0] - b[0], p.pos[1] - b[1]],
                            handle_in: p.handle_in.map(|h| [h[0] - b[0], h[1] - b[1]]),
                            handle_out: p.handle_out.map(|h| [h[0] - b[0], h[1] - b[1]]),
                            kind: p.kind,
                            corner: p.corner,
                        })
                        .collect(),
                    closed: sub.closed || fill,
                })
                .collect::<Vec<_>>();
            let mut iter = contours.into_iter();
            self.paths += 1;
            (
                ObjectKind::Curve(Curve::from_contour_and_holes(
                    iter.next().unwrap_or_default(),
                    iter.collect(),
                )),
                format!("path_{}", self.paths),
            )
        };
        self.doc.create_object(Object {
            name,
            pos: [b[0] as f32, b[1] as f32],
            size: [(b[2] - b[0]) as f32, (b[3] - b[1]) as f32],
            fill: fill_paint,
            stroke: stroke_style,
            opacity,
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
                    self.apply_clip(&[clip], false, &mut inner);
                } else {
                    self.loss(LossKind::Mask, "form bounding box missing or invalid");
                }
                let form_resources = stream
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|o| dict_of(self.pdf, o))
                    .cloned();
                let form_resources = form_resources
                    .map(|d| vec![d])
                    .unwrap_or_else(|| resources.to_vec());
                if stream.dict.has(b"Group") {
                    self.loss(LossKind::Blend, "form transparency group flattened");
                }
                self.run(&content.operations, &form_resources, inner, depth + 1);
            }
            _ => self.loss(LossKind::Other, "unsupported XObject subtype skipped"),
        }
    }

    fn image_pixels(
        &mut self,
        stream: &Stream,
        resources: &[Dictionary],
    ) -> Option<(u32, u32, Vec<u8>)> {
        let dict = &stream.dict;
        let dimension = |key| {
            dict.get(key)
                .ok()
                .and_then(|o| num(deref(self.pdf, o)))
                .filter(|v| *v > 0.0 && *v <= i32::MAX as f64 && v.fract() == 0.0)
                .map(|v| v as u32)
        };
        let (Some(width), Some(height)) = (dimension(b"Width"), dimension(b"Height")) else {
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
        let channels = match space {
            Space::Grey => 1,
            Space::Rgb => 3,
            Space::Cmyk => 4,
            _ => {
                self.loss(
                    LossKind::Image,
                    "image colour space unsupported; image skipped",
                );
                return None;
            }
        };
        let Some(bytes) = stream_bytes(stream) else {
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
        let decode = match dict.get(b"Decode") {
            Ok(object) => match deref(self.pdf, object).as_array().ok().map(|a| nums(a)) {
                Some(values) if values.len() == channels * 2 => values,
                _ => {
                    self.loss(LossKind::Image, "invalid image Decode array; image skipped");
                    return None;
                }
            },
            Err(_) => (0..channels).flat_map(|_| [0.0, 1.0]).collect(),
        };
        let mut rgba = Vec::with_capacity(count * 4);
        for pixel in bytes.chunks_exact(channels) {
            let sample = |i: usize| {
                (decode[2 * i] + pixel[i] as f64 / 255.0 * (decode[2 * i + 1] - decode[2 * i]))
                    .clamp(0.0, 1.0)
            };
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
        if space == Space::Cmyk {
            self.loss(
                LossKind::Icc,
                "CMYK image converted to RGB preview without an ICC transform",
            );
        }
        if dict.has(b"SMask") || dict.has(b"Mask") {
            self.loss(LossKind::Mask, "image soft or colour-key mask dropped");
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
        let mut resource = ImageResource::new_embedded(
            ImageFormat::Bmp,
            [width, height],
            self.doc.dpi,
            encode_bmp(width, height, &rgba),
        );
        resource.name = name.clone();
        resource.colour_space = "RGBA".into();
        let resource_id = self.doc.add_image_resource(resource);
        let id = self.doc.create_object(Object {
            name,
            pos: [b[0] as f32, b[1] as f32],
            size: [(b[2] - b[0]) as f32, (b[3] - b[1]) as f32],
            opacity: gs.fill_alpha,
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

    fn font_info(&mut self, name: &[u8], resources: &[Dictionary]) -> Option<FontInfo> {
        let object = self.lookup(resources, b"Font", name)?;
        let font = dict_of(self.pdf, &object)?.clone();
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
        if !standard.contains(&family.as_str()) {
            self.loss(
                LossKind::Font,
                format!(
                    "font program not registered; family reference only: {}",
                    family
                ),
            );
        }
        let mut decode = font.clone();
        if font.has(b"ToUnicode") {
            decode.remove(b"Encoding");
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
                    .filter_map(|o| num(deref(self.pdf, o)))
                    .collect()
            })
            .unwrap_or_default();
        if two_byte || font.get(b"Widths").is_err() {
            self.loss(LossKind::Font, "text advance uses fallback metrics; CID widths and standard font metrics not loaded");
        }
        Some(FontInfo {
            family,
            style,
            decode,
            two_byte,
            first_char,
            widths,
            default_width: if two_byte { 1000.0 } else { 500.0 },
            vertical: encoding.ends_with("-V"),
        })
    }

    fn decode_string(&mut self, font: Option<&FontInfo>, bytes: &[u8]) -> String {
        if let Some(font) = font {
            if let Ok(encoding) = font
                .decode
                .get_font_encoding_with_limit(self.pdf, MAX_BYTES)
            {
                if font.decode.has(b"ToUnicode")
                    && !matches!(encoding, lopdf::Encoding::UnicodeMapEncoding(_))
                {
                    self.loss(
                        LossKind::Glyph,
                        "ToUnicode map could not be parsed; fallback encoding used",
                    );
                }
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

    fn show_text(&mut self, parts: &[PdfObject], gs: &GState, tm: &mut Matrix) -> Option<Run> {
        let font = gs.font.as_deref();
        let start = *tm;
        let mut text = String::new();
        let mut advance = 0.0;
        for part in parts {
            match part {
                PdfObject::String(bytes, _) => {
                    text.push_str(&self.decode_string(font, bytes));
                    let step = if font.is_some_and(|f| f.two_byte) {
                        2
                    } else {
                        1
                    };
                    for code in bytes.chunks(step) {
                        let width = font
                            .and_then(|f| {
                                if f.two_byte {
                                    None
                                } else {
                                    let index = code[0] as i64 - f.first_char;
                                    if index >= 0 {
                                        f.widths.get(index as usize).copied()
                                    } else {
                                        None
                                    }
                                }
                            })
                            .unwrap_or_else(|| font.map(|f| f.default_width).unwrap_or(500.0));
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
        if gs.render_mode >= 4 {
            self.loss(LossKind::Mask, "text clipping mode dropped");
        }
        if gs.render_mode == 3 || gs.render_mode == 7 {
            self.loss(LossKind::Other, "invisible text skipped");
            return None;
        }
        if text.is_empty() {
            return None;
        }
        if font.is_some_and(|f| f.vertical) {
            self.loss(
                LossKind::VerticalText,
                "vertical text laid out horizontally",
            );
        }
        if !matches!(gs.render_mode, 0 | 4) {
            self.loss(
                LossKind::Other,
                "text stroke rendering approximated as fill",
            );
        }
        let line = mul(&start, &gs.ctm);
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
            fill: if gs.render_mode == 1 {
                gs.stroke.clone()
            } else {
                gs.fill.clone()
            },
            opacity: if gs.render_mode == 1 {
                gs.stroke_alpha
            } else {
                gs.fill_alpha
            },
            parent: gs.parent,
        })
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
        self.doc.create_object(Object {
            name: text,
            pos: [bounds[0] as f32, bounds[1] as f32],
            size: [
                (bounds[2] - bounds[0]).max(0.0) as f32,
                (bounds[3] - bounds[1]).max(0.0) as f32,
            ],
            fill: first.fill,
            opacity: first.opacity,
            kind: if framed {
                ObjectKind::FrameText { story: story_id }
            } else {
                ObjectKind::ArtText { story: story_id }
            },
            parent_id: first.parent,
            ..Object::default()
        });
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
