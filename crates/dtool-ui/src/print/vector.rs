use dtool_core::{
    CanvasFlags, Document, ObjectKind,
    colour::{ColourModel, ProfileStore},
    paint::{Colour, Paint, Transparency},
    scene::{self, Prim, Scene},
    text_lines,
};
use dtool_protocol::CommandError;
use windows_sys::Win32::{
    Foundation::{COLORREF, POINT},
    Graphics::Gdi::{
        ALTERNATE, CreateSolidBrush, DeleteObject, GetStockObject, HDC, IntersectClipRect,
        NULL_PEN, NULLREGION, PolyPolygon, RestoreDC, SaveDC, SelectObject, SetPolyFillMode,
    },
};

use super::{
    sheet::Region,
    windows::{Paper, PrintArea},
};

pub(super) struct VectorFills {
    fills: Vec<Fill>,
    pub reference: Option<Scene>,
}

struct Fill {
    points: Vec<[f32; 2]>,
    counts: Vec<i32>,
    colour: COLORREF,
}

impl VectorFills {
    pub(super) fn prepare(doc: &Document, pages: &[usize]) -> Option<Self> {
        if doc.colour.profile != ProfileStore::SRGB || !doc.layer_masks.is_empty() {
            return None;
        }
        dtool_render::scene::install();
        let mut reference = scene::build_scene(doc, &CanvasFlags::default());
        let members = dtool_export::page_members(doc, pages);
        dtool_export::retain_page_scene(&mut reference, pages, &members);
        let note_fills = dtool_export::output_note_fills(doc, &reference);
        reference.prims.retain(|prim| !matches!(prim, Prim::Mark { .. } | Prim::Handle { .. }));
        reference.prims.extend(note_fills);
        let mut fills = Vec::new();
        let primitive_count = reference.prims.len();
        let mut clip_depth = 0usize;
        reference.prims.retain(|prim| {
            match prim {
                Prim::Page { .. } => true,
                Prim::Clip { rect, .. } => {
                    clip_depth = if rect.is_some() { clip_depth + 1 } else { clip_depth.saturating_sub(1) };
                    false
                }
                _ if clip_depth > 0 => false,
                _ => Fill::append_primitive(doc, prim, &mut fills).is_some(),
            }
        });
        // With no eligible shape, the normal raster path has nothing to replace.
        if fills.is_empty() { return None; }
        let reference = (reference.prims.len() != primitive_count).then_some(reference);
        Some(Self { fills, reference })
    }

    pub(super) fn draw(
        &self,
        dc: HDC,
        (x, y, width, height): Region,
        paper: Paper,
        cell: PrintArea,
        origin: [f64; 2],
        size: [f64; 2],
    ) -> Result<Option<bool>, CommandError> {
        let map = |axis: usize, value: f64| {
            let value = (value - f64::from(paper.offset[axis])).round();
            // GDI region coordinates are signed 27-bit integers.
            (value.is_finite() && (-67_108_864.0..=67_108_863.0).contains(&value)).then_some(value as i32)
        };
        let target_start = [map(0, origin[0]), map(1, origin[1])];
        let target_end = [map(0, origin[0] + size[0]), map(1, origin[1] + size[1])];
        let ([Some(left), Some(top)], [Some(right), Some(bottom)]) = (target_start, target_end) else {
            return Ok(None);
        };
        let [cell_left, cell_top] = [0, 1].map(|axis| cell.origin[axis] - paper.offset[axis]);
        let left = left.max(cell_left);
        let top = top.max(cell_top);
        let right = right.min(cell_left + cell.size[0]);
        let bottom = bottom.min(cell_top + cell.size[1]);
        if left >= right || top >= bottom {
            return Ok(Some(false));
        }
        let sx = size[0] / f64::from(width);
        let sy = size[1] / f64::from(height);
        let points = |fill: &Fill| fill.points.iter().map(|&[px, py]| {
            Some(POINT {
                x: map(0, origin[0] + (f64::from(px) - f64::from(x)) * sx)?,
                y: map(1, origin[1] + (f64::from(py) - f64::from(y)) * sy)?,
            })
        }).collect::<Option<Vec<_>>>();
        let Some(points) = self.fills.iter().map(points).collect::<Option<Vec<_>>>() else {
            return Ok(None);
        };
        let ok = unsafe {
            let state = SaveDC(dc);
            if state == 0 { return Err(print_error()); }
            let region = IntersectClipRect(dc, left, top, right, bottom);
            let drawn = if region == NULLREGION as i32 {
                Some(false)
            } else if region == 0 || SetPolyFillMode(dc, ALTERNATE) == 0
                || SelectObject(dc, GetStockObject(NULL_PEN)).is_null()
            {
                None
            } else {
                self.fills.iter().zip(points).all(|(fill, points)| draw_fill(dc, fill, &points)).then_some(true)
            };
            if RestoreDC(dc, state) == 0 { return Err(print_error()); }
            drawn
        };
        ok.map(Some).ok_or_else(print_error)
    }
}

impl Fill {
    fn append_primitive(doc: &Document, prim: &Prim, fills: &mut Vec<Self>) -> Option<()> {
        let id = match prim {
            Prim::Fill { object, opacity: 1.0, .. }
            | Prim::Stroke { object, opacity: 1.0, .. }
            | Prim::Text { object, opacity: 1.0, .. } => *object,
            _ => return None,
        };
        let object = doc.get_object(id)?;
        if object.parent_id.is_some() || !object.clip_role.is_none()
            || object.vector_crop.is_some()
            || !dtool_core::layer_style::is_normal(&object.blend_mode)
            || object.blend_ranges.is_some()
            || object.icc.is_some()
            || object.effects.iter().any(|effect| effect.enabled)
            || !matches!(object.transparency, Transparency::None)
        {
            return None;
        }
        match prim {
            Prim::Fill { contours, paint, .. } => fills.push(Self::from_contours(contours.iter().map(Vec::as_slice), paint)?),
            Prim::Stroke { points, closed, width, paint, .. } => {
                if !matches!(paint, Paint::Solid { .. }) { return None; }
                let contours = scene::solid_stroke_contours(object, points, *closed, *width)?;
                fills.push(Self::from_contours(contours.iter().map(Vec::as_slice), paint)?);
            }
            Prim::Text { lines, .. } if matches!(object.kind, ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_)) => {
                let outlines = text_lines::text_outlines(doc, id)?;
                let decorations = text_lines::text_decorations(doc, id);
                let decoration_fills = |under_text| decorations.iter()
                    .filter(move |item| item.under_text == under_text)
                    .map(|item| {
                        let [x, y, w, h] = item.rect;
                        if w <= 0.0 || h <= 0.0 { return None; }
                        let contour = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
                        Self::from_contours([contour.as_slice()], &item.paint)
                    });
                let groups = scene::glyph_groups(lines, outlines);
                let glyphs = lines.iter().zip(&groups).filter(|(_, contours)| !contours.is_empty())
                    .map(|(line, contours)| Self::from_contours(contours.iter().map(Vec::as_slice), &line.paint));
                // Keep the entire text primitive on the raster path if any part cannot be represented.
                let text = decoration_fills(true).chain(glyphs).chain(decoration_fills(false))
                    .collect::<Option<Vec<_>>>()?;
                if text.is_empty() { return None; }
                fills.extend(text);
            }
            _ => return None,
        }
        Some(())
    }

    fn from_contours<'a>(contours: impl IntoIterator<Item = &'a [[f32; 2]]>, paint: &Paint) -> Option<Self> {
        let Paint::Solid { colour: Colour { r, g, b, a: 255, model: ColourModel::Rgb, .. } } = paint else { return None };
        let mut counts = Vec::new();
        let mut points = Vec::new();
        for contour in contours.into_iter().filter(|points| points.len() >= 3) {
            if !contour.iter().flatten().all(|value| value.is_finite()) {
                return None;
            }
            counts.push(i32::try_from(contour.len()).ok()?);
            points.extend_from_slice(contour);
        }
        let colour = u32::from_le_bytes([*r, *g, *b, 0]);
        (!counts.is_empty()).then_some(Self { points, counts, colour })
    }
}

fn draw_fill(dc: HDC, fill: &Fill, points: &[POINT]) -> bool {
    unsafe {
        let brush = CreateSolidBrush(fill.colour);
        if brush.is_null() { return false; }
        let old = SelectObject(dc, brush);
        let drawn = !old.is_null() && PolyPolygon(dc, points.as_ptr(), fill.counts.as_ptr(), fill.counts.len() as i32) != 0;
        if !old.is_null() { SelectObject(dc, old); }
        DeleteObject(brush) != 0 && drawn
    }
}

fn print_error() -> CommandError {
    CommandError::new("PRINT_FAILED")
}
