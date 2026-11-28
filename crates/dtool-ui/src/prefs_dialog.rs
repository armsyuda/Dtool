use std::path::{Path, PathBuf};

use dtool_core::prefs::{Prefs, TextReplacement};
use dtool_design::tokens::parse_hex;
use dtool_design::{button, color, dropdown, number_field, panel_header, slider, space, tab, toggle};
use dtool_strings::{format, text};
use egui::Ui;

use crate::settings::UiSettings;
use crate::shortcuts_page::ShortcutsPage;

pub const TAB_ORDER_SOURCE: &str = "dlg_Settings_PrefGeneralPage.txt";
pub const COMMAND_QUERY: &str = "pref.query";
pub const COMMAND_SET: &str = "pref.set";
pub const GENERAL_SHOTS: [&str; 4] = [
    "B14_settings_general_p1",
    "B14_settings_general_p2",
    "B14_settings_general_p3",
    "B14_settings_general_p4",
];
pub const PERFORMANCE_SHOT: &str = "B14_settings_performance";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsTab {
    General,
    MachineLearning,
    ModelContextProtocol,
    Scripting,
    Colour,
    Performance,
    UserInterface,
    GridGuideColours,
    HomeScreen,
    Tools,
    Shortcuts,
    Assistant,
    PhotoshopPlugins,
    LinkedServices,
    AutoCorrect,
    Abbreviations,
    FillerText,
    TitleExceptions,
    Miscellaneous,
}

impl PrefsTab {
    pub const ALL: [PrefsTab; 19] = [
        PrefsTab::General,
        PrefsTab::MachineLearning,
        PrefsTab::ModelContextProtocol,
        PrefsTab::Scripting,
        PrefsTab::Colour,
        PrefsTab::Performance,
        PrefsTab::UserInterface,
        PrefsTab::GridGuideColours,
        PrefsTab::HomeScreen,
        PrefsTab::Tools,
        PrefsTab::Shortcuts,
        PrefsTab::Assistant,
        PrefsTab::PhotoshopPlugins,
        PrefsTab::LinkedServices,
        PrefsTab::AutoCorrect,
        PrefsTab::Abbreviations,
        PrefsTab::FillerText,
        PrefsTab::TitleExceptions,
        PrefsTab::Miscellaneous,
    ];

    pub fn key(self) -> &'static str {
        match self {
            PrefsTab::General => "pref.tab.general",
            PrefsTab::MachineLearning => "pref.tab.machine_learning",
            PrefsTab::ModelContextProtocol => "pref.tab.mcp",
            PrefsTab::Scripting => "pref.tab.scripting",
            PrefsTab::Colour => "pref.tab.colour",
            PrefsTab::Performance => "pref.tab.performance",
            PrefsTab::UserInterface => "pref.tab.ui",
            PrefsTab::GridGuideColours => "pref.tab.grid_colours",
            PrefsTab::HomeScreen => "pref.tab.home",
            PrefsTab::Tools => "pref.tab.tools",
            PrefsTab::Shortcuts => "pref.tab.shortcuts",
            PrefsTab::Assistant => "pref.tab.assistant",
            PrefsTab::PhotoshopPlugins => "pref.tab.photoshop_plugins",
            PrefsTab::LinkedServices => "pref.tab.linked_services",
            PrefsTab::AutoCorrect => "pref.tab.autocorrect",
            PrefsTab::Abbreviations => "pref.tab.abbreviations",
            PrefsTab::FillerText => "pref.tab.filler",
            PrefsTab::TitleExceptions => "pref.tab.title_exceptions",
            PrefsTab::Miscellaneous => "pref.tab.misc",
        }
    }

    pub fn title(self) -> &'static str {
        text(self.key())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsAction {
    ResetFills,
    ResetBrushes,
    ResetObjectStyles,
    ResetTextStyles,
    ResetUserDefaults,
    GridResetAppDefaults,
    GridDefaultsFromDocument,
    GridDefaultsToDocument,
    BrowseDictionaryFolder,
    BrowseOcioConfig,
    DevelopAssistant,
    LinkService,
}

const ML_MODELS: [(&str, u32, &str, &str); 12] = [
    ("segmentation", 290, "pref.ml.segmentation", "pref.ml.segmentation.desc"),
    ("depth", 99, "pref.ml.depth", "pref.ml.depth.desc"),
    ("colourisation", 215, "pref.ml.colourisation", "pref.ml.colourisation.desc"),
    ("super_resolution", 5, "pref.ml.super_resolution", "pref.ml.super_resolution.desc"),
    ("shape_recognition", 5, "pref.ml.shape_recognition", "pref.ml.shape_recognition.desc"),
    ("deblur", 130, "pref.ml.deblur", "pref.ml.deblur.desc"),
    ("sdr_to_hdr", 8, "pref.ml.sdr_to_hdr", "pref.ml.sdr_to_hdr.desc"),
    ("noise_reduction", 95, "pref.ml.noise_reduction", "pref.ml.noise_reduction.desc"),
    ("mixed_light", 1, "pref.ml.mixed_light", "pref.ml.mixed_light.desc"),
    ("object_detection", 80, "pref.ml.object_detection", "pref.ml.object_detection.desc"),
    ("region_detection", 197, "pref.ml.region_detection", "pref.ml.region_detection.desc"),
    ("remove_stars", 109, "pref.ml.remove_stars", "pref.ml.remove_stars.desc"),
];

const RGB32_PROFILES: [&str; 3] = ["sRGB IEC61966-2.1 (Linear)", "Adobe RGB (1998) (Linear)", "ACEScg"];

pub fn profile_names(model: dtool_core::colour::ColourModel) -> Vec<String> {
    dtool_core::colour::services()
        .read()
        .map(|services| services.profiles.list(model).into_iter().map(str::to_string).collect())
        .unwrap_or_default()
}

fn profile_choice(ui: &mut Ui, key: &str, value: &mut String, model: dtool_core::colour::ColourModel) {
    let names = profile_names(model);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    name_choice(ui, key, key, value, &names);
}

const INTENTS: [(&str, &str); 4] = [
    ("perceptual", "pref.colour.intent.perceptual"),
    ("relative_colorimetric", "pref.colour.intent.relative"),
    ("saturation", "pref.colour.intent.saturation"),
    ("absolute_colorimetric", "pref.colour.intent.absolute"),
];

const VIEW_QUALITY: [(&str, &str); 5] = [
    ("nearest", "pref.performance.view_quality.nearest"),
    ("bilinear", "pref.performance.view_quality.bilinear"),
    ("bicubic", "pref.performance.view_quality.bicubic"),
    ("lanczos3_nonseparable", "pref.performance.view_quality.lanczos_nonsep"),
    ("lanczos3_separable", "pref.performance.view_quality.lanczos_sep"),
];

const RENDERERS: [(&str, &str); 3] = [
    ("default", "pref.performance.renderer.default"),
    ("warp", "pref.performance.renderer.warp"),
    ("software", "pref.performance.renderer.software"),
];

const RETINA: [(&str, &str); 3] = [
    ("auto", "pref.performance.retina.auto"),
    ("low", "pref.performance.retina.low"),
    ("high", "pref.performance.retina.high"),
];

const VIEW_MODES: [(&str, &str); 3] = [
    ("vector", "pref.ui.view_mode.vector"),
    ("pixel", "pref.ui.view_mode.pixel"),
    ("auto", "pref.ui.view_mode.auto"),
];

const HANDLE_SIZES: [(f32, &str); 4] = [
    (0.75, "pref.tools.handle.small"),
    (1.0, "pref.tools.handle.default"),
    (1.25, "pref.tools.handle.medium"),
    (1.5, "pref.tools.handle.large"),
];

const MARQUEE: [(&str, &str); 2] = [
    ("intersect", "pref.tools.marquee.intersect"),
    ("enclose", "pref.tools.marquee.enclose"),
];

const ASPECT: [(&str, &str); 2] = [
    ("shift_constrains", "pref.tools.aspect.shift_constrains"),
    ("shift_unconstrains", "pref.tools.aspect.shift_unconstrains"),
];

const TABLET: [(&str, &str); 3] = [
    ("windows_ink", "pref.tools.tablet.windows_ink"),
    ("wintab", "pref.tools.tablet.wintab"),
    ("pointer", "pref.tools.tablet.pointer"),
];

const LANGUAGES: [(&str, &str); 2] = [
    ("en_gb", "pref.lang.en_gb"),
    ("en_us", "pref.lang.en_us"),
];

const ACT_NEW_PIXEL: (&str, &str) = ("new_pixel_layer", "pref.assistant.action.new_pixel_layer");
const ACT_NOTHING: (&str, &str) = ("do_nothing", "pref.assistant.action.do_nothing");
const ACT_RASTERISE: (&str, &str) = ("rasterise", "pref.assistant.action.rasterise");
const ACT_RASTERISE_MASK: (&str, &str) = ("rasterise_mask", "pref.assistant.action.rasterise_mask");
const ACT_LIVE: (&str, &str) = ("live_filter", "pref.assistant.action.live_filter");
const ACT_SHARE: (&str, &str) = ("share", "pref.assistant.action.share");
const ACT_CHILD: (&str, &str) = ("add_child", "pref.assistant.action.add_child");
const ACT_NEW_LAYER: (&str, &str) = ("add_new", "pref.assistant.action.add_new");

fn data_dir(name: &str) -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join("DTool").join(name),
        None => PathBuf::from(name),
    }
}

fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

fn section(ui: &mut Ui, key: &str) {
    ui.add_space(space("md"));
    panel_header(ui, text(key));
    ui.add_space(space("xs"));
}

fn note(ui: &mut Ui, key: &str) {
    ui.small(text(key));
}

fn planned(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_enabled_ui(false, add);
        ui.label(text("menu.planned.suffix"));
    });
}

fn switch(ui: &mut Ui, value: &mut bool, key: &str) {
    toggle(ui, value, text(key));
}

fn switch_with_note(ui: &mut Ui, value: &mut bool, key: &str, note_key: &str) {
    toggle(ui, value, text(key));
    note(ui, note_key);
}

fn choice(ui: &mut Ui, id: &str, label_key: &str, value: &mut String, options: &[(&str, &str)]) {
    ui.horizontal(|ui| {
        ui.label(text(label_key));
        let labels: Vec<&str> = options.iter().map(|(_, key)| text(key)).collect();
        let before = options
            .iter()
            .position(|(v, _)| *v == value.as_str())
            .unwrap_or(0);
        let mut index = before;
        dropdown(ui, id, &mut index, &labels);
        if index != before {
            if let Some((v, _)) = options.get(index) {
                *value = v.to_string();
            }
        }
    });
}

fn name_choice(ui: &mut Ui, id: &str, label_key: &str, value: &mut String, names: &[&str]) {
    ui.horizontal(|ui| {
        ui.label(text(label_key));
        let mut list: Vec<&str> = names.to_vec();
        if !list.contains(&value.as_str()) {
            list.push(value.as_str());
        }
        let before = list.iter().position(|n| *n == value.as_str()).unwrap_or(0);
        let mut index = before;
        dropdown(ui, id, &mut index, &list);
        let picked = list.get(index).map(|n| n.to_string());
        if index != before {
            if let Some(name) = picked {
                *value = name;
            }
        }
    });
}

fn num_u32(ui: &mut Ui, label_key: &str, value: &mut u32) {
    let mut v = *value as f64;
    number_field(ui, &mut v, text(label_key));
    *value = v.max(0.0).round() as u32;
}

fn num_f32(ui: &mut Ui, label_key: &str, value: &mut f32) {
    let mut v = *value as f64;
    number_field(ui, &mut v, text(label_key));
    *value = v as f32;
}

fn colour_row(ui: &mut Ui, label_key: &str, hex: &mut String) {
    ui.horizontal(|ui| {
        let mut c = parse_hex(hex.as_str()).unwrap_or_else(|| color("well.border"));
        let response = egui::color_picker::color_edit_button_srgba(
            ui,
            &mut c,
            egui::color_picker::Alpha::OnlyBlend,
        );
        if response.changed() {
            *hex = format!("#{:02x}{:02x}{:02x}{:02x}", c.r(), c.g(), c.b(), c.a());
        }
        ui.label(text(label_key));
    });
}

fn string_list(ui: &mut Ui, items: &mut Vec<String>, entry: &mut String, selected: &mut Option<usize>) {
    for (i, item) in items.iter().enumerate() {
        if tab(ui, *selected == Some(i), item.as_str()).clicked() {
            *selected = Some(i);
        }
    }
    ui.horizontal(|ui| {
        ui.text_edit_singleline(entry);
        if button(ui, text("pref.add")).clicked() && !entry.trim().is_empty() {
            items.push(entry.trim().to_string());
            entry.clear();
        }
        if button(ui, text("pref.remove")).clicked() {
            if let Some(i) = selected.take() {
                if i < items.len() {
                    items.remove(i);
                }
            }
        }
    });
}

fn home_item_key(id: &str) -> &'static str {
    match id {
        "recent" => "pref.home.recent",
        "templates" => "pref.home.templates",
        "favourites" => "pref.home.favourites",
        "presets" => "pref.home.presets",
        _ => "pref.home.unknown",
    }
}

pub struct PrefsDialog {
    pub open: bool,
    pub tab: PrefsTab,
    pub search: String,
    pub actions: Vec<PrefsAction>,
    pub shortcuts: ShortcutsPage,
    pub(crate) open_pie_settings: bool,
    loaded: bool,
    prefs_path: PathBuf,
    ui_path: PathBuf,
    entry: String,
    entry_with: String,
    selected: Option<usize>,
    window_size: Option<egui::Vec2>,
    resize_start: Option<(egui::Pos2, egui::Vec2)>,
}

impl Default for PrefsDialog {
    fn default() -> Self {
        Self {
            open: false,
            tab: PrefsTab::General,
            search: String::new(),
            actions: Vec::new(),
            shortcuts: ShortcutsPage::default(),
            open_pie_settings: false,
            loaded: false,
            prefs_path: Prefs::default_path(),
            ui_path: UiSettings::default_path(),
            entry: String::new(),
            entry_with: String::new(),
            selected: None,
            window_size: None,
            resize_start: None,
        }
    }
}

impl PrefsDialog {
    pub fn with_paths(prefs_path: PathBuf, ui_path: PathBuf) -> Self {
        Self { prefs_path, ui_path, ..Self::default() }
    }

    pub fn prefs_path(&self) -> &std::path::Path {
        &self.prefs_path
    }

    pub fn load(&mut self, prefs: &mut Prefs, settings: &mut UiSettings) {
        *prefs = Prefs::load(&self.prefs_path);
        *settings = UiSettings::load(&self.ui_path);
        prefs.text_contrast = settings.text_contrast;
        let _ = dtool_core::colour::sync_app_prefs(prefs);
        self.loaded = true;
    }

    pub fn run(
        &mut self,
        prefs: &mut Prefs,
        settings: &mut UiSettings,
        name: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, dtool_core::prefs::PrefsError> {
        if !self.loaded {
            self.load(prefs, settings);
        }
        let result = prefs.run(name, params, &self.prefs_path);
        if result.is_ok() && name == COMMAND_SET {
            settings.text_contrast = prefs.text_contrast;
            let _ = settings.save(&self.ui_path);
        }
        result
    }

    pub fn show(&mut self, ui: &mut Ui, prefs: &mut Prefs, settings: &mut UiSettings) {
        if !self.loaded {
            self.load(prefs, settings);
        }
        if !self.open {
            return;
        }
        let before_prefs = prefs.clone();
        let before_settings = serde_json::to_value(&*settings).ok();
        let mut open = self.open;
        let mut dismiss = false;
        let viewport = ui.ctx().content_rect();
        let maximum = (viewport.size() - egui::vec2(32.0, 64.0)).max(egui::vec2(300.0, 220.0));
        let initial = egui::vec2(820.0, 630.0).min(maximum);
        let minimum = egui::vec2(460.0, 300.0).min(maximum);
        let size = self.window_size.unwrap_or(initial).clamp(minimum, maximum);
        egui::Window::new(text("pref.dialog.title"))
            .collapsible(false)
            .fixed_size(size)
            .default_pos(viewport.center() - initial * 0.5)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                let content = ui.available_rect_before_wrap();
                let body_height = (content.height() - 48.0).max(140.0);
                if ui.available_width() >= 560.0 {
                    let navigation_width = (ui.available_width() * 0.25).clamp(168.0, 208.0);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(navigation_width);
                            dtool_design::dialog_section(ui, |ui| {
                                let search_width = ui.available_width();
                                self.search_field(ui, search_width);
                                egui::ScrollArea::vertical().id_salt("pref.navigation")
                                    .max_height(body_height - 38.0).auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for page in self.filtered_tabs() {
                                            let selected = self.tab == page;
                                            let width = ui.available_width();
                                            let row = dtool_design::with_icon_selection(ui, |ui| {
                                                ui.add(egui::Button::selectable(selected, page.title())
                                                    .truncate().min_size(egui::vec2(width, 25.0)))
                                            });
                                            if row.on_hover_text(page.title()).clicked() {
                                                self.select_page(page);
                                            }
                                        }
                                    });
                            });
                        });
                        ui.vertical(|ui| self.settings_panel(ui, body_height, prefs, settings));
                    });
                } else {
                    ui.horizontal(|ui| {
                        let search_width = (ui.available_width() * 0.42).max(94.0);
                        self.search_field(ui, search_width);
                        egui::ComboBox::from_id_salt("pref.category").selected_text(self.tab.title())
                            .width(ui.available_width()).truncate().show_ui(ui, |ui| {
                                for page in self.filtered_tabs() {
                                    if ui.selectable_label(self.tab == page, page.title()).clicked() {
                                        self.select_page(page);
                                    }
                                }
                            });
                    });
                    self.settings_panel(ui, body_height - 30.0, prefs, settings);
                }
                ui.separator();
                ui.allocate_ui_with_layout(egui::vec2(content.width() - 20.0, 26.0),
                    egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        dismiss = button(ui, text("common.close")).clicked();
                    });
                ui.expand_to_include_rect(content);
                self.resize_window(ui, content.right_bottom(), size, minimum, maximum);
            });
        self.open = open && !dismiss && (egui::Popup::is_any_open(ui.ctx())
            || dtool_design::dialog_keys(ui) != Some(dtool_design::DialogKey::Cancel));
        if *prefs != before_prefs {
            settings.text_contrast = prefs.text_contrast;
            let _ = prefs.save(&self.prefs_path);
            let _ = dtool_core::colour::sync_app_prefs(prefs);
        }
        if serde_json::to_value(&*settings).ok() != before_settings {
            let _ = settings.save(&self.ui_path);
        }
    }

    fn resize_window(&mut self, ui: &mut Ui, corner: egui::Pos2, size: egui::Vec2, minimum: egui::Vec2, maximum: egui::Vec2) {
        let rect = egui::Rect::from_min_max(corner - egui::Vec2::splat(18.0), corner);
        let handle = ui.interact(rect, ui.id().with("pref.resize"), egui::Sense::DRAG)
            .on_hover_and_drag_cursor(egui::CursorIcon::ResizeNwSe);
        handle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, "Resize window"));
        let anchor = corner - egui::Vec2::splat(2.0);
        let stroke = ui.style().interact(&handle).fg_stroke;
        for length in [4.0, 8.0, 12.0] {
            ui.painter().line_segment([anchor - egui::vec2(length, 0.0), anchor - egui::vec2(0.0, length)], stroke);
        }
        if let Some(origin) = ui.input(|input| input.events.iter().rev().find_map(|event| match event {
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                if rect.contains(*pos) => Some(*pos),
            _ => None,
        })) && ui.ctx().layer_id_at(origin) == Some(ui.layer_id()) {
            self.resize_start = Some((origin, size));
        }
        let (pointer, down) = ui.input(|input| (input.pointer.interact_pos(), input.pointer.primary_down()));
        if let Some((origin, initial)) = self.resize_start && let Some(pointer) = pointer {
            self.window_size = Some((initial + (pointer - origin)).clamp(minimum, maximum));
            ui.ctx().request_repaint();
        }
        if !down {
            self.resize_start = None;
        }
    }

    fn search_field(&mut self, ui: &mut Ui, width: f32) {
        ui.add(egui::TextEdit::singleline(&mut self.search).id_salt("pref.category_filter")
            .hint_text(text("pref.search")).desired_width(width));
    }

    fn filtered_tabs(&self) -> impl Iterator<Item = PrefsTab> + use<> {
        let query = self.search.trim().to_lowercase();
        PrefsTab::ALL.into_iter().filter(move |page| page.title().to_lowercase().contains(&query))
    }

    fn select_page(&mut self, page: PrefsTab) {
        if self.tab != page {
            self.tab = page;
            self.selected = None;
        }
    }

    fn settings_panel(&mut self, ui: &mut Ui, height: f32, prefs: &mut Prefs, settings: &mut UiSettings) {
        dtool_design::dialog_section(ui, |ui| {
            ui.set_width(ui.available_width());
            panel_header(ui, self.tab.title());
            egui::ScrollArea::vertical().id_salt(("pref.body", self.tab.key()))
                .max_height((height - 28.0).max(96.0)).auto_shrink([false, false])
                .show(ui, |ui| self.show_body(ui, prefs, settings));
        });
    }

    fn show_body(&mut self, ui: &mut Ui, prefs: &mut Prefs, settings: &mut UiSettings) {
        match self.tab {
            PrefsTab::General => self.show_general(ui, prefs, settings),
            PrefsTab::MachineLearning => self.show_machine_learning(ui, prefs),
            PrefsTab::ModelContextProtocol => self.show_mcp(ui, prefs),
            PrefsTab::Scripting => self.show_scripting(ui, prefs),
            PrefsTab::Colour => self.show_colour(ui, prefs),
            PrefsTab::Performance => self.show_performance(ui, prefs),
            PrefsTab::UserInterface => self.show_ui(ui, prefs, settings),
            PrefsTab::GridGuideColours => self.show_grid_colours(ui, prefs),
            PrefsTab::HomeScreen => self.show_home(ui, prefs),
            PrefsTab::Tools => self.show_tools(ui, prefs, settings),
            PrefsTab::Shortcuts => {
                if ui.button("파이메뉴 설정").clicked() {
                    self.open_pie_settings = true;
                }
                self.shortcuts.show(ui);
            },
            PrefsTab::Assistant => self.show_assistant(ui, prefs),
            PrefsTab::PhotoshopPlugins => self.show_plugins(ui, prefs),
            PrefsTab::LinkedServices => self.show_linked(ui, prefs),
            PrefsTab::AutoCorrect => self.show_autocorrect(ui, prefs),
            PrefsTab::Abbreviations => self.show_abbreviations(ui, prefs),
            PrefsTab::FillerText => self.show_filler(ui, prefs),
            PrefsTab::TitleExceptions => self.show_title_exceptions(ui, prefs),
            PrefsTab::Miscellaneous => self.show_misc(ui),
        }
    }

    fn show_general(&mut self, ui: &mut Ui, prefs: &mut Prefs, settings: &mut UiSettings) {
        section(ui, "pref.general.document");
        switch(ui, &mut prefs.reopen_docs, "pref.general.reopen_docs");
        switch(ui, &mut prefs.limit_zoom_100, "pref.general.limit_zoom_100");
        switch(ui, &mut prefs.open_floating, "pref.general.open_floating");
        switch(ui, &mut prefs.open_in_active_group, "pref.general.open_in_active_group");
        switch(ui, &mut prefs.auto_update_links, "pref.general.auto_update_links");
        switch(ui, &mut prefs.save_thumbnails, "pref.general.save_thumbnails");
        switch_with_note(ui, &mut prefs.psd_save_over, "pref.general.psd_save_over", "pref.general.psd_save_over.warning");

        section(ui, "pref.general.text");
        switch(ui, &mut prefs.preserve_unicode_breaks, "pref.general.preserve_unicode_breaks");
        switch(ui, &mut prefs.smart_cut_paste, "pref.general.smart_cut_paste");
        switch(ui, &mut prefs.filler_as_text, "pref.general.filler_as_text");
        switch(ui, &mut prefs.psd_text_as_text, "pref.general.psd_text_as_text");

        section(ui, "pref.general.haptics");
        switch(ui, &mut prefs.haptics, "pref.general.enable_haptics");

        section(ui, "pref.general.other");
        switch(ui, &mut prefs.copy_as_svg, "pref.general.copy_as_svg");
        switch(ui, &mut prefs.prefer_metafile_paste, "pref.general.prefer_metafile_paste");
        switch(ui, &mut prefs.keep_selection_after_delete, "pref.general.keep_selection_after_delete");
        switch(ui, &mut prefs.load_xmp_sidecars, "pref.general.load_xmp_sidecars");
        switch(ui, &mut prefs.refine_heic_depth, "pref.general.refine_heic_depth");
        switch(ui, &mut prefs.apply_gain_maps, "pref.general.apply_gain_maps");
        switch(ui, &mut prefs.psd_smart_objects, "pref.general.psd_smart_objects");
        switch(ui, &mut prefs.show_home_on_startup, "pref.general.show_home_on_startup");
        switch(ui, &mut prefs.send_usage_data, "pref.general.send_usage_data");
        switch(ui, &mut prefs.upload_crash_reports, "pref.general.upload_crash_reports");

        ui.add_space(space("sm"));
        note(ui, "pref.general.lens_profiles");
        if button(ui, text("pref.general.open_lens_profiles")).clicked() {
            open_folder(&data_dir("LensProfiles"));
        }
        note(ui, "pref.general.store_fonts");
        if button(ui, text("pref.general.open_fonts")).clicked() {
            open_folder(&data_dir("Fonts"));
        }

        ui.add_space(space("sm"));
        let mut language = settings.language.clone();
        choice(ui, "pref.general.language", "settings.language", &mut language, &[("ko", "settings.language.ko")]);
        settings.language = language;
        note(ui, "settings.restart_needed");
    }

    fn show_machine_learning(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        ui.label(text("pref.ml.intro"));
        section(ui, "pref.ml.inference");
        for (id, mb, title_key, desc_key) in ML_MODELS {
            ui.add_space(space("sm"));
            ui.horizontal(|ui| {
                let mut on = prefs.ml_models.iter().any(|m| m == id);
                let was = on;
                toggle(ui, &mut on, "");
                ui.strong(text(title_key));
                let size = mb.to_string();
                ui.label(format("pref.ml.size", &[("mb", size.as_str())]));
                if on != was {
                    if on {
                        prefs.ml_models.push(id.to_string());
                    } else {
                        prefs.ml_models.retain(|m| m != id);
                    }
                }
            });
            note(ui, desc_key);
        }
    }

    fn show_mcp(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        switch_with_note(ui, &mut prefs.mcp_enabled, "pref.mcp.enable", "pref.mcp.enable.desc");
        ui.add_enabled_ui(prefs.mcp_enabled, |ui| {
            switch_with_note(ui, &mut prefs.mcp_desktop_files, "pref.mcp.desktop_files", "pref.mcp.desktop_files.desc");
            switch_with_note(ui, &mut prefs.mcp_networks, "pref.mcp.networks", "pref.mcp.networks.desc");
            switch_with_note(ui, &mut prefs.mcp_use_saved_scripts, "pref.mcp.use_saved_scripts", "pref.mcp.use_saved_scripts.desc");
            switch_with_note(ui, &mut prefs.mcp_save_scripts, "pref.mcp.save_scripts", "pref.mcp.save_scripts.desc");
            switch_with_note(ui, &mut prefs.mcp_local_hints, "pref.mcp.local_hints", "pref.mcp.local_hints.desc");
            switch_with_note(ui, &mut prefs.mcp_share_hints, "pref.mcp.share_hints", "pref.mcp.share_hints.desc");
        });
    }

    fn show_scripting(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        switch_with_note(ui, &mut prefs.scripting_enabled, "pref.scripting.enable", "pref.scripting.enable.desc");
        section(ui, "pref.scripting.default_permissions");
        note(ui, "pref.scripting.default_permissions.desc");
        switch_with_note(ui, &mut prefs.script_file_system, "pref.scripting.file_system", "pref.scripting.file_system.desc");
        switch_with_note(ui, &mut prefs.script_networks, "pref.scripting.networks", "pref.scripting.networks.desc");
        section(ui, "pref.scripting.dynamic_code");
        switch(ui, &mut prefs.script_dynamic_code, "pref.scripting.dynamic_code.allow");
        section(ui, "pref.scripting.folders");
        note(ui, "pref.scripting.folders.desc");
        string_list(ui, &mut prefs.script_folders, &mut self.entry, &mut self.selected);
    }

    fn show_colour(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        profile_choice(ui, "pref.colour.rgb", &mut prefs.rgb_profile, dtool_core::colour::ColourModel::Rgb);
        name_choice(ui, "pref.colour.rgb32", "pref.colour.rgb32", &mut prefs.rgb32_profile, &RGB32_PROFILES);
        profile_choice(ui, "pref.colour.cmyk", &mut prefs.cmyk_profile, dtool_core::colour::ColourModel::Cmyk);
        profile_choice(ui, "pref.colour.grey", &mut prefs.grey_profile, dtool_core::colour::ColourModel::Grey);
        profile_choice(ui, "pref.colour.lab", &mut prefs.lab_profile, dtool_core::colour::ColourModel::Lab);
        choice(ui, "pref.colour.intent", "pref.colour.intent", &mut prefs.rendering_intent, &INTENTS);
        switch(ui, &mut prefs.black_point_compensation, "pref.colour.black_point");
        ui.horizontal(|ui| {
            switch(ui, &mut prefs.convert_to_working, "pref.colour.convert_to_working");
            switch(ui, &mut prefs.convert_warn, "pref.colour.and_warn");
        });
        switch(ui, &mut prefs.warn_assign_unprofiled, "pref.colour.warn_unprofiled");
        switch(ui, &mut prefs.hdr_32bit_default, "pref.colour.hdr_32bit");

        section(ui, "pref.colour.ocio");
        ui.label(text("pref.colour.ocio_file"));
        ui.horizontal(|ui| {
            if prefs.ocio_config.is_empty() {
                ui.label(text("pref.colour.ocio_none"));
            } else {
                ui.label(prefs.ocio_config.as_str());
            }
            if button(ui, text("pref.select")).clicked() {
                self.actions.push(PrefsAction::BrowseOcioConfig);
            }
        });
        ui.horizontal(|ui| {
            switch(ui, &mut prefs.ocio_by_filename, "pref.colour.ocio_by_filename");
            switch(ui, &mut prefs.ocio_warn_on_load, "pref.colour.ocio_warn");
        });

        section(ui, "pref.colour.exr");
        switch(ui, &mut prefs.exr_associate_alpha, "pref.colour.exr_associate_alpha");
        switch(ui, &mut prefs.exr_post_divide, "pref.colour.exr_post_divide");
        switch(ui, &mut prefs.exr_perturb_zero_alpha, "pref.colour.exr_perturb_zero_alpha");
    }

    fn show_performance(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        planned(ui, |ui| num_u32(ui, "pref.performance.ram_limit", &mut prefs.ram_limit));
        planned(ui, |ui| num_u32(ui, "pref.performance.disk_warn", &mut prefs.disk_warn_gb));
        num_u32(ui, "pref.performance.undo_limit", &mut prefs.undo_limit);
        choice(ui, "pref.performance.view_quality", "pref.performance.view_quality", &mut prefs.view_quality, &VIEW_QUALITY);
        switch(ui, &mut prefs.dither_gradients, "pref.performance.dither_gradients");
        switch(ui, &mut prefs.precise_clipping, "pref.performance.precise_clipping");
        ui.horizontal(|ui| {
            num_u32(ui, "pref.performance.recovery_interval", &mut prefs.recovery_interval_s);
            ui.label(text("pref.unit.seconds"));
        });
        planned(ui, |ui| choice(ui, "pref.performance.renderer", "pref.performance.renderer", &mut prefs.renderer, &RENDERERS));
        choice(ui, "pref.performance.retina", "pref.performance.retina", &mut prefs.retina_rendering, &RETINA);
        planned(ui, |ui| switch(ui, &mut prefs.opencl, "pref.performance.opencl"));
        if prefs.gpu_device.is_empty() {
            note(ui, "pref.performance.gpu_unknown");
        } else {
            ui.small(prefs.gpu_device.as_str());
        }
    }

    fn show_ui(&mut self, ui: &mut Ui, prefs: &mut Prefs, settings: &mut UiSettings) {
        slider(ui, &mut settings.background_grey, 0.0..=1.0, text("settings.background_grey"));
        slider(ui, &mut prefs.artboard_grey, 0.0..=1.0, text("pref.ui.artboard_grey"));
        slider(ui, &mut prefs.text_contrast, 0.0..=1.0, text("settings.text_contrast"));
        slider(ui, &mut settings.ui_brightness, 0.0..=1.0, text("settings.ui_brightness"));
        ui.horizontal(|ui| {
            ui.label(text("settings.ui_contrast"));
            if tab(ui, !settings.high_contrast, text("settings.ui_contrast.default")).clicked() {
                settings.high_contrast = false;
            }
            if tab(ui, settings.high_contrast, text("settings.ui_contrast.high")).clicked() {
                settings.high_contrast = true;
            }
        });
        ui.horizontal(|ui| {
            let accent = parse_hex(&prefs.accent_colour).unwrap_or_else(|| color("accent"));
            let [r, g, b, _] = accent.to_srgba_unmultiplied();
            let mut rgb = [r, g, b];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                let [r, g, b] = rgb;
                prefs.accent_colour = format!("#{r:02x}{g:02x}{b:02x}ff");
            }
            ui.label(text("pref.ui.accent"));
        });
        ui.horizontal(|ui| {
            num_u32(ui, "pref.ui.tooltip_delay", &mut prefs.tooltip_delay_ms);
            note(ui, "settings.restart_needed");
        });

        section(ui, "pref.ui.decimals");
        if button(ui, text("pref.reset")).clicked() {
            let d = Prefs::default();
            prefs.decimals_px = d.decimals_px;
            prefs.decimals_pt = d.decimals_pt;
            prefs.decimals_pc = d.decimals_pc;
            prefs.decimals_deg = d.decimals_deg;
            prefs.decimals_in = d.decimals_in;
            prefs.decimals_ft = d.decimals_ft;
            prefs.decimals_yd = d.decimals_yd;
            prefs.decimals_mi = d.decimals_mi;
            prefs.decimals_mm = d.decimals_mm;
            prefs.decimals_cm = d.decimals_cm;
            prefs.decimals_m = d.decimals_m;
            prefs.decimals_km = d.decimals_km;
        }
        egui::Grid::new("pref.ui.decimals.grid").show(ui, |ui| {
            num_u32(ui, "pref.ui.decimals.px", &mut prefs.decimals_px);
            num_u32(ui, "pref.ui.decimals.pt", &mut prefs.decimals_pt);
            num_u32(ui, "pref.ui.decimals.pc", &mut prefs.decimals_pc);
            num_u32(ui, "pref.ui.decimals.deg", &mut prefs.decimals_deg);
            ui.end_row();
            num_u32(ui, "pref.ui.decimals.in", &mut prefs.decimals_in);
            num_u32(ui, "pref.ui.decimals.ft", &mut prefs.decimals_ft);
            num_u32(ui, "pref.ui.decimals.yd", &mut prefs.decimals_yd);
            num_u32(ui, "pref.ui.decimals.mi", &mut prefs.decimals_mi);
            ui.end_row();
            num_u32(ui, "pref.ui.decimals.mm", &mut prefs.decimals_mm);
            num_u32(ui, "pref.ui.decimals.cm", &mut prefs.decimals_cm);
            num_u32(ui, "pref.ui.decimals.m", &mut prefs.decimals_m);
            num_u32(ui, "pref.ui.decimals.km", &mut prefs.decimals_km);
            ui.end_row();
        });

        ui.add_space(space("sm"));
        switch(ui, &mut prefs.lock_bg_on_import, "pref.ui.lock_bg_on_import");
        choice(ui, "pref.ui.view_mode", "pref.ui.view_mode", &mut prefs.default_view_mode, &VIEW_MODES);
        switch(ui, &mut prefs.lines_in_points, "pref.ui.lines_in_points");
        switch(ui, &mut prefs.text_in_points, "pref.ui.text_in_points");
        switch(ui, &mut prefs.show_brush_previews, "pref.ui.brush_previews");
        switch(ui, &mut prefs.always_brush_crosshair, "pref.ui.brush_crosshair");
        switch(ui, &mut prefs.auto_commit_filters, "pref.ui.auto_commit_filters");
        switch_with_note(ui, &mut prefs.pointer_support, "pref.ui.pointer_support", "settings.restart_needed");
        switch(ui, &mut prefs.ask_layer_name, "pref.ui.ask_layer_name");
        switch(ui, &mut prefs.white_font_dropdown, "pref.ui.white_font_dropdown");

        section(ui, "pref.ui.font_preview");
        let mut picked: Option<String> = None;
        for sample in prefs.font_preview_samples.iter() {
            if tab(ui, *sample == prefs.font_preview_text, sample.as_str()).clicked() {
                picked = Some(sample.clone());
            }
        }
        if let Some(sample) = picked {
            prefs.font_preview_text = sample;
        }
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.entry);
            if button(ui, text("pref.add")).clicked() && !self.entry.trim().is_empty() {
                prefs.font_preview_samples.push(self.entry.trim().to_string());
                self.entry.clear();
            }
            if button(ui, text("pref.remove")).clicked() && prefs.font_preview_samples.len() > 1 {
                let current = prefs.font_preview_text.clone();
                prefs.font_preview_samples.retain(|s| *s != current);
                if let Some(first) = prefs.font_preview_samples.first() {
                    prefs.font_preview_text = first.clone();
                }
            }
        });
    }

    fn show_grid_colours(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        colour_row(ui, "pref.grid.bleed", &mut prefs.bleed_colour);
        colour_row(ui, "pref.grid.column_fill", &mut prefs.column_guide_fill_colour);
        colour_row(ui, "pref.grid.margins", &mut prefs.margins_colour);
        colour_row(ui, "pref.grid.column_stroke", &mut prefs.column_guide_stroke_colour);
        colour_row(ui, "pref.grid.guides", &mut prefs.guides_colour);
        colour_row(ui, "pref.grid.baseline", &mut prefs.baseline_grid_colour);
        ui.horizontal(|ui| {
            colour_row(ui, "pref.grid.snapping", &mut prefs.snapping_grid_colour);
            slider(ui, &mut prefs.snapping_grid_opacity, 0.0..=1.0, text("pref.grid.opacity"));
        });
        ui.horizontal(|ui| {
            colour_row(ui, "pref.grid.snapping_sub", &mut prefs.snapping_grid_subdivision_colour);
            slider(ui, &mut prefs.snapping_grid_subdivision_opacity, 0.0..=1.0, text("pref.grid.opacity"));
        });
        ui.horizontal(|ui| {
            colour_row(ui, "pref.grid.pixel", &mut prefs.pixel_grid_colour);
            slider(ui, &mut prefs.pixel_grid_opacity, 0.0..=1.0, text("pref.grid.opacity"));
        });
        switch(ui, &mut prefs.grid_link_defaults, "pref.grid.link_defaults");
        ui.add_space(space("sm"));
        ui.horizontal(|ui| {
            if button(ui, text("pref.grid.reset_app")).clicked() {
                let d = Prefs::default();
                prefs.bleed_colour = d.bleed_colour;
                prefs.column_guide_fill_colour = d.column_guide_fill_colour;
                prefs.margins_colour = d.margins_colour;
                prefs.column_guide_stroke_colour = d.column_guide_stroke_colour;
                prefs.guides_colour = d.guides_colour;
                prefs.baseline_grid_colour = d.baseline_grid_colour;
                prefs.snapping_grid_colour = d.snapping_grid_colour;
                prefs.snapping_grid_subdivision_colour = d.snapping_grid_subdivision_colour;
                prefs.pixel_grid_colour = d.pixel_grid_colour;
                prefs.snapping_grid_opacity = d.snapping_grid_opacity;
                prefs.snapping_grid_subdivision_opacity = d.snapping_grid_subdivision_opacity;
                prefs.pixel_grid_opacity = d.pixel_grid_opacity;
                self.actions.push(PrefsAction::GridResetAppDefaults);
            }
            if button(ui, text("pref.grid.from_document")).clicked() {
                self.actions.push(PrefsAction::GridDefaultsFromDocument);
            }
            if button(ui, text("pref.grid.to_document")).clicked() {
                self.actions.push(PrefsAction::GridDefaultsToDocument);
            }
        });
    }

    fn show_home(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        ui.label(text("pref.home.hint"));
        let mut moved: Option<(usize, usize)> = None;
        for i in 0..prefs.home_items.len() {
            let label_key = home_item_key(&prefs.home_items[i].id);
            let row = ui
                .horizontal(|ui| {
                    ui.dnd_drag_source(egui::Id::new(("pref.home.item", i)), i, |ui| {
                        ui.label(text(label_key));
                    });
                    toggle(ui, &mut prefs.home_items[i].visible, "");
                })
                .response;
            if let Some(from) = row.dnd_release_payload::<usize>() {
                moved = Some((*from, i));
            }
        }
        if let Some((from, to)) = moved {
            let len = prefs.home_items.len();
            if from != to && from < len && to < len {
                let item = prefs.home_items.remove(from);
                prefs.home_items.insert(to, item);
            }
        }
    }

    fn show_tools(&mut self, ui: &mut Ui, prefs: &mut Prefs, settings: &mut UiSettings) {
        ui.horizontal(|ui| {
            ui.label(text("settings.tool_handle_size"));
            for (value, key) in HANDLE_SIZES {
                let selected = (settings.tool_handle_size - value).abs() < 0.01;
                if tab(ui, selected, text(key)).clicked() {
                    settings.tool_handle_size = value;
                }
            }
        });
        switch(ui, &mut prefs.shift_cycles_tools, "pref.tools.shift_cycles");
        switch(ui, &mut prefs.wheel_zoom, "pref.tools.wheel_zoom");
        switch(ui, &mut prefs.scrubby_zoom, "pref.tools.scrubby_zoom");
        switch(ui, &mut prefs.dial_support, "pref.tools.dial_support");
        switch(ui, &mut prefs.sync_tools, "pref.tools.sync_tools");
        switch(ui, &mut prefs.touch_gestures_only, "pref.tools.touch_gestures_only");
        switch(ui, &mut prefs.alt_scroll_rotate, "pref.tools.alt_scroll_rotate");
        choice(ui, "pref.tools.marquee", "pref.tools.marquee", &mut prefs.marquee_behaviour, &MARQUEE);
        choice(ui, "pref.tools.aspect", "pref.tools.aspect", &mut prefs.move_aspect_constrain, &ASPECT);
        choice(ui, "pref.tools.tablet", "pref.tools.tablet", &mut prefs.tablet_input, &TABLET);
        note(ui, "settings.restart_needed");
        num_f32(ui, "pref.tools.blend_gamma", &mut prefs.text_blend_gamma);
        num_f32(ui, "pref.tools.nudge", &mut prefs.nudge_distance);
        num_f32(ui, "pref.tools.modifier_nudge", &mut prefs.modifier_nudge_distance);
        section(ui, "pref.tools.dictionary_folder");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut prefs.dictionary_folder);
            if button(ui, text("pref.browse")).clicked() {
                self.actions.push(PrefsAction::BrowseDictionaryFolder);
            }
            if button(ui, text("pref.open")).clicked() {
                let path = if prefs.dictionary_folder.is_empty() {
                    data_dir("Dictionaries")
                } else {
                    PathBuf::from(&prefs.dictionary_folder)
                };
                open_folder(&path);
            }
        });
    }

    fn show_assistant(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        switch(ui, &mut prefs.assistant_enabled, "pref.assistant.enable");
        switch(ui, &mut prefs.assistant_auto_undo, "pref.assistant.auto_undo");
        switch(ui, &mut prefs.assistant_alert, "pref.assistant.alert");
        ui.add_space(space("sm"));
        choice(ui, "pref.assistant.paint_no_layer", "pref.assistant.paint_no_layer", &mut prefs.assist_paint_no_layer, &[ACT_NEW_PIXEL, ACT_NOTHING]);
        choice(ui, "pref.assistant.erase_vector", "pref.assistant.erase_vector", &mut prefs.assist_erase_vector, &[ACT_RASTERISE, ACT_RASTERISE_MASK, ACT_NOTHING]);
        choice(ui, "pref.assistant.brush_vector", "pref.assistant.brush_vector", &mut prefs.assist_brush_vector, &[ACT_NEW_PIXEL, ACT_RASTERISE, ACT_NOTHING]);
        choice(ui, "pref.assistant.brush_sharing", "pref.assistant.brush_sharing", &mut prefs.assist_brush_sharing, &[ACT_SHARE, ACT_NOTHING]);
        choice(ui, "pref.assistant.filter_non_pixel", "pref.assistant.filter_non_pixel", &mut prefs.assist_filter_non_pixel, &[ACT_RASTERISE, ACT_LIVE, ACT_NOTHING]);
        choice(ui, "pref.assistant.adjustment_selection", "pref.assistant.adjustment_selection", &mut prefs.assist_adjustment_selection, &[ACT_CHILD, ACT_NEW_LAYER]);
        choice(ui, "pref.assistant.mask_selection", "pref.assistant.mask_selection", &mut prefs.assist_mask_selection, &[ACT_CHILD, ACT_NEW_LAYER]);
        choice(ui, "pref.assistant.filter_layer_selection", "pref.assistant.filter_layer_selection", &mut prefs.assist_filter_layer_selection, &[ACT_CHILD, ACT_NEW_LAYER]);
        choice(ui, "pref.assistant.delete_image", "pref.assistant.delete_image", &mut prefs.assist_delete_image, &[ACT_RASTERISE, ACT_NOTHING]);
        choice(ui, "pref.assistant.duplicate_image", "pref.assistant.duplicate_image", &mut prefs.assist_duplicate_image, &[ACT_RASTERISE, ACT_NOTHING]);
        ui.add_space(space("sm"));
        if button(ui, text("pref.assistant.develop")).clicked() {
            self.actions.push(PrefsAction::DevelopAssistant);
        }
    }

    fn show_plugins(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        let folder = data_dir("Plugins");
        let shown = folder.display().to_string();
        ui.label(format("pref.plugins.default_folder", &[("path", shown.as_str())]));
        if button(ui, text("pref.plugins.open_default")).clicked() {
            open_folder(&folder);
        }
        section(ui, "pref.plugins.search_folders");
        string_list(ui, &mut prefs.plugin_folders, &mut self.entry, &mut self.selected);
        section(ui, "pref.plugins.detected");
        egui::Grid::new("pref.plugins.grid").striped(true).show(ui, |ui| {
            ui.strong(text("pref.plugins.name"));
            ui.strong(text("pref.plugins.category"));
            ui.strong(text("pref.plugins.status"));
            ui.end_row();
        });
        note(ui, "pref.plugins.none");
        switch(ui, &mut prefs.allow_unknown_plugins, "pref.plugins.allow_unknown");
        note(ui, "pref.plugins.restart_note");
    }

    fn show_linked(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        for (i, service) in prefs.linked_services.iter().enumerate() {
            if tab(ui, self.selected == Some(i), service.as_str()).clicked() {
                self.selected = Some(i);
            }
        }
        if prefs.linked_services.is_empty() {
            note(ui, "pref.linked.none");
        }
        ui.horizontal(|ui| {
            if button(ui, text("pref.linked.link")).clicked() {
                self.actions.push(PrefsAction::LinkService);
            }
            if button(ui, text("pref.linked.unlink")).clicked() {
                if let Some(i) = self.selected.take() {
                    if i < prefs.linked_services.len() {
                        prefs.linked_services.remove(i);
                    }
                }
            }
        });
    }

    fn show_autocorrect(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        switch(ui, &mut prefs.autocorrect_enabled, "pref.autocorrect.enable");
        switch(ui, &mut prefs.autocorrect_on_import, "pref.autocorrect.on_import");
        ui.add_space(space("sm"));
        switch(ui, &mut prefs.ac_caps_lock, "pref.autocorrect.caps_lock");
        switch(ui, &mut prefs.ac_capitalise_sentences, "pref.autocorrect.capitalise_sentences");
        switch(ui, &mut prefs.ac_typographic_quotes, "pref.autocorrect.typographic_quotes");
        switch(ui, &mut prefs.ac_language_single_quotes, "pref.autocorrect.language_single_quotes");
        switch(ui, &mut prefs.ac_tab_paragraph_level, "pref.autocorrect.tab_paragraph_level");
        switch(ui, &mut prefs.ac_two_initial_caps, "pref.autocorrect.two_initial_caps");
        switch(ui, &mut prefs.ac_superscript_ordinals, "pref.autocorrect.superscript_ordinals");
        switch(ui, &mut prefs.ac_bulleted_lists, "pref.autocorrect.bulleted_lists");
        switch(ui, &mut prefs.ac_numbered_lists, "pref.autocorrect.numbered_lists");
        ui.add_space(space("sm"));
        switch(ui, &mut prefs.ac_replace_while_typing, "pref.autocorrect.replace_while_typing");
        egui::Grid::new("pref.autocorrect.grid").striped(true).show(ui, |ui| {
            ui.strong(text("pref.autocorrect.replace"));
            ui.strong(text("pref.autocorrect.with"));
            ui.end_row();
            for (i, rule) in prefs.ac_replacements.iter().enumerate() {
                if tab(ui, self.selected == Some(i), rule.replace.as_str()).clicked() {
                    self.selected = Some(i);
                }
                ui.label(rule.with.as_str());
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.entry);
            ui.text_edit_singleline(&mut self.entry_with);
            if button(ui, text("pref.add")).clicked() && !self.entry.trim().is_empty() {
                prefs.ac_replacements.push(TextReplacement {
                    replace: self.entry.trim().to_string(),
                    with: self.entry_with.clone(),
                });
                self.entry.clear();
                self.entry_with.clear();
            }
            if button(ui, text("pref.remove")).clicked() {
                if let Some(i) = self.selected.take() {
                    if i < prefs.ac_replacements.len() {
                        prefs.ac_replacements.remove(i);
                    }
                }
            }
        });
    }

    fn show_abbreviations(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        if !prefs.autocorrect_enabled {
            note(ui, "pref.abbreviations.disabled");
        }
        choice(ui, "pref.abbreviations.language", "pref.language", &mut prefs.abbreviation_language, &LANGUAGES);
        string_list(ui, &mut prefs.abbreviations, &mut self.entry, &mut self.selected);
    }

    fn show_filler(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        crate::settings::filler::show(ui, prefs);
    }

    fn show_title_exceptions(&mut self, ui: &mut Ui, prefs: &mut Prefs) {
        choice(ui, "pref.title_exceptions.language", "pref.language", &mut prefs.title_exception_language, &LANGUAGES);
        string_list(ui, &mut prefs.title_exceptions, &mut self.entry, &mut self.selected);
    }

    fn show_misc(&mut self, ui: &mut Ui) {
        let rows = [
            ("pref.misc.reset_fills", PrefsAction::ResetFills),
            ("pref.misc.reset_brushes", PrefsAction::ResetBrushes),
            ("pref.misc.reset_object_styles", PrefsAction::ResetObjectStyles),
            ("pref.misc.reset_text_styles", PrefsAction::ResetTextStyles),
            ("pref.misc.reset_user_defaults", PrefsAction::ResetUserDefaults),
        ];
        for (key, action) in rows {
            ui.horizontal(|ui| {
                ui.label(text(key));
                if button(ui, text("pref.reset")).clicked() {
                    self.actions.push(action);
                }
            });
        }
    }
}
