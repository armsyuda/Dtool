use std::collections::BTreeSet;
use std::path::PathBuf;

use dtool_design::{button, toggle};
use dtool_strings::{has, text};
use egui::Ui;

use crate::shortcuts::{Modifiers, ShortcutMap};

pub const PAGE_SHOT: &str = "doc_1181_dlg_Settings_PrefKeyboardShortcutsPage";
pub const PAGE_XAML: &str = "prefkeyboardshortcutspage.xaml";
pub const PAGE_CAPTURE: &str = "B13_settings_shortcuts";
pub const LOAD_DIALOG_SHOT: &str = "B13_shortcuts_load_import_dialog";
pub const SAVE_DIALOG_SHOT: &str = "B13_shortcuts_save_export_dialog";
pub const CMD_SET: &str = "ui.shortcut.set";
pub const CMD_RESET: &str = "ui.shortcuts.reset";
pub const CMD_IMPORT: &str = "ui.shortcuts.import";
pub const CMD_EXPORT: &str = "ui.shortcuts.export";

fn default_file_path() -> String {
    let base = match std::env::var_os("USERPROFILE") {
        Some(home) => PathBuf::from(home).join("Documents"),
        None => PathBuf::from("."),
    };
    base.join("dtool_shortcuts.json").display().to_string()
}

pub fn affinity_key_name(key: egui::Key) -> String {
    let name = match key {
        egui::Key::Num0 => "D0",
        egui::Key::Num1 => "D1",
        egui::Key::Num2 => "D2",
        egui::Key::Num3 => "D3",
        egui::Key::Num4 => "D4",
        egui::Key::Num5 => "D5",
        egui::Key::Num6 => "D6",
        egui::Key::Num7 => "D7",
        egui::Key::Num8 => "D8",
        egui::Key::Num9 => "D9",
        egui::Key::Backspace => "Back",
        egui::Key::PageDown => "Next",
        egui::Key::PageUp => "PageUp",
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        egui::Key::Comma => "OemComma",
        egui::Key::Minus => "OemMinus",
        egui::Key::Plus | egui::Key::Equals => "OemPlus",
        egui::Key::Period => "OemPeriod",
        egui::Key::Semicolon => "Oem1",
        egui::Key::Slash => "Oem2",
        egui::Key::Backtick => "Oem3",
        egui::Key::OpenBracket => "Oem4",
        egui::Key::Backslash => "Oem5",
        egui::Key::CloseBracket => "Oem6",
        egui::Key::Quote => "OemQuotes",
        egui::Key::Enter => "Return",
        other => other.name(),
    };
    name.to_string()
}

fn key_label(key: &str) -> String {
    let shown = match key {
        "D0" => "0",
        "D1" => "1",
        "D2" => "2",
        "D3" => "3",
        "D4" => "4",
        "D5" => "5",
        "D6" => "6",
        "D7" => "7",
        "D8" => "8",
        "D9" => "9",
        "OemComma" => ",",
        "OemMinus" => "-",
        "OemPlus" => "+",
        "OemPeriod" => ".",
        "Oem1" => ";",
        "Oem2" => "/",
        "Oem3" => "`",
        "Oem4" => "[",
        "Oem5" => "\\",
        "Oem6" => "]",
        "OemQuotes" => "'",
        "Next" => "PageDown",
        "Back" => "Backspace",
        other => other,
    };
    shown.to_string()
}

pub fn display_shortcut(shortcut: &crate::shortcuts::Shortcut) -> String {
    let mut parts: Vec<String> = Vec::new();
    if shortcut.modifiers.ctrl {
        parts.push(text("shortcut.mod.ctrl").to_string());
    }
    if shortcut.modifiers.alt {
        parts.push(text("shortcut.mod.alt").to_string());
    }
    if shortcut.modifiers.shift {
        parts.push(text("shortcut.mod.shift").to_string());
    }
    parts.push(key_label(&shortcut.key));
    parts.join("+")
}

pub fn command_label(command: &str) -> String {
    let short = command.rsplit('.').next().unwrap_or(command);
    let short = short.strip_suffix("Command").unwrap_or(short);
    let key = format!("shortcut.cmd.{short}");
    if has(&key) {
        text(&key).to_string()
    } else {
        short.to_string()
    }
}

pub struct ShortcutsPage {
    pub map: ShortcutMap,
    pub apply_to_all: bool,
    pub ignore_modifier: bool,
    pub file_path: String,
    defaults: ShortcutMap,
    loaded: bool,
    filter: String,
    capturing: Option<String>,
    status: Option<&'static str>,
}

impl Default for ShortcutsPage {
    fn default() -> Self {
        Self {
            map: ShortcutMap::default(),
            apply_to_all: true,
            ignore_modifier: false,
            file_path: default_file_path(),
            defaults: ShortcutMap::default(),
            loaded: false,
            filter: String::new(),
            capturing: None,
            status: None,
        }
    }
}

impl ShortcutsPage {
    pub fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.map = ShortcutMap::load();
            self.defaults = ShortcutMap::affinity_default();
            self.loaded = true;
        }
    }

    pub fn run(&mut self, name: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        self.ensure_loaded();
        self.map.run(name, params)
    }

    fn store(&mut self) {
        self.status = match self.map.save_checked() {
            Ok(()) => None,
            Err(_) => Some("pref.shortcuts.save_failed"),
        };
    }

    fn capture(&mut self, ui: &mut Ui) {
        let command = match self.capturing.clone() {
            Some(c) => c,
            None => return,
        };
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, mods)) = pressed {
            if key == egui::Key::Escape {
                self.capturing = None;
                return;
            }
            let modifiers = if self.ignore_modifier {
                Modifiers::default()
            } else {
                Modifiers {
                    ctrl: mods.ctrl || mods.command,
                    shift: mods.shift,
                    alt: mods.alt,
                }
            };
            self.map.set(&command, &affinity_key_name(key), modifiers);
            self.capturing = None;
            self.store();
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        self.ensure_loaded();
        self.capture(ui);

        ui.horizontal(|ui| {
            toggle(ui, &mut self.apply_to_all, text("pref.shortcuts.apply_to_all"));
            toggle(ui, &mut self.ignore_modifier, text("pref.shortcuts.ignore_modifier"));
        });
        ui.horizontal(|ui| {
            ui.label(text("pref.shortcuts.file"));
            ui.text_edit_singleline(&mut self.file_path);
        });
        ui.horizontal(|ui| {
            if button(ui, text("pref.shortcuts.import")).clicked() {
                match ShortcutMap::import_file(&self.file_path) {
                    Ok(map) => {
                        self.map = map;
                        self.store();
                        if self.status.is_none() {
                            self.status = Some("pref.shortcuts.imported");
                        }
                    }
                    Err(_) => self.status = Some("pref.shortcuts.import_failed"),
                }
            }
            if button(ui, text("pref.shortcuts.export")).clicked() {
                self.status = match self.map.export_file(&self.file_path) {
                    Ok(()) => Some("pref.shortcuts.exported"),
                    Err(_) => Some("pref.shortcuts.export_failed"),
                };
            }
            if button(ui, text("pref.shortcuts.reset")).clicked() {
                self.map.reset();
                self.capturing = None;
                self.store();
            }
            if button(ui, text("pref.shortcuts.clear_all")).clicked() {
                self.map.clear_all();
                self.capturing = None;
                self.store();
            }
        });
        if let Some(key) = self.status {
            ui.small(text(key));
        }
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(text("pref.shortcuts.filter")));

        let mut commands: BTreeSet<String> = self.defaults.commands();
        commands.extend(self.map.commands());
        let filter = self.filter.trim().to_lowercase();
        let mut clear: Option<String> = None;
        egui::Grid::new("pref.shortcuts.grid").striped(true).show(ui, |ui| {
            for command in commands.iter() {
                let label = command_label(command);
                if !filter.is_empty()
                    && !label.to_lowercase().contains(&filter)
                    && !command.to_lowercase().contains(&filter)
                {
                    continue;
                }
                ui.label(label.as_str());
                let shown = if self.capturing.as_deref() == Some(command.as_str()) {
                    text("pref.shortcuts.press_key").to_string()
                } else {
                    self.map
                        .bindings_for(command)
                        .iter()
                        .map(display_shortcut)
                        .collect::<Vec<String>>()
                        .join(", ")
                };
                if button(ui, shown).clicked() {
                    self.capturing = Some(command.clone());
                }
                if button(ui, text("pref.shortcuts.clear")).clicked() {
                    clear = Some(command.clone());
                }
                ui.end_row();
            }
        });
        if let Some(command) = clear {
            self.map.clear_command(&command);
            self.store();
        }
    }
}
