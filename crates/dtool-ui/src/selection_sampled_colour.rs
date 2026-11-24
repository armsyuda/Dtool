use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::commands::selection_magic_wand::{COLOUR_RANGE, colour_source_for_page};
use dtool_core::raster_view::RasterView;
use dtool_core::selection::colour_mask;
use dtool_core::workspace::ContextId;
use dtool_core::{Command, CommandError, Session};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, TextureHandle, TextureOptions};
use serde_json::json;

pub const MENU_ID: &str = "pixel.pixel_selection.select_sampled_colour";

pub enum Action { KeepOpen, Cancel, Apply(Command) }

pub struct SampledColourDialog {
    pub owner: ContextId,
    pub generation: u64,
    pub error: Option<String>,
    page_id: Option<dtool_core::document::PageId>,
    rgba: Vec<u8>,
    size: [u32; 2],
    preview_rgba: Vec<u8>,
    preview_size: [u32; 2],
    colour: Option<[u8; 4]>,
    tolerance: f32,
    preview: Option<(Option<[u8; 4]>, f32, TextureHandle)>,
}

impl SampledColourDialog {
    pub fn new(owner: ContextId, session: &Session, page_id: Option<dtool_core::document::PageId>) -> Result<Self, CommandError> {
        let (size, rgba) = colour_source_for_page(session, None, page_id)?;
        let scale = (320.0 / size[0].max(size[1]) as f32).min(1.0);
        let preview_size = size.map(|side| (side as f32 * scale).round().max(1.0) as u32);
        let mut preview_rgba = Vec::with_capacity((preview_size[0] * preview_size[1] * 4) as usize);
        let raster = RasterView::new(size[0], size[1], &rgba);
        for y in 0..preview_size[1] {
            for x in 0..preview_size[0] {
                let coordinate = |value, axis: usize| {
                    ((f64::from(value) + 0.5) * f64::from(size[axis]) / f64::from(preview_size[axis])) as u32
                };
                if let Some(pixel) = raster.pixel(coordinate(x, 0), coordinate(y, 1)) {
                    preview_rgba.extend_from_slice(&pixel);
                }
            }
        }
        Ok(Self { owner, generation: session.generation, error: None, page_id, rgba, size, preview_rgba,
            preview_size, colour: None, tolerance: 10.0, preview: None })
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Action {
        let (dialog, (apply, cancel)) = show_modal(
            ctx, "pixel.selection.sampled_colour", text("menu.pixel.pixel_selection.select_sampled_colour"),
            ClosePolicy::CancelOnly, true, |ui| {
                ui.set_width((ctx.content_rect().width() - 32.0).clamp(240.0, 380.0));
                ui.label(text("selection.sampled_colour.pick"));
                self.show_preview(ui);
                ui.horizontal(|ui| {
                    let mut rgba = self.colour.unwrap_or([0, 0, 0, 255]);
                    let mut rgb = [rgba[0], rgba[1], rgba[2]];
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        rgba = [rgb[0], rgb[1], rgb[2], 255];
                        self.colour = Some(rgba);
                    }
                    ui.label(self.colour.map_or_else(|| "—".into(), |[r, g, b, _]| format!("#{r:02X}{g:02X}{b:02X}")));
                });
                ui.add(egui::Slider::new(&mut self.tolerance, 0.0..=100.0).step_by(1.0)
                    .suffix("%").text(text("selection.tolerance")));
                if self.preview.as_ref().is_some_and(|(colour, tolerance, _)| {
                    *colour != self.colour || *tolerance != self.tolerance
                }) {
                    ctx.request_repaint();
                }
                if let Some(error) = &self.error { ui.label(text(error)); }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                    let apply = ui.add_enabled(self.colour.is_some(), egui::Button::new(text("button.apply"))).clicked();
                    (apply, cancel)
                }).inner
            },
        );
        if cancel || dialog == DialogResult::Cancel {
            Action::Cancel
        } else if (apply || dialog == DialogResult::Confirm) && let Some(colour) = self.colour {
            Action::Apply(Command::with_params(COLOUR_RANGE, json!({"color": colour, "tolerance": self.tolerance,
                "page_id": self.page_id})))
        } else {
            Action::KeepOpen
        }
    }

    fn show_preview(&mut self, ui: &mut egui::Ui) {
        let size = self.preview_size;
        if self.preview.as_ref().is_none_or(|(colour, tolerance, _)| *colour != self.colour || *tolerance != self.tolerance) {
            let selected = self.colour.map(|colour| colour_mask(
                &RasterView::new(size[0], size[1], &self.preview_rgba), colour, self.tolerance,
            ));
            let colours = self.preview_rgba.chunks_exact(4).enumerate().map(|(index, rgba)| {
                let [r, g, b, a] = [rgba[0], rgba[1], rgba[2], rgba[3]];
                let composite = |c: u8| ((u16::from(c) * u16::from(a) + 238 * u16::from(255 - a) + 127) / 255) as u8;
                let amount = selected.as_ref().map_or(0, |mask| u16::from(mask.coverage[index]) / 2);
                let tint = |base: u8, accent: u8| ((u16::from(base) * (255 - amount) + u16::from(accent) * amount + 127) / 255) as u8;
                Color32::from_rgb(tint(composite(r), 32), tint(composite(g), 192), tint(composite(b), 238))
            }).collect();
            let image = egui::ColorImage::new(size.map(|v| v as usize), colours);
            if let Some((colour, tolerance, texture)) = &mut self.preview {
                *colour = self.colour;
                *tolerance = self.tolerance;
                texture.set(image, TextureOptions::NEAREST);
            } else {
                self.preview = Some((self.colour, self.tolerance,
                    ui.ctx().load_texture("selection.sampled_colour", image, TextureOptions::NEAREST)));
            }
        }

        let [w, h] = size.map(|side| side as f32);
        let scale = (ui.available_width() / w).min(210.0 / h);
        let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h * scale), egui::Sense::hover());
        let image = Rect::from_center_size(area.center(), egui::vec2(w, h) * scale);
        let hover = ui.interact(image, ui.id().with("pick_sampled_colour"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::Crosshair);
        if let Some((_, _, texture)) = &self.preview {
            ui.painter().image(texture.id(), image, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        if hover.clicked() && let Some(at) = hover.interact_pointer_pos() {
            let x = ((at.x - image.min.x) / image.width() * self.size[0] as f32) as u32;
            let y = ((at.y - image.min.y) / image.height() * self.size[1] as f32) as u32;
            let source = RasterView::new(self.size[0], self.size[1], &self.rgba);
            if let Some([r, g, b, a]) = source.pixel(x, y) && a != 0 {
                self.colour = Some([r, g, b, 255]);
            }
        }
    }
}
