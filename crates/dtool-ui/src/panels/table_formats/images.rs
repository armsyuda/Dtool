use dtool_core::Document;
use dtool_core::document::DocInstance;
use dtool_core::paint::Paint;
use egui::{Color32, Painter, Rect, TextureHandle};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Images {
    stamp: Option<(DocInstance, u64)>,
    textures: HashMap<String, Option<(TextureHandle, f32)>>,
}

impl Images {
    pub(super) fn sync(&mut self, doc: &Document) {
        let stamp = (doc.instance, doc.version);
        if self.stamp != Some(stamp) {
            self.textures.clear();
            self.stamp = Some(stamp);
        }
    }

    pub(super) fn fill(
        &mut self,
        painter: &Painter,
        rect: Rect,
        paint: Option<&Paint>,
        doc: &Document,
    ) {
        let resource = match paint {
            Some(Paint::Bitmap(paint)) => Some(&paint.resource_id),
            Some(Paint::Pattern(paint)) => Some(&paint.resource_id),
            _ => None,
        };
        if let Some(resource) = resource {
            let texture = self
                .textures
                .entry(resource.clone())
                .or_insert_with(|| thumbnail(painter.ctx(), doc, resource));
            if let Some((texture, aspect)) = texture {
                let target_aspect = rect.width() / rect.height().max(f32::EPSILON);
                let uv = if target_aspect < *aspect {
                    let span = target_aspect / *aspect;
                    Rect::from_min_max(
                        egui::pos2((1.0 - span) * 0.5, 0.0),
                        egui::pos2((1.0 + span) * 0.5, 1.0),
                    )
                } else {
                    let span = *aspect / target_aspect;
                    Rect::from_min_max(
                        egui::pos2(0.0, (1.0 - span) * 0.5),
                        egui::pos2(1.0, (1.0 + span) * 0.5),
                    )
                };
                painter.image(texture.id(), rect, uv, Color32::WHITE);
            }
        } else if let Some(colour) = paint.and_then(crate::fill_stroke::paint_well_colour) {
            painter.rect_filled(rect, 0.0, colour);
        }
    }
}

fn thumbnail(ctx: &egui::Context, doc: &Document, resource: &str) -> Option<(TextureHandle, f32)> {
    let ([width, height], pixels) = dtool_core::scene::bitmap_source(doc, resource)?;
    let scale = width.max(height).div_ceil(128);
    let size = [width.div_ceil(scale), height.div_ceil(scale)];
    let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let source = ((y * scale + scale / 2).min(height - 1) * width
                + (x * scale + scale / 2).min(width - 1))
                * 4;
            rgba.extend_from_slice(&pixels[source..source + 4]);
        }
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
    let texture = ctx.load_texture("table-format-image", image, egui::TextureOptions::LINEAR);
    Some((texture, width as f32 / height as f32))
}
