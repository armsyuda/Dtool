use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::canvas_size::{DOC_RESIZE_CANVAS, document_pixel_size, px_to_unit, unit_to_px};
use dtool_core::document::{Document, Units};
use dtool_core::workspace::ContextId;
use dtool_core::Command;
use dtool_strings::text;
use serde_json::json;

pub const MENU_ID: &str = "document.setup.resize.canvas";

pub(super) enum Action {
    Open,
    Cancel,
    Resize(Command),
}

pub(super) struct ResizeCanvasDialog {
    pub owner: ContextId,
    size: [f64; 2],
    units: Units,
    dpi: f32,
    anchor: u8,
    pub error: Option<String>,
}

const ANCHORS: [&str; 9] = [
    "top_left", "top", "top_right",
    "left", "center", "right",
    "bottom_left", "bottom", "bottom_right",
];

impl ResizeCanvasDialog {
    pub fn new(owner: ContextId, doc: &Document) -> Option<Self> {
        let (w, h) = document_pixel_size(doc).ok()?;
        Some(Self {
            owner,
            size: [w, h].map(|pixels| f64::from(px_to_unit(pixels as f32, doc.units, doc.dpi))),
            units: doc.units,
            dpi: doc.dpi,
            anchor: 4,
            error: None,
        })
    }

    fn pixels(&self) -> Option<[u32; 2]> {
        let px = self.size.map(|value| f64::from(unit_to_px(value as f32, self.units, self.dpi)).round());
        px.iter().all(|value| (1.0..=f64::from(u32::MAX)).contains(value))
            .then(|| px.map(|value| value as u32))
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Action {
        let can_confirm = self.pixels().is_some();
        let (result, (ok, cancel)) = show_modal(
            ctx, "dtool.resize_canvas", text("resize.canvas.title"), ClosePolicy::CancelOnly, can_confirm, |ui| {
                ui.set_min_width(290.0);
                egui::Grid::new("resize_canvas_fields").spacing([12.0, 6.0]).show(ui, |ui| {
                    for (axis, label) in ["dialog.new_document.width", "dialog.new_document.height"].into_iter().enumerate() {
                        let caption = ui.label(text(label));
                        dtool_design::number_drag(ui, ui.scope_id().with(axis), &mut self.size[axis], 0.001..=1_000_000_000.0, 0.1)
                            .labelled_by(caption.id);
                        ui.end_row();
                    }
                    ui.label(text("dialog.new_document.units"));
                    let mut units = self.units;
                    let options = [
                        (Units::Px, text("units.px")), (Units::Mm, text("units.mm")),
                        (Units::Cm, text("units.cm")), (Units::In, text("units.in")), (Units::Pt, text("units.pt")),
                    ];
                    if dtool_design::dropdown_value(ui, "resize-canvas-units", &mut units, &options, &Default::default()).changed() {
                        self.size = self.size.map(|value| f64::from(px_to_unit(
                            unit_to_px(value as f32, self.units, self.dpi), units, self.dpi,
                        )));
                        self.units = units;
                    }
                    ui.end_row();
                    ui.label(text("resize.anchor"));
                    egui::Grid::new("resize_canvas_anchor").spacing([3.0, 3.0]).show(ui, |ui| {
                        for (index, name) in ANCHORS.iter().enumerate() {
                            let glyph = if *name == "center" { "centre" } else { name };
                            let symbol = format!("panel.transform.anchor.{glyph}");
                            let label = format!("frame.anchor.{name}");
                            let selected = usize::from(self.anchor) == index;
                            let response = ui.add_sized([25.0, 21.0], egui::Button::new(text(&symbol)).selected(selected))
                                .on_hover_text(text(&label));
                            if response.clicked() { self.anchor = index as u8; }
                            if index % 3 == 2 { ui.end_row(); }
                        }
                    });
                    ui.end_row();
                });
                if let Some(error) = &self.error {
                    ui.label(text(error));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ok = ui.add_enabled_ui(self.pixels().is_some(), |ui| {
                        dtool_design::button(ui, text("menu.document.setup.resize")).clicked()
                    }).inner;
                    (ok, dtool_design::button(ui, text("dialog.cancel")).clicked())
                }).inner
            },
        );
        if cancel || result == DialogResult::Cancel {
            Action::Cancel
        } else if (ok || result == DialogResult::Confirm) && let Some([w, h]) = self.pixels() {
            Action::Resize(Command::with_params(DOC_RESIZE_CANVAS, json!({"w": w, "h": h, "anchor": self.anchor})))
        } else {
            Action::Open
        }
    }
}
