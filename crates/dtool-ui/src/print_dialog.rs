use std::path::PathBuf;
use egui::Ui;
use dtool_strings::text;
use dtool_design::{space, icon_button, button, dropdown, number_field, toggle, panel_frame, panel_header, tab, toolbar};
use crate::print::{PrintSettings, PrintRange, FitType, PrintBackend, FilePrintBackend};

const SECTION_KEYS: [&str; 5] = [
    "print.layout",
    "print.rasterization",
    "print.bleed_marks",
    "print.colour_management",
    "print.to_file",
];

pub struct PrintDialog {
    pub open: bool,
    pub settings: PrintSettings,
    pub current_page: u32,
    pub total_pages: u32,
    section: usize,
}

impl Default for PrintDialog {
    fn default() -> Self {
        Self {
            open: false,
            settings: PrintSettings::default(),
            current_page: 1,
            total_pages: 1,
            section: 0,
        }
    }
}

fn choose_string(ui: &mut Ui, id: &str, value: &mut String, options: &[&str]) {
    let mut index = options.iter().position(|o| *o == value.as_str()).unwrap_or(0);
    let before = index;
    dropdown(ui, id, &mut index, options);
    if index != before {
        if let Some(choice) = options.get(index) {
            *value = choice.to_string();
        }
    }
}

fn edit_u32(ui: &mut Ui, value: &mut u32, min: u32) {
    let mut number = *value as f64;
    if number_field(ui, &mut number, "").changed() {
        *value = number.round().max(min as f64) as u32;
    }
}

fn edit_f32(ui: &mut Ui, value: &mut f32) {
    let mut number = *value as f64;
    if number_field(ui, &mut number, "").changed() {
        *value = number.max(0.0) as f32;
    }
}

impl PrintDialog {
    pub fn show(&mut self, ui: &mut Ui) {
        if !self.open {
            return;
        }

        panel_frame(ui, |ui| {
            panel_header(ui, text("print.title"));

            ui.horizontal(|ui| {
                if icon_button(ui, text("menu.document.pages.first")).clicked() {
                    self.current_page = 1;
                }
                if icon_button(ui, text("menu.document.pages.previous")).clicked() && self.current_page > 1 {
                    self.current_page -= 1;
                }
                if icon_button(ui, text("menu.document.pages.next")).clicked() && self.current_page < self.total_pages {
                    self.current_page += 1;
                }
                if icon_button(ui, text("menu.document.pages.last")).clicked() {
                    self.current_page = self.total_pages;
                }
                
                ui.label(format!("{}/{}", self.current_page, self.total_pages));
            });

            ui.add_space(space("md"));

            ui.horizontal(|ui| {
                for (index, key) in SECTION_KEYS.iter().enumerate() {
                    if tab(ui, self.section == index, text(key)).clicked() {
                        self.section = index;
                    }
                }
            });

            ui.add_space(space("sm"));

            match self.section {
                0 => self.show_layout_section(ui),
                1 => self.show_rasterization_section(ui),
                2 => self.show_bleed_marks_section(ui),
                3 => self.show_colour_management_section(ui),
                _ => self.show_print_to_file_section(ui),
            }

            ui.add_space(space("md"));

            toolbar(ui, |ui| {
                if button(ui, text("print.cancel")).clicked() {
                    self.open = false;
                }
                if button(ui, text("print.print")).clicked() {
                    let _ = crate::print::run_print(&self.settings);
                    self.open = false;
                }
            });
        });
    }

    fn show_layout_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(text("print.profile"));
            choose_string(ui, "print.profile", &mut self.settings.profile, &["Custom", "Default"]);
        });

        ui.horizontal(|ui| {
            ui.label(text("print.printer"));
            let printers = FilePrintBackend.printers();
            let options: Vec<&str> = printers.iter().map(|p| p.as_str()).collect();
            let mut index = self
                .settings
                .printer
                .as_deref()
                .and_then(|name| options.iter().position(|o| *o == name))
                .unwrap_or(0);
            let before = index;
            dropdown(ui, "print.printer", &mut index, &options);
            if index != before || self.settings.printer.is_none() {
                self.settings.printer = options.get(index).map(|o| o.to_string());
            }
        });

        ui.horizontal(|ui| {
            ui.label(text("print.copies"));
            edit_u32(ui, &mut self.settings.copies, 1);
            
            toggle(ui, &mut self.settings.collate, text("print.collate"));
        });

        ui.horizontal(|ui| {
            ui.label(text("print.range"));
            let mut range_index = match &self.settings.range {
                PrintRange::All => 0,
                PrintRange::Current => 1,
                PrintRange::Pages(_) => 2,
                PrintRange::Artboards(_) => 3,
                PrintRange::Selection => 4,
            };
            let before = range_index;
            dropdown(ui, "print.range", &mut range_index, &["Document", "Current", "Pages", "Artboards", "Selection"]);
            if range_index != before {
                self.settings.range = match range_index {
                    0 => PrintRange::All,
                    1 => PrintRange::Current,
                    2 => PrintRange::Pages((1..=self.total_pages.max(1)).collect()),
                    3 => PrintRange::Artboards(Vec::new()),
                    _ => PrintRange::Selection,
                };
            }
        });

        ui.horizontal(|ui| {
            ui.label(text("print.fit_type"));
            let mut fit_index = match &self.settings.fit {
                FitType::Scale => 0,
                FitType::ScaleToFit => 1,
                FitType::ActualSize => 2,
            };
            let before = fit_index;
            dropdown(ui, "print.fit_type", &mut fit_index, &["Scale", "Fit to Printable", "Actual Size"]);
            if fit_index != before {
                self.settings.fit = match fit_index {
                    0 => FitType::Scale,
                    1 => FitType::ScaleToFit,
                    _ => FitType::ActualSize,
                };
            }
            
            edit_f32(ui, &mut self.settings.scale);
        });

        ui.horizontal(|ui| {
            ui.label(text("print.include"));
            choose_string(ui, "print.include", &mut self.settings.include, &["All Sheets", "Current Sheet"]);
        });

        toggle(ui, &mut self.settings.reverse, text("print.reverse"));
        
        ui.horizontal(|ui| {
            ui.label(text("print.orientation"));
            let mut orient_index = if self.settings.landscape { 2 } else { 0 };
            let before = orient_index;
            dropdown(ui, "print.orientation", &mut orient_index, &["Automatic", "Portrait", "Landscape"]);
            if orient_index != before {
                self.settings.landscape = orient_index == 2;
            }
        });
    }

    fn show_rasterization_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(text("print.rasterize_dpi"));
            edit_u32(ui, &mut self.settings.rasterize_dpi, 1);
        });

        toggle(ui, &mut self.settings.clip_transparency, text("print.clip_transparency"));
        toggle(ui, &mut self.settings.rasterize_page, text("print.rasterize_page"));
    }

    fn show_bleed_marks_section(&mut self, ui: &mut Ui) {
        toggle(ui, &mut self.settings.bleed, text("print.bleed"));
        
        toggle(ui, &mut self.settings.marks.crop_marks, text("print.crop_marks"));
        toggle(ui, &mut self.settings.marks.registration_marks, text("print.registration_marks"));
        toggle(ui, &mut self.settings.marks.color_bars, text("print.color_bars"));
        toggle(ui, &mut self.settings.marks.page_info, text("print.page_info"));
    }

    fn show_colour_management_section(&mut self, ui: &mut Ui) {
        ui.label(text("print.colour_profile"));
        choose_string(ui, "print.colour_profile", &mut self.settings.profile, &["Custom", "sRGB", "CMYK"]);
    }

    fn show_print_to_file_section(&mut self, ui: &mut Ui) {
        let mut to_file = self.settings.to_file.is_some();
        if toggle(ui, &mut to_file, text("print.to_file")).changed() {
            self.settings.to_file = if to_file { Some(PathBuf::new()) } else { None };
        }
        
        if let Some(path) = &mut self.settings.to_file {
            let mut shown = path.display().to_string();
            if ui.text_edit_singleline(&mut shown).changed() {
                *path = PathBuf::from(shown);
            }
        }
    }
}
