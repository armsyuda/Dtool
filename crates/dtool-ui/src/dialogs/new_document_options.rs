use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use dtool_core::colour::{self, ColourFormat};
use dtool_core::command::DocNewArgs;
use dtool_core::doc_setup::{unit_factor, Margins};
use dtool_core::document::Units;
use dtool_core::units::ReadoutUnit;
use dtool_design::{button, numeric_value, toggle, NumericSpec};
use dtool_strings::text;
use egui::Ui;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{builtin_preset_names, prepare_new_document, preset_card, NewDocumentDialogState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DocumentKind {
    VectorArtboard,
    LayoutPages,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Options {
    format: ColourFormat,
    profile: Option<String>,
    transparent: bool,
    margins: Margins,
    bleed: Option<Margins>,
    drawing_scale: [f32; 2],
    measurement_unit: ReadoutUnit,
    pub(super) kind: Option<DocumentKind>,
    facing_pages: bool,
    start_on_right: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: ColourFormat::RGBA_8,
            profile: None,
            transparent: false,
            margins: Margins::default(),
            bleed: None,
            drawing_scale: [1.0, 1.0],
            measurement_unit: ReadoutUnit::Document,
            kind: None,
            facing_pages: true,
            start_on_right: true,
        }
    }
}

impl Options {
    pub(super) fn write_params(&self, params: &mut Value, units: Units) {
        if let Some(kind) = self.kind {
            params["document_kind"] = json!(kind);
        }
        params["colour_format"] = json!(self.format.to_string());
        if let Some(profile) = &self.profile {
            params["colour_profile"] = json!(profile);
        }
        let to_setup_units = |mut sides: Margins| {
            if units == Units::Cm {
                // Document setup stores metric geometry in millimetres.
                sides.left *= 10.0;
                sides.right *= 10.0;
                sides.top *= 10.0;
                sides.bottom *= 10.0;
            }
            sides
        };
        let mut setup = json!({
            "transparent_background": self.transparent,
            "margins": to_setup_units(self.margins),
            "facing_pages": self.facing_pages,
            "start_on_right": self.start_on_right,
            "drawing_scale": self.drawing_scale,
            "measurement_unit": self.measurement_unit,
        });
        if let Some(bleed) = &self.bleed {
            setup["bleed"] = json!(to_setup_units(*bleed));
        }
        params["setup"] = setup;
    }

    pub(super) fn reset_for_builtin_preset(&mut self, format: ColourFormat) {
        *self = Self {
            format,
            kind: self.kind,
            facing_pages: self.facing_pages,
            start_on_right: self.start_on_right,
            ..Self::default()
        };
    }

    pub(super) fn show_kind(&mut self, ui: &mut Ui, icons: &mut crate::icon_cache::IconCache) -> bool {
        ui.horizontal(|ui| {
            let previous = self.kind;
            let choices = [
                (DocumentKind::VectorArtboard, "tool-artboard", "dialog.new_document.kind.vector_artboard"),
                (DocumentKind::LayoutPages, "persona-layout", "dialog.new_document.kind.layout_pages"),
            ];
            for (kind, icon, label) in choices {
                if icons.studio_button(ui, icon, text(label), self.kind == Some(kind)).clicked() {
                    self.kind = Some(kind);
                }
            }
            if let Some((_, _, label)) = choices.into_iter().find(|(kind, _, _)| self.kind == Some(*kind)) {
                ui.label(text(label));
            }
            self.kind != previous
        }).inner
    }

    pub(super) fn facing_preview(&self) -> bool {
        self.kind == Some(DocumentKind::LayoutPages) && self.facing_pages
    }

    pub(super) fn start_on_right(&self) -> bool {
        self.start_on_right
    }

    fn scale(&mut self, factor: f32) {
        for sides in std::iter::once(&mut self.margins).chain(self.bleed.iter_mut()) {
            sides.left *= factor;
            sides.right *= factor;
            sides.top *= factor;
            sides.bottom *= factor;
        }
    }

    pub(super) fn show_layout(&mut self, ui: &mut Ui) -> bool {
        super::options_group(ui, text("dialog.new_document.page_layout"));
        let mut changed = toggle(ui, &mut self.facing_pages, text("doc_setup.facing_pages")).changed();
        changed |= ui.add_enabled_ui(self.facing_pages, |ui| {
            arrangement(ui, text("doc_setup.start_on"), &mut self.start_on_right,
                "doc_setup.start_on.left", "doc_setup.start_on.right")
        }).inner;
        changed
    }

    pub(super) fn show(&mut self, ui: &mut Ui, units: Units, dpi: f32, reset: bool, invalid: &mut bool) -> bool {
        let mut changed = false;
        super::options_group(ui, text("doc_setup.colour"));
        ui.label(text("doc_setup.colour_format"));
        dtool_design::with_icon_selection(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for &format in ColourFormat::colour_format_list() {
                    let button = egui::Button::selectable(format == self.format, format.to_string())
                        .wrap_mode(egui::TextWrapMode::Extend);
                    if ui.add(button).clicked() && format != self.format {
                        self.format = format;
                        self.profile = None;
                        changed = true;
                    }
                }
            });
        });
        if let Ok(services) = colour::services().read() {
            let default =
                dtool_core::commands::document_colour::default_profile(self.format, &services);
            let profiles: Vec<String> = services
                .profiles
                .list(self.format.model)
                .into_iter()
                .map(str::to_owned)
                .collect();
            drop(services);
            ui.label(text("doc_setup.colour_profile"));
            let default_label = format!(
                "{} · {default}",
                text("dialog.new_document.default_profile")
            );
            let selected_label = self.profile.as_deref().unwrap_or(&default_label);
            egui::ComboBox::from_id_salt("new_document.profile")
                .width(ui.available_width().min(280.0)).truncate()
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.profile.is_none(), default_label)
                        .clicked()
                        && self.profile.is_some()
                    {
                        self.profile = None;
                        changed = true;
                    }
                    for profile in profiles {
                        let selected = self.profile.as_deref() == Some(profile.as_str());
                        if ui.selectable_label(selected, &profile).clicked() && !selected {
                            self.profile = Some(profile);
                            changed = true;
                        }
                    }
                });
        }
        changed |= toggle(
            ui,
            &mut self.transparent,
            text("menu.document.setup.transparent_background"),
        )
        .changed();
        crate::dialog_chrome::options_divider(ui);
        let margins_edit = sides_editor(ui, &mut self.margins, units, "doc_setup.margins", reset);
        changed |= margins_edit.changed;
        crate::dialog_chrome::options_divider(ui);
        let amount = dtool_core::command::new_document_bleed(units, dpi);
        let mut bleed = self.bleed.unwrap_or(Margins {
            enabled: true,
            linked: true,
            left: amount,
            right: amount,
            top: amount,
            bottom: amount,
        });
        let bleed_edit = sides_editor(ui, &mut bleed, units, "doc_setup.bleed", reset);
        if bleed_edit.changed {
            self.bleed = Some(bleed);
            changed = true;
        }
        crate::dialog_chrome::options_divider(ui);
        let (scale_changed, invalid_scale) = drawing_scale_section(
            ui, &mut self.drawing_scale, &mut self.measurement_unit,
            egui::Id::unique("new_document_scale_input"), reset,
        );
        changed |= scale_changed;
        *invalid |= margins_edit.invalid || bleed_edit.invalid || invalid_scale;
        changed
    }
}

pub(super) fn drawing_scale_section(
    ui: &mut Ui, scale: &mut [f32; 2], unit: &mut ReadoutUnit, scope: egui::Id, reset: bool,
) -> (bool, bool) {
    const PRESETS: [[f32; 2]; 12] = [
        [10.0, 1.0], [5.0, 1.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [1.0, 5.0],
        [1.0, 10.0], [1.0, 20.0], [1.0, 25.0], [1.0, 50.0], [1.0, 100.0], [1.0, 200.0],
    ];
    let fields = ["doc_setup.scale.drawing", "doc_setup.scale.actual"].map(|key| (key, scope.with(key)));
    let range = f64::from(f32::MIN_POSITIVE)..=f64::from(f32::MAX);
    let parse = |input: &str| input.parse::<f64>().ok().filter(|value| range.contains(value));
    let spec = || NumericSpec::new("").decimals(6).range(range.clone()).parser(&parse);
    if reset {
        for (_, id) in fields { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
    }
    let before = *scale;
    let [drawing, actual] = before;
    let heading = format!("{}  {drawing}:{actual}", text("ctx.measure.drawing_scale"));
    let changed = egui::CollapsingHeader::new(heading)
        .id_salt("drawing_scale_section")
        .default_open(drawing != actual || !unit.is_document())
        .show(ui, |ui| {
            let selected = PRESETS.iter().any(|preset| preset == scale);
            let mut preset_selected = false;
            dtool_design::studio_row(ui, text("doc_setup.scale.preset"), |ui, label| {
                let selected_text = if selected { format!("{drawing}:{actual}") } else { text("doc_setup.size_custom").to_owned() };
                egui::ComboBox::from_id_salt("drawing_scale_preset")
                    .width(ui.available_width())
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for preset @ [drawing, actual] in PRESETS {
                            if ui.selectable_label(*scale == preset, format!("{drawing}:{actual}")).clicked() {
                                *scale = preset;
                                preset_selected = true;
                            }
                        }
                    }).response.labelled_by(label);
            });
            if preset_selected {
                for (_, id) in fields { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
            }
            dtool_design::studio_row(ui, text("doc_setup.scale.ratio"), |ui, _| {
                ui.horizontal(|ui| {
                    let width = ((ui.available_width() - 15.0) * 0.5).clamp(30.0, 70.0);
                    let field_spec = spec().width(width);
                    for (index, ((label, id), term)) in fields.into_iter().zip(&mut *scale).enumerate() {
                        if index != 0 { ui.label(":"); }
                        let mut value = f64::from(*term);
                        if numeric_value(ui, id, &mut value, &field_spec).on_hover_text(text(label)).changed() {
                            *term = value as f32;
                        }
                    }
                });
            });
            let units_changed = dtool_design::studio_row(ui, text("doc_setup.measurement_unit"), |ui, label| {
                measurement_unit_editor(ui, unit, label)
            });
            before != *scale || units_changed
        }).body_returned.unwrap_or(false);
    let spec = spec();
    let invalid = fields.into_iter().any(|(_, id)| {
        let edit = dtool_design::numeric_state(ui.ctx(), id);
        edit.edited && edit.buffer.as_deref().is_some_and(|draft| spec.parse_text(draft).is_none())
    });
    (changed, invalid)
}

fn measurement_unit_editor(ui: &mut Ui, unit: &mut ReadoutUnit, label: egui::Id) -> bool {
    use ReadoutUnit::*;
    let options = [
        (Document, "unit.document"), (Px, "units.px"), (Pt, "units.pt"),
        (Mm, "units.mm"), (Cm, "units.cm"), (M, "doc_setup.measurement_unit.m"),
        (In, "units.in"), (Ft, "doc_setup.measurement_unit.ft"),
    ];
    let current = options.iter().position(|(value, _)| value == unit).unwrap_or(0);
    let mut index = current;
    egui::ComboBox::from_id_salt("measurement_unit")
        .width(ui.available_width())
        .selected_text(text(options[index].1))
        .show_index(ui, &mut index, options.len(), |index| text(options[index].1)).labelled_by(label);
    *unit = options[index].0;
    index != current
}

fn arrangement(ui: &mut Ui, label: &str, value: &mut bool, first: &str, second: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let old = *value;
        if dtool_design::tab(ui, !*value, text(first)).clicked() {
            *value = false;
        }
        if dtool_design::tab(ui, *value, text(second)).clicked() {
            *value = true;
        }
        *value != old
    }).inner
}

#[derive(Default)]
pub(crate) struct SidesEdit {
    changed: bool,
    pub(crate) invalid: bool,
    pub(crate) pending: Option<(egui::Id, Margins)>,
}

pub(crate) fn sides_editor(ui: &mut Ui, sides: &mut Margins, units: Units, label: &str, reset: bool) -> SidesEdit {
    ui.push_id(label, |ui| {
        let mut changed = toggle(ui, &mut sides.enabled, text(label)).changed();
        let fields = [
            "doc_setup.margin.left", "doc_setup.margin.right",
            "doc_setup.margin.top", "doc_setup.margin.bottom",
        ].map(|key| (key, ui.make_persistent_id(key)));
        if reset || (changed && !sides.enabled) {
            for (_, id) in fields { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
        }
        if !sides.enabled {
            return SidesEdit { changed, ..SidesEdit::default() };
        }
        if toggle(ui, &mut sides.linked, text("doc_setup.margin.linked")).changed() {
            if sides.linked {
                sides.right = sides.left;
                sides.top = sides.left;
                sides.bottom = sides.left;
            }
            changed = true;
        }
        let range = 0.0..=f64::from(f32::MAX);
        let suffix = crate::captions::units_identity(units);
        let parse = |input: &str| input.strip_suffix(suffix).unwrap_or(input).trim_end()
            .parse::<f64>().ok().filter(|value| range.contains(value));
        let spec = NumericSpec::new("")
            .range(range.clone())
            .width(76.0)
            .decimals(3)
            .suffix(suffix)
            .parser(&parse);
        let mut invalid = false;
        let mut linked_value = None;
        let mut pending = None;
        let mut edges = [&mut sides.left, &mut sides.right, &mut sides.top, &mut sides.bottom];
        for start in [0, 2] {
            dtool_design::panel_columns(ui, 2, 144.0, |ui, column| {
                let index = start + column;
                let (key, id) = fields[index];
                let side = &mut *edges[index];
                ui.horizontal_wrapped(|ui| {
                    let caption = ui.label(text(key));
                    let mut value = f64::from(*side);
                    if numeric_value(ui, id, &mut value, &spec).labelled_by(caption.id).changed() {
                        *side = value as f32;
                        linked_value = Some(*side);
                        changed = true;
                    }
                    let edit = dtool_design::numeric_state(ui.ctx(), id);
                    if let Some(draft) = edit.buffer.as_deref().filter(|_| edit.edited) {
                        match spec.parse_text(draft) {
                            Some(value) => pending = Some((id, index, value as f32)),
                            None => invalid = true,
                        }
                    }
                });
            });
        }
        if let Some(value) = linked_value.filter(|_| sides.linked) {
            sides.left = value;
            sides.right = value;
            sides.top = value;
            sides.bottom = value;
        }
        let pending = pending.map(|(id, index, value)| {
            let mut draft = *sides;
            let edges = [&mut draft.left, &mut draft.right, &mut draft.top, &mut draft.bottom];
            if draft.linked {
                for edge in edges { *edge = value; }
            } else {
                *edges[index] = value;
            }
            (id, draft)
        });
        SidesEdit { changed, invalid, pending }
    })
    .inner
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedPreset {
    document: DocNewArgs,
    options: Options,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SavedPresets {
    entries: BTreeMap<String, SavedPreset>,
    name: String,
    loaded: bool,
    favourites: BTreeSet<String>,
}

fn presets_path() -> PathBuf {
    dtool_core::prefs::Prefs::default_path().with_file_name("new_document_presets.json")
}

fn read_presets(path: &Path) -> io::Result<BTreeMap<String, SavedPreset>> {
    read_json_or_default(path)
}

fn favourites_path() -> PathBuf {
    presets_path().with_file_name("new_document_favourites.json")
}

fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let file_error = || text("error.io_error").to_owned();
    let body = serde_json::to_vec_pretty(value).map_err(|_| file_error())?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| file_error())?;
    }
    dtool_core::native_io::write_atomic(path, &body).map_err(|_| file_error())
}

impl NewDocumentDialogState {
    pub(crate) fn load_last_used() -> Option<Self> {
        let path = presets_path().with_file_name("new_document_last_used.json");
        let saved: SavedPreset = read_json_or_default::<Option<SavedPreset>>(&path).ok()??;
        let mut state = Self::default();
        let name = saved.document.preset.clone();
        state.apply_saved_preset(name, saved);
        Some(state)
    }

    pub(crate) fn last_used_snapshot(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            units: self.units,
            dpi: self.dpi,
            pages: self.pages,
            preset: self.preset.clone(),
            options: self.options.clone(),
            ..Self::default()
        }
    }

    pub(crate) fn save_last_used(&self) -> Result<(), String> {
        let saved = SavedPreset {
            document: DocNewArgs {
                width: self.width,
                height: self.height,
                units: self.units,
                dpi: self.dpi,
                pages: self.pages,
                preset: self.preset.clone(),
            },
            options: self.options.clone(),
        };
        write_json(&presets_path().with_file_name("new_document_last_used.json"), &saved)
    }

    pub(crate) fn saved_favourite_names() -> Vec<String> {
        let mut state = Self::default();
        state.load_saved_presets();
        state.favourite_names()
    }

    pub(super) fn favourite_names(&self) -> Vec<String> {
        self.saved_presets.favourites.iter().filter(|name|
            self.saved_presets.entries.contains_key(*name) || builtin_preset_names().any(|preset| preset == name.as_str())
        ).cloned().collect()
    }

    pub(super) fn is_favourite(&self, name: &str) -> bool {
        self.saved_presets.favourites.contains(name)
    }

    pub(super) fn preset_matches(&self, name: &str, filter: &str) -> bool {
        (!self.favourites_only || self.is_favourite(name)) && name.to_lowercase().contains(filter)
    }

    pub(super) fn toggle_favourite(&mut self, name: &str) {
        let path = favourites_path();
        let result = (|| {
            let mut names: BTreeSet<String> = read_json_or_default(&path).map_err(|_| text("error.io_error").to_owned())?;
            if !names.remove(name) { names.insert(name.to_owned()); }
            write_json(&path, &names)?;
            self.saved_presets.favourites = names;
            Ok(())
        })();
        self.error_message = result.err();
        if self.error_message.is_none() {
            self.select_visible_favourite();
        }
    }

    pub(crate) fn saved_preset_names() -> Vec<String> {
        read_presets(&presets_path()).unwrap_or_default().into_keys().collect()
    }

    pub(crate) fn select_saved_preset(&mut self, name: &str) -> bool {
        self.load_saved_presets();
        let Some(preset) = self.saved_presets.entries.get(name).cloned() else {
            return false;
        };
        self.apply_saved_preset(Some(name.to_owned()), preset);
        true
    }

    fn apply_saved_preset(&mut self, name: Option<String>, preset: SavedPreset) {
        let doc = preset.document;
        self.width = doc.width;
        self.height = doc.height;
        self.units = doc.units;
        self.dpi = doc.dpi;
        self.pages = doc.pages;
        let kind = preset.options.kind.or(self.options.kind);
        self.options = preset.options;
        self.options.kind = kind;
        self.preset = name;
        self.error_message = None;
        self.reset_dimension_edits = true;
    }

    pub(super) fn convert_units(&mut self, units: Units) {
        if units != self.units {
            let factor = unit_factor(self.units, self.dpi, units, self.dpi);
            self.width *= factor;
            self.height *= factor;
            self.options.scale(factor);
            self.units = units;
        }
    }

    pub(super) fn load_saved_presets(&mut self) {
        if !self.saved_presets.loaded {
            self.saved_presets.loaded = true;
            match read_presets(&presets_path()) {
                Ok(entries) => self.saved_presets.entries = entries,
                Err(_) => self.error_message = Some(text("error.io_error").to_owned()),
            }
            match read_json_or_default(&favourites_path()) {
                Ok(names) => self.saved_presets.favourites = names,
                Err(_) => self.error_message = Some(text("error.io_error").to_owned()),
            }
        }
    }

    pub(super) fn show_saved_presets(&mut self, ui: &mut Ui, filter: &str) -> (bool, bool, bool) {
        let mut entries = self
            .saved_presets
            .entries
            .iter()
            .filter(|(name, _)| self.preset_matches(name, filter))
            .peekable();
        if entries.peek().is_none() {
            return (false, false, false);
        }
        ui.strong(text("pref.home.presets"));
        let mut selected = None;
        let mut toggle = None;
        let mut create = false;
        ui.horizontal_wrapped(|ui| {
            for (name, preset) in entries {
                let doc = &preset.document;
                let (clicked, double_clicked, favourite_clicked) = preset_card(
                    ui,
                    name,
                    doc.width,
                    doc.height,
                    doc.units,
                    self.preset.as_deref() == Some(name),
                    self.is_favourite(name),
                );
                if favourite_clicked {
                    toggle = Some(name.clone());
                } else if clicked {
                    selected = Some((name.clone(), preset.clone()));
                    create = double_clicked;
                }
            }
        });
        let changed = selected.is_some() || toggle.is_some();
        if let Some((name, preset)) = selected {
            self.apply_saved_preset(Some(name), preset);
        }
        if let Some(name) = toggle {
            self.toggle_favourite(&name);
        }
        ui.add_space(10.0);
        (true, changed, create)
    }

    pub(super) fn show_save_preset(&mut self, ui: &mut Ui, valid: bool) -> bool {
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.saved_presets.name)
                    .hint_text(text("dialog.new_document.preset_name"))
                    .desired_width(164.0)
                    .char_limit(64),
            );
            let can_save = valid && !self.saved_presets.name.trim().is_empty();
            if ui
                .add_enabled_ui(can_save, |ui| {
                    button(ui, text("dialog.new_document.save_preset"))
                })
                .inner
                .clicked()
            {
                self.error_message = self.save_preset().err();
            }
            edit.has_focus() || edit.lost_focus()
        })
        .inner
    }

    fn save_preset(&mut self) -> Result<(), String> {
        if self.pages == 0 {
            return Err(text("error.invalid_params").to_owned());
        }
        prepare_new_document(self, 1).map_err(|error| text(&error.message_key).to_owned())?;
        let path = presets_path();
        let file_error = || text("error.io_error").to_owned();
        let mut entries = read_presets(&path).map_err(|_| file_error())?;
        let name = self.saved_presets.name.trim();
        let normalized = name.to_lowercase();
        let matches_name = |existing: &str| existing.to_lowercase() == normalized;
        if builtin_preset_names().any(matches_name) || entries.keys().any(|existing| matches_name(existing)) {
            return Err(text("dialog.new_document.preset_exists").to_owned());
        }
        let document = DocNewArgs {
            width: self.width,
            height: self.height,
            units: self.units,
            dpi: self.dpi,
            pages: self.pages,
            preset: None,
        };
        entries.insert(
            name.to_owned(),
            SavedPreset {
                document,
                options: self.options.clone(),
            },
        );
        write_json(&path, &entries)?;
        self.preset = Some(name.to_owned());
        self.saved_presets.entries = entries;
        self.saved_presets.name.clear();
        self.preset_search.clear();
        Ok(())
    }
}
