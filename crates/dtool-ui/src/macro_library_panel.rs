use dtool_core::{CommandError, Session};
use dtool_export::macros::{self, Macro, MacroLibrary};
use dtool_strings::text;
use egui::Ui;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryAction {
    Played(String),
    Exported(PathBuf),
    Imported(serde_json::Value),
    Changed,
}

#[derive(Default)]
pub struct MacroLibraryPanel {
    pub open: bool,
    pub library: MacroLibrary,
    pub library_path: Option<PathBuf>,
    pub selected_category: Option<String>,
    pub selected_macro: Option<String>,
    pub name_input: String,
    pub path_input: String,
    pub problem: Option<CommandError>,
}

impl MacroLibraryPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_library(library: MacroLibrary, library_path: Option<PathBuf>) -> Self {
        Self { library, library_path, ..Self::default() }
    }

    pub fn load(&mut self) {
        let path = self.store_path();
        self.library = if path.exists() { MacroLibrary::load_from_file(&path).unwrap_or_default() } else { MacroLibrary::new() };
        self.selected_category = None;
        self.selected_macro = None;
    }

    pub fn store_path(&self) -> PathBuf {
        self.library_path.clone().unwrap_or_else(MacroLibrary::default_path)
    }

    pub fn save(&self) -> Result<(), CommandError> {
        let path = self.store_path();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO_ERROR"))?;
        }
        self.library.save_to_file(&path)
    }

    fn commit(&mut self, result: Result<(), CommandError>) -> Option<LibraryAction> {
        match result.and_then(|_| self.save()) {
            Ok(()) => {
                self.problem = None;
                Some(LibraryAction::Changed)
            }
            Err(error) => {
                self.problem = Some(error);
                None
            }
        }
    }

    pub fn add_category(&mut self, name: &str) -> Option<LibraryAction> {
        let result = self.library.add_category(name);
        if result.is_ok() {
            self.selected_category = Some(name.trim().to_string());
            self.selected_macro = None;
        }
        self.commit(result)
    }

    pub fn rename_selected(&mut self, to: &str) -> Option<LibraryAction> {
        let category = self.selected_category.clone()?;
        let result = match self.selected_macro.clone() {
            Some(name) => self.library.rename_macro(&category, &name, to).map(|_| {
                self.selected_macro = Some(to.trim().to_string());
            }),
            None => self.library.rename_category(&category, to).map(|_| {
                self.selected_category = Some(to.trim().to_string());
            }),
        };
        self.commit(result)
    }

    pub fn delete_selected(&mut self) -> Option<LibraryAction> {
        let category = self.selected_category.clone()?;
        let result = match self.selected_macro.take() {
            Some(name) => self.library.delete_macro(&category, &name).map(|_| ()),
            None => {
                self.selected_category = None;
                self.library.delete_category(&category).map(|_| ())
            }
        };
        self.commit(result)
    }

    pub fn selected(&self) -> Option<&Macro> {
        let category = self.selected_category.as_deref()?;
        let name = self.selected_macro.as_deref()?;
        self.library.get_in(category, name)
    }

    pub fn play_selected(&mut self, session: &mut Session) -> Option<LibraryAction> {
        let entry = self.selected()?.clone();
        match macros::play_steps(session, entry.enabled_steps()) {
            Ok(_) => {
                self.problem = None;
                Some(LibraryAction::Played(entry.name))
            }
            Err(error) => {
                self.problem = Some(error);
                None
            }
        }
    }

    pub fn export_selected(&mut self, path: &str) -> Option<LibraryAction> {
        let category = self.selected_category.clone()?;
        let path = PathBuf::from(path.trim());
        let result = match self.selected() {
            Some(entry) => macros::export_macro(entry, &path),
            None => macros::export_category(&self.library, &category, &path),
        };
        match result {
            Ok(()) => {
                self.problem = None;
                Some(LibraryAction::Exported(path))
            }
            Err(error) => {
                self.problem = Some(error);
                None
            }
        }
    }

    pub fn import_path(&mut self, path: &str) -> Option<LibraryAction> {
        let path = PathBuf::from(path.trim());
        let result = macros::import_into(&mut self.library, &path);
        match result {
            Ok(value) => match self.save() {
                Ok(()) => {
                    self.problem = None;
                    self.selected_category = value.get("category").and_then(|v| v.as_str()).map(str::to_string);
                    self.selected_macro = value.get("name").and_then(|v| v.as_str()).map(str::to_string);
                    Some(LibraryAction::Imported(value))
                }
                Err(error) => {
                    self.problem = Some(error);
                    None
                }
            },
            Err(error) => {
                self.problem = Some(error);
                None
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, session: &mut Session) -> Option<LibraryAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        dtool_design::panel_frame(ui, |ui| {
            dtool_design::panel_header(ui, text("macro.library"));

            let categories = self.library.category_names();
            for category in &categories {
                let category_selected = self.selected_category.as_deref() == Some(category.as_str());
                if dtool_design::tab(ui, category_selected && self.selected_macro.is_none(), category.as_str()).clicked() {
                    self.selected_category = Some(category.clone());
                    self.selected_macro = None;
                }
                if category_selected {
                    let names: Vec<String> = self.library.category(category).map(|list| list.iter().map(|m| m.name.clone()).collect()).unwrap_or_default();
                    ui.indent(("macro-library", category.as_str()), |ui| {
                        for name in names {
                            let macro_selected = self.selected_macro.as_deref() == Some(name.as_str());
                            if dtool_design::tab(ui, macro_selected, name.as_str()).clicked() {
                                self.selected_macro = Some(name.clone());
                            }
                        }
                    });
                }
            }

            ui.separator();
            ui.text_edit_singleline(&mut self.name_input);
            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("assets.add_category")).clicked() {
                    let name = self.name_input.clone();
                    action = self.add_category(&name).or(action.take());
                }
                let has_selection = self.selected_category.is_some();
                if ui.add_enabled_ui(has_selection, |ui| dtool_design::button(ui, text("button.rename"))).inner.clicked() {
                    let name = self.name_input.clone();
                    action = self.rename_selected(&name).or(action.take());
                }
                if ui.add_enabled_ui(has_selection, |ui| dtool_design::button(ui, text("button.delete"))).inner.clicked() {
                    action = self.delete_selected().or(action.take());
                }
            });

            ui.horizontal(|ui| {
                let can_play = self.selected().is_some();
                if ui.add_enabled_ui(can_play, |ui| dtool_design::button(ui, text("macro.play"))).inner.clicked() {
                    action = self.play_selected(session).or(action.take());
                }
            });

            ui.text_edit_singleline(&mut self.path_input);
            ui.horizontal(|ui| {
                let can_export = self.selected_category.is_some();
                if ui.add_enabled_ui(can_export, |ui| dtool_design::button(ui, text("macro.export"))).inner.clicked() {
                    let path = self.path_input.clone();
                    action = self.export_selected(&path).or(action.take());
                }
                if dtool_design::button(ui, text("macro.import")).clicked() {
                    let path = self.path_input.clone();
                    action = self.import_path(&path).or(action.take());
                }
            });

            if let Some(problem) = &self.problem {
                ui.label(text(&problem.message_key));
            }
        });
        action
    }
}
