use dtool_core::data_merge::{load_source, PageRepeat, RowWarning};
use dtool_core::Session;
use dtool_design::{button, dialog_frame, dropdown, number_field, toggle};
use dtool_strings::text;

pub struct DataMergeManagerDialog {
    pub open: bool,
    pub warnings: Vec<RowWarning>,
    pub error: Option<String>,
}

impl Default for DataMergeManagerDialog {
    fn default() -> Self {
        Self { open: false, warnings: Vec::new(), error: None }
    }
}

impl DataMergeManagerDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new(text("dialog.data_merge_manager.title"))
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                dialog_frame(ui, |ui| {
                    self.body(ui, session);
                });
            });
        self.open = open;
    }

    fn body(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let Some(doc) = session.document_mut() else {
            ui.label(text("app.no_document"));
            return;
        };
        let mut refresh_at: Option<usize> = None;
        for (index, src) in doc.merge_sources.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(&src.name);
                    toggle(ui, &mut src.enabled, text("merge.enabled"));
                    if button(ui, "Refresh").clicked() {
                        refresh_at = Some(index);
                    }
                });
                page_range_editor(ui, index, &mut src.pages);
            });
        }
        if let Some(index) = refresh_at {
            match load_source(&mut doc.merge_sources[index]) {
                Ok(warnings) => {
                    self.warnings = warnings;
                    self.error = None;
                }
                Err(err) => {
                    self.error = Some(err.message_key);
                }
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        if !self.warnings.is_empty() {
            ui.separator();
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for warning in &self.warnings {
                    ui.label(format!("{}: {}", warning.row_index, warning.message));
                }
            });
        }
    }
}

fn page_range_editor(ui: &mut egui::Ui, index: usize, pages: &mut PageRepeat) {
    let mut kind = match pages {
        PageRepeat::All => 0,
        PageRepeat::Range(_, _) => 1,
        PageRepeat::Repeat(_, _) => 2,
    };
    let mut start = match pages {
        PageRepeat::All => 1.0,
        PageRepeat::Range(start, _) | PageRepeat::Repeat(start, _) => *start as f64,
    };
    let mut end = match pages {
        PageRepeat::All => 1.0,
        PageRepeat::Range(_, end) | PageRepeat::Repeat(_, end) => *end as f64,
    };
    ui.horizontal(|ui| {
        ui.label("Pages");
        let id = format!("merge-pages-kind-{index}");
        if dropdown(ui, &id, &mut kind, &["All", "Range", "Repeat"]).changed() {
            *pages = match kind {
                1 => PageRepeat::Range(start.max(1.0).round() as usize, end.max(1.0).round() as usize),
                2 => PageRepeat::Repeat(start.max(1.0).round() as usize, end.max(1.0).round() as usize),
                _ => PageRepeat::All,
            };
        }
        if kind != 0 {
            if number_field(ui, &mut start, "Start").changed() && start.is_finite() {
                let start = start.max(1.0).round() as usize;
                let end = end.max(start as f64).round() as usize;
                *pages = if kind == 2 { PageRepeat::Repeat(start, end) } else { PageRepeat::Range(start, end) };
            }
            if number_field(ui, &mut end, "End").changed() && end.is_finite() {
                let start = start.max(1.0).round() as usize;
                let end = end.max(start as f64).round() as usize;
                *pages = if kind == 2 { PageRepeat::Repeat(start, end) } else { PageRepeat::Range(start, end) };
            }
        }
    });
}
