use crate::document::Document;
use dtool_protocol::CommandError;

#[derive(Clone, Debug, PartialEq)]
pub enum CropMode {
    Unconstrained,
    OriginalRatio,
    CustomRatio([f32; 2]),
    CustomSize([f32; 2]),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CropOverlay {
    None,
    Thirds,
    GoldenSpiral,
    Diagonals,
    PhiGrid,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CropPreset {
    pub name: String,
    pub mode: CropMode,
    pub dpi: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CropPreview {
    pub rect: [f32; 4],
    pub rotation_deg: f32,
    pub mode: CropMode,
    pub overlay: CropOverlay,
    pub darken: bool,
}


fn page_size(doc: &Document) -> Option<(f32, f32)> {
    doc.pages.first().map(|page| (page.size[0], page.size[1])).filter(|(w, h)| *w > 0.0 && *h > 0.0)
}

fn clamp_rect(rect: [f32; 4], w: f32, h: f32) -> [f32; 4] {
    let x0 = rect[0].max(0.0);
    let y0 = rect[1].max(0.0);
    let mut x1 = rect[2].min(w);
    let mut y1 = rect[3].min(h);

    if x1 <= x0 {
        x1 = x0 + 1.0;
    }
    if y1 <= y0 {
        y1 = y0 + 1.0;
    }

    [x0, y0, x1, y1]
}

fn apply_ratio(rect: [f32; 4], ratio_w: f32, ratio_h: f32) -> [f32; 4] {
    let mut x0 = rect[0];
    let mut y0 = rect[1];
    let mut x1 = rect[2];
    let mut y1 = rect[3];

    let w = x1 - x0;
    let h = y1 - y0;

    if ratio_w <= 0.0 || ratio_h <= 0.0 {
        return [x0, y0, x1, y1];
    }

    let target_ratio = ratio_w / ratio_h;
    let current_ratio = w / h;

    if (current_ratio - target_ratio).abs() > 1e-6 {
        let new_w = w.max(h * target_ratio);
        let new_h = new_w / target_ratio;

        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;

        x0 = cx - new_w * 0.5;
        x1 = cx + new_w * 0.5;
        y0 = cy - new_h * 0.5;
        y1 = cy + new_h * 0.5;
    }

    [x0, y0, x1, y1]
}

pub fn crop_preview(doc: &Document, p: &CropPreview) -> Result<CropPreview, CommandError> {
    let (w, h) = page_size(doc).ok_or_else(CommandError::no_document)?;

    let mut rect = clamp_rect(p.rect, w, h);

    match &p.mode {
        CropMode::Unconstrained => {}
        CropMode::OriginalRatio => {
            rect = apply_ratio(rect, w, h);
            rect = clamp_rect(rect, w, h);
        }
        CropMode::CustomRatio(r) => {
            rect = apply_ratio(rect, r[0], r[1]);
            rect = clamp_rect(rect, w, h);
        }
        CropMode::CustomSize(s) => {
            let target_w = s[0];
            let target_h = s[1];

            if target_w > 0.0 && target_h > 0.0 {
                let cx = (rect[0] + rect[2]) * 0.5;
                let cy = (rect[1] + rect[3]) * 0.5;

                rect[0] = cx - target_w * 0.5;
                rect[1] = cy - target_h * 0.5;
                rect[2] = cx + target_w * 0.5;
                rect[3] = cy + target_h * 0.5;

                rect = clamp_rect(rect, w, h);
            }
        }
    }

    Ok(CropPreview {
        rect,
        rotation_deg: p.rotation_deg,
        mode: p.mode.clone(),
        overlay: p.overlay.clone(),
        darken: p.darken,
    })
}

pub fn straighten_angle(line: [[f32; 2]; 2]) -> f32 {
    let dx = line[1][0] - line[0][0];
    let dy = line[1][1] - line[0][1];

    if dx.abs() < 1e-9 && dy.abs() < 1e-9 {
        return 0.0;
    }

    let angle_rad = dy.atan2(dx);
    let angle_deg = angle_rad.to_degrees();

    -angle_deg
}

pub fn apply_crop(doc: &mut Document, rect: [f32; 4], rotation_deg: f32) -> Result<(), CommandError> {
    let (w, h) = page_size(doc).ok_or_else(CommandError::no_document)?;

    let clamped = clamp_rect(rect, w, h);

    let new_w = (clamped[2] - clamped[0]).round().max(1.0);
    let new_h = (clamped[3] - clamped[1]).round().max(1.0);

    if let Some(page) = doc.pages.first_mut() {
        page.size = [new_w, new_h];
    }
    if rotation_deg != 0.0 {
        for object in &mut doc.objects {
            object.rotation_deg += rotation_deg;
        }
    }

    Ok(())
}
