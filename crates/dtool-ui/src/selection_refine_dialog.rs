use dtool_core::commands::selection_refine::{capture, REFINE};
use dtool_core::raster_view::RasterView;
use dtool_core::selection::refinement::{refine, RefineBrush, RefineStroke};
use dtool_core::selection::{PixelSelection, RefineOutput, RefinePreview, RefineSettings};
use dtool_core::workspace::ContextId;
use dtool_core::{Command, CommandError, Session};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, Response, Ui};
use serde_json::json;

pub const MENU_ID: &str = "pixel.pixel_selection.refine_edges";

pub enum Action {
    KeepOpen,
    Cancel,
    Apply(Command),
}

pub struct RefineDialog {
    pub owner: ContextId,
    pub generation: u64,
    pub error: Option<String>,
    original: PixelSelection,
    rgba: Vec<u8>,
    document_size: [u32; 2],
    preview: RefinePreview,
    output: RefineOutput,
    matte: bool,
    border: f32,
    smooth: f32,
    feather: f32,
    ramp: f32,
    decontaminate: bool,
    brush: RefineBrush,
    brush_width: f32,
    strokes: Vec<RefineStroke>,
    stroke: Option<RefineStroke>,
    can_mask: bool,
    dirty: bool,
    texture: Option<egui::TextureHandle>,
}

impl RefineDialog {
    pub fn new(owner: ContextId, session: &Session) -> Result<Self, CommandError> {
        let mask = session.document.pixel_selection.as_ref().filter(|mask| !mask.is_empty())
            .ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
        let document_size = [mask.width, mask.height];
        let scale = (480.0 / mask.width.max(mask.height) as f32).min(1.0);
        let [w, h] = document_size.map(|side| (side as f32 * scale).round().max(1.0) as u32);
        let rgba = capture(&session.document, [w, h])?;
        let mut original = PixelSelection::empty(w, h);
        for y in 0..h {
            for x in 0..w {
                let sx = ((f64::from(x) + 0.5) * f64::from(mask.width) / f64::from(w)) as u32;
                let sy = ((f64::from(y) + 0.5) * f64::from(mask.height) / f64::from(h)) as u32;
                original.coverage[(y * w + x) as usize] = mask.get(sx, sy);
            }
        }
        let can_mask = session.selection.ids.iter().any(|id| session.document.get_object(*id).is_some_and(|object| !object.locked));
        Ok(Self {
            owner, generation: session.generation, error: None,
            original, rgba, document_size, preview: RefinePreview::Overlay, output: RefineOutput::Selection,
            matte: true, border: 12.0, smooth: 0.0, feather: 0.0, ramp: 0.0,
            decontaminate: false, brush: RefineBrush::Matte, brush_width: 50.0,
            strokes: Vec::new(), stroke: None, can_mask, dirty: true, texture: None,
        })
    }

    fn settings(&self) -> RefineSettings {
        RefineSettings {
            border_width: if self.matte { self.border } else { 0.0 },
            smooth: self.smooth,
            feather: self.feather,
            ramp: (-self.ramp / 50.0).exp2(),
            decontaminate: self.decontaminate && matches!(self.output, RefineOutput::NewLayer | RefineOutput::NewLayerWithMask),
            preview: self.preview,
            output: self.output,
        }
    }

    fn command(&self) -> Command {
        let settings = self.settings();
        Command::with_params(REFINE, json!({
            "border_width": settings.border_width,
            "smooth": settings.smooth,
            "feather": settings.feather,
            "ramp": settings.ramp,
            "decontaminate": settings.decontaminate,
            "output": settings.output,
            "strokes": self.strokes,
        }))
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Action {
        use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};
        self.update_texture(ctx);
        let horizontal = ctx.content_rect().width() >= 610.0 && ctx.content_rect().height() >= 500.0;
        let (result, (apply, cancel)) = show_modal(ctx, "pixel.refine", text("refine.title"), ClosePolicy::CancelOnly, true, |ui| {
            if horizontal {
                ui.horizontal_top(|ui| {
                    let width = (ctx.content_rect().width() - 340.0).clamp(220.0, 440.0);
                    self.show_preview(ui, egui::vec2(width, 390.0));
                    ui.add_space(10.0);
                    ui.vertical(|ui| self.controls(ui));
                });
            } else {
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 130.0).max(130.0)).show(ui, |ui| {
                    let size = (ctx.content_rect().width() - 60.0).clamp(160.0, 340.0);
                    self.show_preview(ui, egui::vec2(size, 240.0));
                    self.controls(ui);
                });
            }
            if let Some(error) = &self.error {
                ui.colored_label(dtool_design::color("text"), text(error));
            }
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                let apply = dtool_design::button(ui, text("refine.apply")).clicked();
                (apply, cancel)
            }).inner
        });
        if self.dirty { ctx.request_repaint(); }
        if cancel || result == DialogResult::Cancel { Action::Cancel }
        else if apply || result == DialogResult::Confirm { Action::Apply(self.command()) }
        else { Action::KeepOpen }
    }

    fn controls(&mut self, ui: &mut Ui) {
        ui.set_width(244.0);
        ui.spacing_mut().slider_width = 170.0;
        let previews = [
            (RefinePreview::Overlay, text("refine.preview.overlay")),
            (RefinePreview::BlackMatte, text("refine.preview.black_matte")),
            (RefinePreview::WhiteMatte, text("refine.preview.white_matte")),
            (RefinePreview::BlackAndWhite, text("refine.preview.black_and_white")),
            (RefinePreview::Transparent, text("refine.preview.transparent")),
        ];
        ui.label(text("refine.preview"));
        self.dirty |= dtool_design::dropdown_value(ui, "refine.preview", &mut self.preview, &previews, &Default::default()).changed();
        self.dirty |= ui.checkbox(&mut self.matte, text("refine.matte_edges")).changed();
        self.dirty |= ui.add_enabled_ui(self.matte, |ui| {
            slider(ui, "refine.border_width", &mut self.border, 0.0..=100.0, " px")
        }).inner;
        self.dirty |= slider(ui, "refine.smooth", &mut self.smooth, 0.0..=64.0, " px");
        self.dirty |= slider(ui, "selection.feather", &mut self.feather, 0.0..=64.0, " px");
        self.dirty |= slider(ui, "refine.ramp", &mut self.ramp, -100.0..=100.0, "%");
        ui.separator();
        ui.label(text("refine.adjustment_brush"));
        ui.horizontal_wrapped(|ui| {
            for (brush, key) in [
                (RefineBrush::Matte, "refine.matte"), (RefineBrush::Foreground, "refine.foreground"),
                (RefineBrush::Background, "refine.background"), (RefineBrush::Feather, "selection.feather"),
            ] {
                ui.selectable_value(&mut self.brush, brush, text(key));
            }
        });
        slider(ui, "selection.width", &mut self.brush_width, 1.0..=256.0, " px");
        if ui.add_enabled(!self.strokes.is_empty(), egui::Button::new(text("refine.undo_stroke"))).clicked() {
            self.strokes.pop();
            self.dirty = true;
        }
        ui.separator();
        let mut outputs = vec![(RefineOutput::Selection, text("refine.output.selection"))];
        if self.can_mask { outputs.push((RefineOutput::Mask, text("refine.output.mask"))); }
        outputs.extend([
            (RefineOutput::NewLayer, text("refine.output.new_layer")),
            (RefineOutput::NewLayerWithMask, text("refine.output.new_layer_with_mask")),
        ]);
        ui.label(text("refine.output"));
        dtool_design::dropdown_value(ui, "refine.output", &mut self.output, &outputs, &Default::default());
        let pixels = matches!(self.output, RefineOutput::NewLayer | RefineOutput::NewLayerWithMask);
        ui.add_enabled(pixels, egui::Checkbox::new(&mut self.decontaminate, text("refine.decontaminate")));
    }

    fn show_preview(&mut self, ui: &mut Ui, size: egui::Vec2) {
        let (space, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().rect_filled(space, 4.0, Color32::from_gray(25));
        let aspect = egui::vec2(self.original.width as f32, self.original.height as f32);
        let size = aspect * (size.x / aspect.x).min(size.y / aspect.y);
        let image = Rect::from_center_size(space.center(), size);
        let response = ui.interact(image, ui.scope_id().with("refine.canvas"), egui::Sense::click_and_drag());
        if let Some(texture) = &self.texture {
            ui.painter().image(texture.id(), image, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        self.brush_input(ui, &response, image);
        let painter = ui.painter_at(image);
        let width = self.brush_width * image.width() / self.document_size[0] as f32;
        if let Some(stroke) = &self.stroke {
            let points: Vec<_> = stroke.points.iter().map(|point| egui::pos2(
                image.left() + point[0] * image.width() / self.document_size[0] as f32,
                image.top() + point[1] * image.height() / self.document_size[1] as f32,
            )).collect();
            let tint = Color32::from_rgba_unmultiplied(237, 154, 74, 115);
            if let [point] = points.as_slice() {
                painter.circle_filled(*point, width * 0.5, tint);
            } else if points.len() > 1 {
                painter.add(egui::Shape::line(points, egui::Stroke::new(width, tint)));
            }
        }
        if let Some(at) = response.hover_pos() {
            painter.circle_stroke(at, (width * 0.5).max(2.0), egui::Stroke::new(2.0, Color32::BLACK));
            painter.circle_stroke(at, (width * 0.5).max(2.0), egui::Stroke::new(1.0, Color32::WHITE));
        }
    }

    fn brush_input(&mut self, ui: &Ui, response: &Response, image: Rect) {
        if self.strokes.len() >= 128 { return; }
        let [w, h] = self.document_size.map(|side| side as f32);
        let point = |at: Pos2| {
            let position = at.clamp(image.min, image.max) - image.min;
            [position.x * w / image.width(), position.y * h / image.height(), 1.0]
        };
        let append = |stroke: &mut RefineStroke, at| {
            let at = point(at);
            if stroke.points.last() != Some(&at) { stroke.points.push(at); }
        };
        use egui::{Event, PointerButton};
        for event in ui.input(|input| input.events.clone()) {
            match event {
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, .. }
                    if response.enabled() && response.interact_rect.contains(pos)
                        && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                        && !egui::Popup::is_any_open(ui.ctx()) =>
                {
                    self.stroke = Some(RefineStroke {
                        brush: self.brush, width: self.brush_width, points: vec![point(pos)],
                    });
                }
                Event::PointerMoved(pos) => {
                    if let Some(stroke) = &mut self.stroke
                        && stroke.points.len() < dtool_core::commands::pixel_brush::MAX_POINTS - 1
                    {
                        append(stroke, pos);
                    }
                }
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. } => {
                    if let Some(mut stroke) = self.stroke.take() {
                        append(&mut stroke, pos);
                        self.strokes.push(stroke);
                        self.dirty = true;
                    }
                }
                Event::PointerGone => self.stroke = None,
                _ => {}
            }
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            self.stroke = None;
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        if !self.dirty { return; }
        self.dirty = false;
        let [w, h] = [self.original.width, self.original.height];
        let scale = w as f32 / self.document_size[0] as f32;
        let sy = h as f32 / self.document_size[1] as f32;
        let mut settings = self.settings();
        settings.border_width *= scale;
        settings.feather *= scale;
        settings.smooth *= scale;
        let mut strokes = self.strokes.clone();
        for stroke in &mut strokes {
            stroke.width *= scale;
            for point in &mut stroke.points {
                point[0] *= scale;
                point[1] *= sy;
            }
        }
        let source = RasterView::new(w, h, &self.rgba);
        let mask = refine(&self.original, &source, &settings, &strokes, 0.0);
        let pixels = self.rgba.chunks_exact(4).zip(mask.coverage).enumerate().map(|(i, (pixel, mask))| {
            let checker = if ((i % w as usize) / 8 + (i / w as usize) / 8) % 2 == 0 { 206 } else { 160 };
            preview_pixel(pixel, mask, settings.preview, checker)
        }).collect();
        let image = egui::ColorImage::new([w as usize, h as usize], pixels);
        if let Some(texture) = &mut self.texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture("pixel.refine.preview", image, egui::TextureOptions::LINEAR));
        }
    }
}

fn slider(ui: &mut Ui, key: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) -> bool {
    let label = ui.label(text(key));
    ui.add(egui::Slider::new(value, range).integer().suffix(suffix).trailing_fill(true)).labelled_by(label.id).changed()
}

fn preview_pixel(src: &[u8], mask: u8, mode: RefinePreview, checker: u8) -> Color32 {
    let alpha = f32::from(src[3]) / 255.0;
    let coverage = f32::from(mask) / 255.0;
    let blend = |from: u8, to: u8, t: f32| (f32::from(from) * (1.0 - t) + f32::from(to) * t).round() as u8;
    let rgb: [u8; 3] = std::array::from_fn(|i| match mode {
        RefinePreview::Overlay => blend(blend(255, src[i], alpha), [226, 54, 70][i], 0.55 * (1.0 - coverage)),
        RefinePreview::BlackMatte => blend(0, src[i], alpha * coverage),
        RefinePreview::WhiteMatte => blend(255, src[i], alpha * coverage),
        RefinePreview::Transparent => blend(checker, src[i], alpha * coverage),
        RefinePreview::BlackAndWhite => mask,
    });
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
