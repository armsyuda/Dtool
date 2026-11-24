use dtool_design::{size, space};
use dtool_strings::{format, text};
use egui::Ui;

use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};

pub struct AboutDialog {
    pub open: bool,
}

impl Default for AboutDialog {
    fn default() -> Self {
        Self { open: false }
    }
}

pub const ABOUT_MENU_ID: &str = "help.about";

pub const LICENCES: [&str; 11] = ["dtool-core", "dtool-ui", "dtool-strings", "egui", "serde", "anyhow", "hunspell-dict-ko 0.7.94 (GPL-3.0)", "Kiwi 0.24.1 (Apache-2.0; third-party notices)", "arboard (MIT OR Apache-2.0)", "libloading (MIT OR Apache-2.0)", "SCOWL English dictionary"];

pub fn version_line() -> String {
    format("about.version", &[("version", env!("CARGO_PKG_VERSION"))])
}

pub fn body(ui: &mut Ui) -> bool {
    ui.heading(text("app.title"));
    ui.label(version_line());
    ui.add_space(space("lg"));
    ui.label(text("about.licences"));
    egui::ScrollArea::vertical().max_height(size("dialog.max_body_height")).show(ui, |ui| {
        for licence in LICENCES {
            ui.label(licence);
        }
    });
    ui.add_space(space("lg"));
    dtool_design::button(ui, text("button.close")).clicked()
}

impl AboutDialog {
    pub fn show(&mut self, ui: &mut Ui) {
        if !self.open {
            return;
        }
        let (result, close) = show_modal(ui.ctx(), "dtool.dialog.about", text("about.title"), ClosePolicy::Free, true, |ui| body(ui));
        if close || result != DialogResult::Open {
            self.open = false;
        }
    }
}
