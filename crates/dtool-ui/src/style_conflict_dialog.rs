use dtool_design::{button, dialog_frame, number_field, panel_header};
use dtool_io::StyleConflict;

pub struct StyleConflictDialog {
    pub conflicts: Vec<StyleConflict>,
    pub open: bool,
}

impl StyleConflictDialog {
    pub fn new(conflicts: Vec<StyleConflict>) -> Self {
        Self {
            conflicts,
            open: true,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Vec<StyleConflict>> {
        if !self.open {
            return None;
        }

        let mut result = None;
        let mut should_close = false;

        dialog_frame(ctx, "import.styles.title", |ui| {
            panel_header(ui, "import.styles.title");

            for conflict in &mut self.conflicts {
                if !conflict.conflicts {
                    continue;
                }

                ui.horizontal(|ui| {
                    ui.label(&conflict.incoming);
                    number_field(ui, &mut conflict.rename_to);
                });
            }

            ui.horizontal(|ui| {
                button(ui, "import.styles.title", |ui| {
                    should_close = true;
                    result = Some(self.conflicts.clone());
                });
            });
        });

        if should_close {
            self.open = false;
        }

        result
    }
}
