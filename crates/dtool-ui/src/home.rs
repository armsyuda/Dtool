use dtool_core::prefs::{HomeItem, Prefs};
use dtool_design::{button, panel_header, toggle};
use dtool_strings::text;
use egui::Ui;
use std::path::{Path, PathBuf};

pub(crate) const HOME_MENU_ID: &str = "file.home";
pub const HOME_SCREEN_PAGE_SOURCE: &str = "dlg_Settings_PrefHomeScreenPage.txt";

pub const HOME_SECTION_ORDER: [&str; 4] = ["recent", "templates", "favourites", "presets"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupScreen {
    Home,
    EmptyWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAction {
    Open { section: String, item: String },
    Browse,
    BrowseTemplate,
    AddTemplateFolder(PathBuf),
    RemoveTemplateFolder(PathBuf),
    NewDocument,
    Close,
}

pub fn startup_screen(prefs: &Prefs) -> StartupScreen {
    if prefs.show_home_on_startup {
        StartupScreen::Home
    } else {
        StartupScreen::EmptyWorkspace
    }
}

pub fn home_section_key(id: &str) -> &'static str {
    match id {
        "recent" => "pref.home.recent",
        "templates" => "pref.home.templates",
        "favourites" => "pref.home.favourites",
        "presets" => "pref.home.presets",
        _ => "pref.home.unknown",
    }
}

pub fn ensure_sections(prefs: &mut Prefs) {
    for id in HOME_SECTION_ORDER {
        if !prefs.home_items.iter().any(|item| item.id == id) {
            prefs.home_items.push(HomeItem {
                id: id.to_string(),
                visible: true,
            });
        }
    }
}

pub fn visible_sections(prefs: &Prefs) -> Vec<HomeItem> {
    prefs
        .home_items
        .iter()
        .filter(|item| item.visible && (item.id != "recent" || prefs.home_show_recent))
        .cloned()
        .collect()
}

pub fn set_section_visible(prefs: &mut Prefs, id: &str, visible: bool) -> bool {
    match prefs.home_items.iter_mut().find(|item| item.id == id) {
        Some(item) => {
            item.visible = visible;
            true
        }
        None => false,
    }
}

pub fn move_section(prefs: &mut Prefs, from: usize, to: usize) -> bool {
    let len = prefs.home_items.len();
    if from == to || from >= len || to >= len {
        return false;
    }
    let item = prefs.home_items.remove(from);
    prefs.home_items.insert(to, item);
    true
}

#[derive(Debug, Clone, Default)]
pub struct HomeScreen {
    pub open: bool,
    pub recent: Vec<String>,
    pub templates: Vec<String>,
    pub favourites: Vec<String>,
    pub presets: Vec<String>,
    template_folder_input: String,
    pub(crate) error: Option<String>,
}

impl HomeScreen {
    pub fn on_startup(prefs: &Prefs) -> HomeScreen {
        let mut screen = HomeScreen::default();
        if startup_screen(prefs) == StartupScreen::Home {
            screen.reopen(prefs);
        }
        screen
    }

    pub(crate) fn reopen(&mut self, prefs: &Prefs) {
        self.open = true;
        self.error = None;
        self.presets = crate::dialogs::NewDocumentDialogState::saved_preset_names();
        self.favourites = crate::dialogs::NewDocumentDialogState::saved_favourite_names();
        self.refresh_templates(&prefs.home_template_folders);
    }

    pub(crate) fn refresh_templates(&mut self, folders: &[PathBuf]) {
        let mut templates = folders.iter().filter_map(|folder| std::fs::read_dir(folder).ok())
            .flatten().filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|extension|
                extension.eq_ignore_ascii_case(dtool_core::commands::document_files::TEMPLATE_EXTENSION)))
            .filter_map(|path| path.into_os_string().into_string().ok())
            .collect::<Vec<_>>();
        templates.sort_unstable();
        templates.dedup();
        self.templates = templates;
    }

    fn template_controls(&mut self, ui: &mut Ui, prefs: &Prefs) -> Option<HomeAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            if button(ui, text("dialog.template.open")).clicked() {
                action = Some(HomeAction::BrowseTemplate);
            }
            ui.add(egui::TextEdit::singleline(&mut self.template_folder_input)
                .hint_text(text("home.template.folder_path")).desired_width(290.0));
            let path = self.template_folder_input.trim();
            if ui.add_enabled_ui(!path.is_empty(), |ui| button(ui, text("home.template.add_folder"))).inner.clicked() {
                action = Some(HomeAction::AddTemplateFolder(PathBuf::from(path)));
            }
        });
        for folder in &prefs.home_template_folders {
            ui.horizontal(|ui| {
                if button(ui, text("home.template.remove_folder")).clicked() {
                    action = Some(HomeAction::RemoveTemplateFolder(folder.clone()));
                }
                let label = folder.to_string_lossy();
                ui.add(egui::Label::new(label.as_ref()).truncate()).on_hover_text(label);
            });
        }
        action
    }

    pub(crate) fn show_template_links(&mut self, ui: &mut Ui, prefs: &Prefs) -> Option<HomeAction> {
        ui.menu_button(text("pref.home.templates"), |ui| {
            let action = self.template_controls(ui, prefs).or_else(|| {
                egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                    self.templates.iter().find_map(|item| {
                        let name = Path::new(item).file_name().and_then(|name| name.to_str()).unwrap_or(item);
                        button(ui, name).on_hover_text(item).clicked()
                            .then(|| HomeAction::Open { section: "templates".into(), item: item.clone() })
                    })
                }).inner
            });
            if action.is_some() { ui.close(); }
            action
        }).inner.flatten()
    }

    pub(crate) fn clear_template_folder_input(&mut self) {
        self.template_folder_input.clear();
    }

    pub fn entries(&self, id: &str) -> &[String] {
        match id {
            "recent" => self.recent.as_slice(),
            "templates" => self.templates.as_slice(),
            "favourites" => self.favourites.as_slice(),
            "presets" => self.presets.as_slice(),
            _ => &[],
        }
    }

    pub fn section_order(&self, prefs: &Prefs) -> Vec<String> {
        visible_sections(prefs)
            .into_iter()
            .map(|item| item.id)
            .collect()
    }

    pub fn show(&mut self, ui: &mut Ui, prefs: &mut Prefs) -> Option<HomeAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        let sections = self.section_order(prefs);
        let (close, ()) = crate::dialog_chrome::show_modal(
            ui.ctx(),
            "dtool.home",
            text("pref.tab.home"),
            crate::dialog_chrome::ClosePolicy::CancelOnly,
            false,
            |ui| {
                ui.set_width(ui.ctx().content_rect().width().min(700.0));
                ui.horizontal(|ui| {
                    if dtool_design::primary_button(ui, text("dialog.new_document.title"), egui::vec2(112.0, 30.0)).clicked() {
                        action = Some(HomeAction::NewDocument);
                    }
                    if ui
                        .add(
                            egui::Button::new(text("menu.file.open"))
                                .min_size(egui::vec2(112.0, 30.0)),
                        )
                        .clicked()
                    {
                        action = Some(HomeAction::Browse);
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(250.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for id in &sections {
                            if id == "templates" {
                                panel_header(ui, text(home_section_key(id)));
                                if let Some(selected) = self.template_controls(ui, prefs) {
                                    action = Some(selected);
                                }
                            }
                            let entries = self.entries(id);
                            if entries.is_empty() && id != "recent" {
                                continue;
                            }
                            if id != "templates" {
                                panel_header(ui, text(home_section_key(id)));
                            }
                            if entries.is_empty() {
                                ui.weak(text("pref.home.recent_empty"));
                            }
                            for entry in entries {
                                let label = if matches!(id.as_str(), "presets" | "favourites") {
                                    entry.as_str()
                                } else {
                                    Path::new(entry)
                                        .file_name()
                                        .and_then(|value| value.to_str())
                                        .unwrap_or(entry)
                                };
                                if button(ui, label).on_hover_text(entry).clicked() {
                                    action = Some(HomeAction::Open {
                                        section: id.clone(),
                                        item: entry.clone(),
                                    });
                                }
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    toggle(ui, &mut prefs.show_home_on_startup, text("pref.show_home"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, text("button.close")).clicked() {
                            action = Some(HomeAction::Close);
                        }
                    });
                });
            },
        );
        if close == crate::dialog_chrome::DialogResult::Cancel {
            action = Some(HomeAction::Close);
        }
        if action.as_ref().is_some_and(|action| !matches!(action, HomeAction::AddTemplateFolder(_) | HomeAction::RemoveTemplateFolder(_))) {
            self.open = false;
        }
        action
    }
}
