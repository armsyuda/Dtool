use dtool_core::commands::masters::{MAX_MASTER_PAGES, MasterPageSetup, master_name};
use dtool_core::units::ReadoutUnit;
use dtool_core::{Command, Document};
use dtool_design::{ControlState, NumericSpec};
use dtool_strings::text;
use egui::{Color32, Rect, Vec2, vec2};
use serde_json::json;

use crate::dialog_chrome::{DialogResult, confirm_cancel_buttons, options_divider, options_scroll, show_resizable_modal};

const PRESETS: [&str; 7] = ["A3", "A4", "A5", "B5 (JIS)", "Letter", "Legal", "Ledger"];

pub(super) struct Draft {
    name: String,
    size: [f32; 2],
    count: usize,
    spine: usize,
    show_spread: bool,
    lock_ratio: bool,
    tag: Option<&'static str>,
    setup: MasterPageSetup,
    margin_colour: Color32,
    use_scale: bool,
    scale: [f32; 2],
}

impl Draft {
    pub fn new(doc: &Document) -> Self {
        let names: Vec<_> = doc.master_pages.spread_roots()
            .filter_map(|id| doc.get_object(id)).map(master_name).collect();
        let name = (1..).map(|number| format!("{} {number}", text("panel.master")))
            .find(|name| !names.contains(&name.as_str())).unwrap_or_default();
        let mut margins = doc.setup.margins;
        if [margins.left, margins.right, margins.top, margins.bottom] == [0.0; 4] {
            let inset = 15.0 * dtool_core::doc_setup::unit_factor(dtool_core::document::Units::Mm, doc.dpi, doc.units, doc.dpi);
            let [width, height] = dtool_core::data_merge::layout::page_size(doc);
            let inset = inset.min(width.min(height) * 0.1);
            [margins.left, margins.right, margins.top, margins.bottom] = [inset; 4];
        }
        Self {
            name, size: dtool_core::data_merge::layout::page_size(doc), count: 1, spine: 1,
            show_spread: false, lock_ratio: false, tag: None,
            setup: MasterPageSetup { margins, measurement_unit: doc.measurement_unit, ..MasterPageSetup::default() },
            margin_colour: dtool_design::color("smart_guide"), use_scale: false, scale: doc.drawing_scale,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, doc: &Document) -> Option<DialogResult> {
        let focus = ctx.memory(|memory| memory.focused());
        let (event, (accept, cancel, ready, name_field)) = show_resizable_modal(
            ctx, "dtool.master.create", text("panel.master.add"), vec2(955.0, 685.0), vec2(295.0, 235.0),
            |ui, size| {
                let body = (size.y - dtool_design::size("button.height") - 15.0).max(30.0);
                let name_field = if size.x >= 620.0 {
                    let right = (size.x * 0.40).clamp(330.0, 390.0);
                    let left = size.x - right - ui.spacing().item_spacing.x;
                    ui.horizontal_top(|ui| {
                        self.preview(ui, vec2(left, body));
                        ui.allocate_ui_with_layout(vec2(right, body), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(right);
                            options_scroll(ui, "master.create.controls", body, |ui| self.controls(ui, doc))
                        }).inner
                    }).inner
                } else {
                    options_scroll(ui, "master.create.stacked", body, |ui| {
                        self.preview(ui, vec2(ui.available_width(), 115.0));
                        self.controls(ui, doc)
                    })
                };
                options_divider(ui);
                let ready = self.valid(doc);
                let (accept, cancel) = confirm_cancel_buttons(ui, text("common.ok"), ready);
                (accept, cancel, ready, name_field)
            },
        );
        if cancel || event == DialogResult::Cancel {
            Some(DialogResult::Cancel)
        } else if ready && (accept || crate::dialog_chrome::confirm_from_enter(ctx, event, focus, &[name_field])) {
            Some(DialogResult::Confirm)
        } else {
            None
        }
    }

    pub fn command(&self) -> Command {
        let mut setup = self.setup.clone();
        setup.margin_colour = Some(format!("#{:02X}{:02X}{:02X}", self.margin_colour.r(), self.margin_colour.g(), self.margin_colour.b()));
        setup.drawing_scale = self.use_scale.then_some(self.scale);
        let mut params = json!({"name": self.name, "size": self.size, "page_count": self.count,
            "colour": self.tag, "page_setup": setup});
        if self.count > 1 {
            params["spine"] = json!(self.spine);
        }
        Command::with_params("master.create", params)
    }

    fn valid(&self, doc: &Document) -> bool {
        let name = self.name.trim();
        !name.is_empty() && !name.chars().any(char::is_control)
            && dtool_core::commands::pages::valid_page_size(self.size)
            && !doc.master_pages.spread_roots().filter_map(|id| doc.get_object(id)).any(|root| master_name(root) == name)
    }

    fn controls(&mut self, ui: &mut egui::Ui, doc: &Document) -> egui::Id {
        let on = ControlState::enabled(true);
        let unit = dtool_core::units::suffix(doc.units);
        ui.heading(text("panel.master.label_section"));
        let name = ui.horizontal(|ui| {
            let label = column_label(ui, "panel.master.name");
            ui.add(egui::TextEdit::singleline(&mut self.name)
                .desired_width(ui.available_width()).char_limit(128)).labelled_by(label.id)
        }).inner;
        ui.horizontal_wrapped(|ui| {
            column_label(ui, "panel.pages.tag_colour");
            if let Some(tag) = super::super::layers::tag_colour_choices(ui, Some(self.tag)) {
                self.tag = tag;
            }
        });

        section_divider(ui);
        ui.heading(text("panel.master.layout_section"));
        ui.horizontal_wrapped(|ui| {
            column_label(ui, "panel.master.dimensions_of");
            ui.add_enabled_ui(self.count > 1, |ui| {
                dtool_design::choice_value(ui, &mut self.show_spread, false, text("panel.master.dimensions_page"));
                dtool_design::choice_value(ui, &mut self.show_spread, true, text("panel.master.dimensions_spread"));
            });
        });
        let papers = PRESETS.map(|key| (key, key));
        let mut preset = PRESETS.into_iter().find(|key| {
            let [w, h] = paper_size(doc, key);
            let [x, y] = self.size;
            ((x.min(y) - w.min(h)).abs() + (x.max(y) - w.max(h)).abs()) < 0.05
        }).unwrap_or("");
        let formats: Vec<_> = std::iter::once(("", text("export.svg.preset_custom"))).chain(papers).collect();
        ui.horizontal_wrapped(|ui| {
            column_label(ui, "panel.master.paper");
            if dtool_design::dropdown_value(ui, "master.paper", &mut preset, &formats, &on).changed() && !preset.is_empty() {
                let [w, h] = paper_size(doc, preset);
                self.size = if self.size[0] > self.size[1] { [w.max(h), w.min(h)] } else { [w.min(h), w.max(h)] };
            }
        });
        let multiplier = if self.show_spread { self.count as f32 } else { 1.0 };
        dtool_design::panel_columns(ui, 2, 118.0, |ui, axis| {
            let id = ["panel.master.width", "panel.master.height"][axis];
            let factor = if axis == 0 { multiplier } else { 1.0 };
            let mut value = self.size[axis] * factor;
            if number(ui, &mut value, id, unit, (0.01..=1_000_000.0 * factor, 2)) {
                let old = self.size[axis];
                self.size[axis] = value / factor;
                if self.lock_ratio {
                    self.size[1-axis] = (self.size[1-axis] * self.size[axis] / old).clamp(0.01, 1_000_000.0);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            dtool_design::toggle(ui, &mut self.lock_ratio, text("panel.master.lock_ratio"));
            ui.label(text("doc_setup.orientation"));
            let landscape = self.size[0] > self.size[1];
            for (option, label) in [(false, "doc_setup.portrait"), (true, "doc_setup.landscape")] {
                if crate::dialogs::orientation_button(ui, option, option == landscape, text(label)).clicked()
                    && option != landscape {
                    self.size.swap(0, 1);
                }
            }
        });
        let mut count = self.count as f32;
        if number(ui, &mut count, "panel.master.page_count", "", (1.0..=MAX_MASTER_PAGES as f32, 0)) {
            self.count = count as usize;
            self.spine = self.spine.clamp(1, self.count.saturating_sub(1).max(1));
            if self.count == 1 { self.show_spread = false; }
        }
        if self.count > 1 {
            let mut spine = self.spine as f32;
            number(ui, &mut spine, "panel.master.spine", "", (1.0..=(self.count - 1) as f32, 0));
            self.spine = spine as usize;
        }

        section_divider(ui);
        let m = &mut self.setup.margins;
        section_switch(ui, &mut m.enabled, "doc_setup.margins");
        ui.add_enabled_ui(m.enabled, |ui| {
            dtool_design::toggle(ui, &mut m.linked, text("doc_setup.margin.linked"));
            let mut sides = [m.left, m.right, m.top, m.bottom];
            let keys = ["doc_setup.margin.left", "doc_setup.margin.right", "doc_setup.margin.top", "doc_setup.margin.bottom"];
            dtool_design::panel_columns(ui, 2, 122.0, |ui, column| {
                for index in [column, column + 2] {
                    let mut value = sides[index];
                    if number(ui, &mut value, keys[index], unit, (0.0..=1_000_000.0, 2)) {
                        if m.linked { sides.fill(value); } else { sides[index] = value; }
                    }
                }
            });
            [m.left, m.right, m.top, m.bottom] = sides;
            ui.horizontal(|ui| {
                ui.label(text("panel.master.margin_colour"));
                egui::color_picker::color_edit_button_srgba(ui, &mut self.margin_colour, egui::color_picker::Alpha::Opaque);
            });
        });

        section_divider(ui);
        section_switch(ui, &mut self.use_scale, "ctx.measure.drawing_scale");
        if self.use_scale {
            dtool_design::panel_columns(ui, 2, 126.0, |ui, axis| {
                let key = ["panel.master.scale_drawing", "panel.master.scale_actual"][axis];
                number(ui, &mut self.scale[axis], key, "", (0.001..=1_000_000.0, 3));
            });
            let units = [(ReadoutUnit::Document, text("panel.master.unit_document")), (ReadoutUnit::Mm, "mm"),
                (ReadoutUnit::Cm, "cm"), (ReadoutUnit::M, "m"), (ReadoutUnit::In, "in"), (ReadoutUnit::Ft, "ft"),
                (ReadoutUnit::Px, "px"), (ReadoutUnit::Pt, "pt")];
            ui.horizontal_wrapped(|ui| {
                ui.label(text("panel.master.readout_units"));
                dtool_design::dropdown_value(ui, "master.readout", &mut self.setup.measurement_unit, &units, &on);
            });
        }
        name.id
    }

    fn preview(&self, ui: &mut egui::Ui, size: Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, dtool_design::radius("sm"), dtool_design::color("dialog.header"));
        let inside = rect.shrink(20.0);
        let gap = 3.0;
        let scale = ((inside.width() - gap * (self.count - 1) as f32) / (self.size[0] * self.count as f32))
            .min(inside.height() / self.size[1]).max(0.0);
        let page = vec2(self.size[0], self.size[1]) * scale;
        let width = page.x * self.count as f32 + gap * (self.count - 1) as f32;
        let start = inside.center() - vec2(width, page.y) * 0.5;
        let m = self.setup.margins;
        for index in 0..self.count {
            let x = start.x + index as f32 * (page.x + gap);
            let paper = Rect::from_min_size(egui::pos2(x, start.y), page);
            painter.rect_filled(paper, 1.0, Color32::from_gray(248));
            let margins = Rect::from_min_max(paper.min + vec2(m.left, m.top) * scale, paper.max - vec2(m.right, m.bottom) * scale);
            if m.enabled && margins.is_positive() && margins != paper {
                painter.rect_stroke(margins, 0.0, egui::Stroke::new(1.0, self.margin_colour), egui::StrokeKind::Inside);
            }
        }
        if self.count > 1 {
            let x = start.x + self.spine as f32 * (page.x + gap) - gap * 0.5;
            painter.vline(x, start.y..=start.y + page.y, egui::Stroke::new(1.0, ui.visuals().weak_text_color()));
        }
    }
}

fn column_label(ui: &mut egui::Ui, key: &str) -> egui::Response {
    ui.add_sized([ui.available_width().min(96.0), 18.0], egui::Label::new(text(key)).truncate())
        .on_hover_text(text(key))
}

fn section_divider(ui: &mut egui::Ui) {
    ui.add_space(dtool_design::space("xs"));
    options_divider(ui);
    ui.add_space(dtool_design::space("xs"));
}

fn section_switch(ui: &mut egui::Ui, value: &mut bool, key: &str) {
    ui.horizontal(|ui| {
        ui.heading(text(key));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            dtool_design::toggle(ui, value, "").on_hover_text(text(key));
        });
    });
}

fn paper_size(doc: &Document, key: &str) -> [f32; 2] {
    let (w, h, unit, _) = crate::dialogs::new_document_preset(key).expect("master paper preset");
    let factor = dtool_core::doc_setup::unit_factor(unit, doc.dpi, doc.units, doc.dpi);
    [w * factor, h * factor]
}

fn number(ui: &mut egui::Ui, value: &mut f32, key: &str, suffix: &str, (range, decimals): (std::ops::RangeInclusive<f32>, usize)) -> bool {
    let mut raw = f64::from(*value);
    let spec = NumericSpec::new(text(key)).range(f64::from(*range.start())..=f64::from(*range.end()))
        .suffix(suffix).decimals(decimals).width(76.0);
    let changed = dtool_design::numeric_value(ui, ui.make_persistent_id(key), &mut raw, &spec).changed();
    *value = raw as f32;
    changed
}
