use skrifa::outline::OutlinePen;

use super::{PdfDocument, PdfMapper, PdfObject, SubPath, dict_of, name_of};
use dtool_core::boolean::{self, BoolOp, FillRule};
use dtool_core::curve::{PathPoint, PointId};
use kurbo::{Affine, BezPath, Shape};
use lopdf::Dictionary;

pub(super) struct EmbeddedFont {
    pub(super) data: Vec<u8>,
    cid_map: Option<Vec<u8>>,
}

impl EmbeddedFont {
    pub(super) fn read(pdf: &PdfDocument, cid: &Dictionary) -> Option<Self> {
        if name_of(pdf, cid, b"Subtype")?.as_str() != "CIDFontType2" {
            return None;
        }
        let descriptor = dict_of(pdf, cid.get(b"FontDescriptor").ok()?)?;
        let data = read_stream(pdf, descriptor.get(b"FontFile2").ok()?, 16 * 1024 * 1024)?;
        let cid_map = match cid.get(b"CIDToGIDMap") {
            Err(_) => None,
            Ok(value) => match super::deref(pdf, value) {
                PdfObject::Name(name) if name == b"Identity" => None,
                _ => {
                    let mapping = read_stream(pdf, value, 2 * 65_536)?;
                    if mapping.len() % 2 != 0 {
                        return None;
                    }
                    Some(mapping)
                }
            },
        };
        Some(Self { data, cid_map })
    }

    pub(super) fn glyph_id(&self, cid: u16) -> Option<skrifa::GlyphId> {
        let gid = if let Some(bytes) = &self.cid_map {
            let index = 2 * usize::from(cid);
            u16::from_be_bytes(bytes.get(index..index + 2)?.try_into().ok()?)
        } else {
            cid
        };
        Some(skrifa::GlyphId::new(u32::from(gid)))
    }
}

fn read_stream(pdf: &PdfDocument, value: &PdfObject, limit: usize) -> Option<Vec<u8>> {
    let stream = super::deref(pdf, value).as_stream().ok()?;
    if stream.dict.has(b"Filter") {
        stream.decompressed_content_with_limit(limit).ok()
    } else {
        (stream.content.len() <= limit).then(|| stream.content.clone())
    }
}

pub(super) fn descendant<'a>(pdf: &'a PdfDocument, font: &'a Dictionary) -> Option<&'a Dictionary> {
    let fonts = super::deref(pdf, font.get(b"DescendantFonts").ok()?)
        .as_array()
        .ok()?;
    dict_of(pdf, fonts.first()?)
}

pub(super) struct GlyphPen {
    pub(super) path: Vec<SubPath>,
    current: Vec<PathPoint>,
    pub(super) x: f64,
    pub(super) h_scale: f64,
    points: usize,
}

impl GlyphPen {
    pub(super) fn new(h_scale: f64) -> Self {
        Self {
            path: Vec::new(),
            current: Vec::new(),
            x: 0.0,
            h_scale,
            points: 0,
        }
    }

    pub(super) fn exceeded_budget(&self) -> bool {
        self.points > 65_536
    }

    fn point(&self, x: f32, y: f32) -> [f64; 2] {
        [self.x + f64::from(x) * self.h_scale, f64::from(y)]
    }

    fn push(&mut self, pos: [f64; 2]) {
        self.points += 1;
        if !self.exceeded_budget() {
            self.current.push(PathPoint::sharp(PointId(0), pos));
        }
    }
}

impl OutlinePen for GlyphPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.push(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(self.point(x, y));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let control = self.point(cx, cy);
        let end = self.point(x, y);
        if let Some(last) = self.current.last_mut() {
            last.handle_out = Some(std::array::from_fn(|i| {
                last.pos[i] + (control[i] - last.pos[i]) * (2.0 / 3.0)
            }));
            let mut point = PathPoint::sharp(PointId(0), end);
            point.handle_in = Some(std::array::from_fn(|i| {
                end[i] + (control[i] - end[i]) * (2.0 / 3.0)
            }));
            self.points += 1;
            if !self.exceeded_budget() {
                self.current.push(point);
            }
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let handle_out = self.point(cx0, cy0);
        let handle_in = self.point(cx1, cy1);
        if let Some(last) = self.current.last_mut() {
            last.handle_out = Some(handle_out);
            self.line_to(x, y);
            if !self.exceeded_budget() {
                self.current.last_mut().unwrap().handle_in = Some(handle_in);
            }
        }
    }

    fn close(&mut self) {
        let mut points = std::mem::take(&mut self.current);
        if points.len() >= 2 {
            PdfMapper::join_closed_endpoints(&mut points);
            self.path.push(SubPath {
                points,
                closed: true,
                rect: false,
            });
        }
    }
}

pub(super) const INK_TOLERANCE: f64 = 0.015;

pub(super) struct TextInk {
    pub(super) path: BezPath,
    pub(super) is_stroke: bool,
}

pub(super) fn valid_ink(glyphs: &[Vec<TextInk>]) -> bool {
    glyphs.iter().flatten().all(|ink| {
        let bounds = ink.path.bounding_box();
        [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
    })
}

pub(super) fn expand_ink(
    outlines: BezPath,
    stroke: &dtool_core::paint::StrokeStyle,
    user_to_doc: Affine,
    fills: bool,
) -> Option<Vec<TextInk>> {
    let inverse = user_to_doc.inverse();
    if !inverse.as_coeffs().iter().all(|value| value.is_finite()) {
        return None;
    }
    let user_path = inverse * &outlines;
    let stroke_path: BezPath = boolean::expand_stroke(&user_path, stroke, INK_TOLERANCE)
        .ok()?
        .into_iter()
        .flat_map(|curve| (user_to_doc * curve.to_kurbo()).into_iter())
        .collect();
    let mut ink = Vec::new();
    if fills {
        let fill_path = if stroke_path.elements().is_empty() {
            outlines
        } else {
            subtract(outlines, std::slice::from_ref(&stroke_path))?
        };
        if !fill_path.elements().is_empty() {
            ink.push(TextInk {
                path: fill_path,
                is_stroke: false,
            });
        }
    }
    if !stroke_path.elements().is_empty() {
        ink.push(TextInk {
            path: stroke_path,
            is_stroke: true,
        });
    }
    Some(ink)
}

fn subtract(path: BezPath, clips: &[BezPath]) -> Option<BezPath> {
    let mut operands = vec![vec![path]];
    operands.extend(clips.iter().cloned().map(|clip| vec![clip]));
    Some(
        boolean::boolean_operands(
            &operands,
            BoolOp::Subtract,
            FillRule::NonZero,
            INK_TOLERANCE,
        )
        .ok()?
        .into_iter()
        .flat_map(|curve| curve.to_kurbo().into_iter())
        .collect(),
    )
}

pub(super) fn knock_out(glyphs: &mut [Vec<TextInk>]) -> Option<()> {
    let mut covered: Vec<(kurbo::Rect, BezPath)> = Vec::new();
    let mut comparisons = 0usize;
    for glyph in glyphs.iter_mut().rev() {
        let mut occupied = Vec::with_capacity(glyph.len());
        for ink in glyph {
            comparisons += covered.len();
            if comparisons > 2_000_000 {
                return None;
            }
            let bounds = ink.path.bounding_box();
            let clips: Vec<_> = covered
                .iter()
                .filter(|(other, _)| bounds.intersect(*other).area() > 0.0)
                .map(|(_, path)| path.clone())
                .collect();
            if !clips.is_empty() {
                ink.path = subtract(std::mem::take(&mut ink.path), &clips)?;
            }
            if !ink.path.elements().is_empty() {
                occupied.push((ink.path.bounding_box(), ink.path.clone()));
            }
        }
        covered.extend(occupied);
    }
    Some(())
}
