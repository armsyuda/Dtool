use dtool_core::Session;
use dtool_strings::text;

use crate::batch_dialog::BatchDialog;
use crate::macro_library_panel::MacroLibraryPanel;
use crate::macro_panel::MacroPanel;

pub const BATCH_JOB_MENU_ID: &str = "file.new_image_process.batch_job";
pub const BATCH_WINDOW_MENU_ID: &str = "window.pixel.batch";
pub const MACRO_WINDOW_MENU_ID: &str = "window.pixel.macro";

pub fn handles(id: &str) -> bool {
    matches!(id, BATCH_JOB_MENU_ID | BATCH_WINDOW_MENU_ID | MACRO_WINDOW_MENU_ID)
}

pub fn runs_without_document(id: &str) -> bool {
    matches!(id, BATCH_JOB_MENU_ID | BATCH_WINDOW_MENU_ID)
}

#[derive(Default)]
pub struct BatchHost {
    pub batch: BatchDialog,
    pub macro_panel: MacroPanel,
    pub macro_library: MacroLibraryPanel,
    batch_library_loaded: bool,
    macro_library_loaded: bool,
}

impl BatchHost {
    pub fn handle_menu(&mut self, id: &str) -> bool {
        match id {
            BATCH_JOB_MENU_ID | BATCH_WINDOW_MENU_ID => {
                self.open_batch();
                true
            }
            MACRO_WINDOW_MENU_ID => {
                self.toggle_macro();
                true
            }
            _ => false,
        }
    }

    pub fn open_batch(&mut self) {
        if !self.batch_library_loaded {
            self.batch.load_library();
            self.batch_library_loaded = true;
        }
        self.batch.open = true;
    }

    pub fn toggle_macro(&mut self) {
        let open = !self.macro_panel.open;
        if open && !self.macro_library_loaded {
            self.macro_library.load();
            self.macro_library_loaded = true;
        }
        self.macro_panel.open = open;
        self.macro_library.open = open;
    }

    pub fn batch_open(&self) -> bool {
        self.batch.open
    }

    pub fn macro_open(&self) -> bool {
        self.macro_panel.open
    }

    pub fn close_batch(&mut self) -> bool {
        if self.batch.can_close() {
            self.batch.open = false;
        }
        !self.batch.open
    }

    pub fn show(&mut self, ctx: &egui::Context, session: Option<&mut Session>) {
        if !self.batch.can_close() {
            self.batch.open = true;
        }
        if self.batch.open {
            self.batch.show(ctx);
        }
        if !self.macro_panel.open {
            return;
        }
        let mut open = true;
        egui::Window::new(text("macro.panel.title")).open(&mut open).show(ctx, |ui| match session {
            Some(session) => {
                self.macro_panel.show(ui, session);
                self.macro_library.show(ui, session);
            }
            None => {
                ui.label(text("error.no_document"));
            }
        });
        if !open {
            self.macro_panel.open = false;
            self.macro_library.open = false;
        }
    }
}
