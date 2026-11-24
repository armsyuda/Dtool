use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use crate::resize_canvas_dialog::Action;
use dtool_core::canvas_size::document_pixel_size;
use dtool_core::pixel_art::{DOC_RESIZE_PIXEL_ART, dimensions};
use dtool_core::workspace::ContextId;
use dtool_core::{Command, Document};
use dtool_strings::text;
use serde_json::json;

pub const MENU_ID: &str = "document.setup.resize.pixel_art_document";

pub(super) struct ResizePixelArtDialog {
    pub owner: ContextId,
    initial_size: [u32; 2],
    scale: u32,
    filter: &'static str,
    pub error: Option<String>,
}

impl ResizePixelArtDialog {
    pub fn new(owner: ContextId, doc: &Document) -> Option<Self> {
        let (width, height) = document_pixel_size(doc).ok()?;
        Some(Self {
            owner,
            initial_size: [width, height],
            scale: 2,
            filter: "hqx",
            error: None,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Action {
        let valid = dimensions(self.initial_size, self.scale).is_ok();
        let (result, (ok, cancel)) = show_modal(
            ctx, "dtool.resize_pixel_art", text("resize.pixel_art.title"), ClosePolicy::CancelOnly, valid, |ui| {
                ui.set_min_width(290.0);
                egui::Grid::new("resize_pixel_art_options").spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(text("resize.scale"));
                    let scales = [(2, "2×"), (3, "3×"), (4, "4×")];
                    if dtool_design::dropdown_value(ui, "pixel-art-scale", &mut self.scale, &scales, &Default::default()).changed() {
                        self.error = None;
                    }
                    ui.end_row();
                    ui.label(text("resize.filter"));
                    let filters = [("hqx", text("resize.pixel_art.hqx")), ("xbr", text("resize.pixel_art.xbr"))];
                    if dtool_design::dropdown_value(ui, "pixel-art-filter", &mut self.filter, &filters, &Default::default()).changed() {
                        self.error = None;
                    }
                    ui.end_row();
                });

                let target = dimensions(self.initial_size, self.scale);
                match &target {
                    Ok([width, height]) => { ui.label(format!("{width} × {height} {}", text("units.px"))); }
                    Err(error) => { ui.label(text(&error.message_key)); }
                }
                if let Some(error) = &self.error { ui.label(text(error)); }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ok = ui.add_enabled_ui(target.is_ok(), |ui| {
                        dtool_design::button(ui, text("menu.document.setup.resize")).clicked()
                    }).inner;
                    (ok, dtool_design::button(ui, text("dialog.cancel")).clicked())
                }).inner
            },
        );

        if cancel || result == DialogResult::Cancel {
            Action::Cancel
        } else if (ok || result == DialogResult::Confirm) && dimensions(self.initial_size, self.scale).is_ok() {
            Action::Resize(Command::with_params(DOC_RESIZE_PIXEL_ART, json!({
                "scale": format!("{}x", self.scale), "filter": self.filter,
            })))
        } else {
            Action::Open
        }
    }
}
