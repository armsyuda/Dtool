use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::commands::selection_modify::{Adjustment, MAX_RADIUS, Operation};
use dtool_core::selection::{OutlineAlign, PixelSelection};
use dtool_core::workspace::ContextId;
use dtool_core::{Command, CommandError, Session};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, TextureHandle};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind { Extent, Feather, Smooth, Outline }

impl Kind {
    pub fn from_menu(id: &str) -> Option<Self> {
        match id {
            "pixel.pixel_selection.grow_shrink" => Some(Self::Extent),
            "pixel.pixel_selection.feather" => Some(Self::Feather),
            "pixel.pixel_selection.smooth" => Some(Self::Smooth),
            "pixel.pixel_selection.outline" => Some(Self::Outline),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Extent => "selection.modify.extent_title",
            Self::Feather => "selection.feather",
            Self::Smooth => "refine.smooth",
            Self::Outline => "selection.modify.outline_title",
        }
    }
}

pub enum Action { KeepOpen, Cancel, Apply(Command) }

pub struct ModifyDialog {
    pub owner: ContextId,
    pub generation: u64,
    pub error: Option<String>,
    kind: Kind,
    radius: f32,
    circular: bool,
    alignment: OutlineAlign,
    base: PixelSelection,
    scale: f32,
    preview: Option<(Adjustment, TextureHandle)>,
}

impl ModifyDialog {
    pub fn new(owner: ContextId, session: &Session, kind: Kind) -> Result<Self, CommandError> {
        let mask = session.document.pixel_selection.as_ref().filter(|mask| !mask.is_empty())
            .ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
        let scale = (240.0 / mask.width.max(mask.height) as f32).min(1.0);
        let [w, h] = [mask.width, mask.height].map(|side| (side as f32 * scale).round().max(1.0) as u32);
        let mut base = PixelSelection::empty(w, h);
        for y in 0..h {
            for x in 0..w {
                let sx = ((f64::from(x) + 0.5) * f64::from(mask.width) / f64::from(w)) as u32;
                let sy = ((f64::from(y) + 0.5) * f64::from(mask.height) / f64::from(h)) as u32;
                base.coverage[(y * w + x) as usize] = mask.get(sx, sy);
            }
        }
        Ok(Self {
            owner, generation: session.generation, error: None, kind, base, scale,
            radius: if kind == Kind::Extent { 0.0 } else { 1.0 },
            circular: false, alignment: OutlineAlign::Centre, preview: None,
        })
    }

    fn adjustment(&self) -> Adjustment {
        let operation = match self.kind {
            Kind::Extent if self.radius < 0.0 => Operation::Shrink,
            Kind::Extent => Operation::Grow,
            Kind::Feather => Operation::Feather,
            Kind::Smooth => Operation::Smooth,
            Kind::Outline => Operation::Outline,
        };
        Adjustment { operation, radius: self.radius.abs(), circular: self.circular, alignment: self.alignment }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Action {
        let (result, (apply, cancel)) = show_modal(
            ctx, "pixel.selection.modify", text(self.kind.title()), ClosePolicy::CancelOnly, true, |ui| {
                ui.set_width((ctx.content_rect().width() - 32.0).clamp(240.0, 340.0));
                self.show_preview(ui);
                let extent = self.kind == Kind::Extent;
                let label = ui.label(text(if self.kind == Kind::Outline { "selection.width" } else { "selection.modify.radius" }));
                let lower = if extent { -MAX_RADIUS } else { 0.0 };
                let range = lower..=MAX_RADIUS;
                ui.horizontal(|ui| {
                    ui.spacing_mut().slider_width = (ui.available_width() - 78.0).max(125.0);
                    let bounds = if extent { -50.0..=50.0 } else { 0.0..=50.0 };
                    let mut shown = self.radius.clamp(*bounds.start(), *bounds.end());
                    let slider = egui::Slider::new(&mut shown, bounds).show_value(false).trailing_fill(true).step_by(1.0);
                    let response = ui.add(slider).labelled_by(label.id);
                    if response.changed() { self.radius = shown; }
                    ui.add(egui::DragValue::new(&mut self.radius).range(range).speed(0.5).max_decimals(2).suffix(" px"))
                        .labelled_by(label.id);
                });
                if extent {
                    dtool_design::toggle(ui, &mut self.circular, text("selection.modify.circular"));
                }
                if self.kind == Kind::Outline {
                    ui.horizontal(|ui| {
                        ui.label(text("selection.modify.alignment"));
                        for (alignment, key) in [
                            (OutlineAlign::Outside, "selection.modify.outside"),
                            (OutlineAlign::Centre, "selection.modify.centre"),
                            (OutlineAlign::Inside, "selection.modify.inside"),
                        ] {
                            ui.selectable_value(&mut self.alignment, alignment, text(key));
                        }
                    });
                }
                if self.preview.as_ref().is_some_and(|(value, _)| *value != self.adjustment()) { ctx.request_repaint(); }
                if let Some(error) = &self.error { ui.label(text(error)); }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                    let apply = dtool_design::button(ui, text("button.apply")).clicked();
                    (apply, cancel)
                }).inner
            },
        );
        if cancel || result == DialogResult::Cancel { Action::Cancel }
        else if apply || result == DialogResult::Confirm { Action::Apply(self.adjustment().command()) }
        else { Action::KeepOpen }
    }

    fn show_preview(&mut self, ui: &mut egui::Ui) {
        let adjustment = self.adjustment();
        if self.preview.as_ref().is_none_or(|(last, _)| *last != adjustment) {
            let mut mask = self.base.clone();
            let mut sample = adjustment;
            sample.radius *= self.scale;
            sample.edit(&mut mask);
            let rgb = mask.coverage.iter().map(|&alpha| {
                let value = 24 + ((u16::from(alpha) * 216 + 127) / 255) as u8;
                Color32::from_gray(value)
            }).collect();
            let image = egui::ColorImage::new([mask.width as usize, mask.height as usize], rgb);
            if let Some((last, texture)) = &mut self.preview {
                *last = adjustment;
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.preview = Some((adjustment, ui.ctx().load_texture("selection.modify.preview", image, egui::TextureOptions::LINEAR)));
            }
        }
        ui.label(text("refine.preview"));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 128.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, Color32::from_gray(20));
        if let Some((_, texture)) = &self.preview {
            let size = texture.size_vec2();
            let fit = (rect.width() / size.x).min(rect.height() / size.y);
            ui.painter().image(texture.id(), Rect::from_center_size(rect.center(), size * fit),
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}
