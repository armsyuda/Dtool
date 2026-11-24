use dtool_strings::text;
use dtool_tools::dtool_text::FontLibrary;
use egui::Ui;

pub const RECENT_LIMIT: usize = 10;

#[derive(Debug, Default, Clone)]
pub struct FontComboState {
    pub recent_families: Vec<String>,
    pub filter: String,
    favorites_only: bool,
}

impl FontComboState {
    pub fn remember(&mut self, family: &str) {
        self.recent_families.retain(|known| !known.eq_ignore_ascii_case(family));
        self.recent_families.insert(0, family.to_string());
        self.recent_families.truncate(RECENT_LIMIT);
    }

    pub fn matches(&self, family: &str) -> bool {
        let filter = self.filter.trim().to_lowercase();
        filter.is_empty() || family.to_lowercase().contains(&filter)
    }

    pub fn visible_families<'a>(&self, library: &'a FontLibrary) -> Vec<&'a str> {
        library.families().into_iter().filter(|family| self.matches(family)).collect()
    }
}

pub fn style_names(library: &FontLibrary, family: &str) -> Vec<String> {
    let mut styles: Vec<String> = library.faces(family).into_iter().map(|face| face.style.clone()).collect();
    styles.dedup();
    styles
}

pub fn font_combo(ui: &mut Ui, id: &str, state: &mut FontComboState, library: &FontLibrary, selected: &str) -> Option<String> {
    family_menu(ui, id, state, &library.families(), selected).0
}

fn favorites_path() -> std::path::PathBuf {
    dtool_core::prefs::Prefs::default_path().with_file_name("font_favorites.json")
}

fn read_favorites(path: &std::path::Path) -> Vec<String> {
    std::fs::read(path).ok().and_then(|body| serde_json::from_slice(&body).ok()).unwrap_or_default()
}

pub(crate) fn family_menu(ui: &mut Ui, id: &str, state: &mut FontComboState, families: &[&str], selected: &str) -> (Option<String>, Option<Option<String>>) {
    let mut picked = None;
    let popup_width = (ui.available_width() - 14.0).clamp(95.0, 196.0);
    let popup = egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .truncate()
        .height(320.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(popup_width);
            let cache_key = egui::Id::new("dtool.font.favorites");
            let path = favorites_path();
            let favorites = ui.data_mut(|data| data.get_temp_mut_or_insert_with(cache_key, || read_favorites(&path)).clone());
            let is_favorite = |family: &str| favorites.iter().any(|entry| entry.eq_ignore_ascii_case(family));
            let previously_favorites_only = state.favorites_only;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.favorites_only, false, text("font.all"));
                ui.selectable_value(&mut state.favorites_only, true, text("font.favorites"));
            });
            let search = ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text(text("font.search")));
            let mut hovered = None;
            let matches = |family: &&str| state.matches(family) && (!state.favorites_only || is_favorite(family));
            let recent: Vec<_> = state.recent_families.iter().map(String::as_str).filter(matches).collect();
            let show_recent = !state.favorites_only && !recent.is_empty();
            let mut rows = Vec::new();
            if show_recent {
                rows.push(None);
                rows.extend(recent.into_iter().map(Some));
            }
            rows.extend(families.iter().copied().filter(matches).map(Some));
            let mut toggle = None;
            let height = ui.spacing().interact_size.y;
            let mut scroll = egui::ScrollArea::vertical().id_salt("font.families")
                .max_height(244.0).auto_shrink([false, true]);
            if search.changed() || previously_favorites_only != state.favorites_only {
                scroll = scroll.vertical_scroll_offset(0.0);
            }
            scroll.show_rows(ui, height, rows.len(), |ui, visible| {
                for index in visible {
                    let Some(family) = rows[index] else {
                        ui.add_sized([popup_width, height], egui::Label::new(text("font.recent")));
                        continue;
                    };
                    let favorite = is_favorite(family);
                    let action = text(if favorite { "font.favorite.remove" } else { "font.favorite.add" });
                    ui.push_id(index, |ui| ui.horizontal(|ui| {
                        let family_width = (popup_width - 20.0 - 18.0 - ui.spacing().item_spacing.x).max(24.0);
                        let row = ui.add_sized([family_width, height], egui::Button::selectable(family == selected, family).truncate());
                        if row.hovered() { hovered = Some(family.to_owned()); }
                        if row.clicked() { picked = Some(family.to_owned()); ui.close(); }
                        let label = format!("{family} · {action}");
                        let star = ui.add_sized([20.0, height], egui::Button::new(if favorite { "★" } else { "☆" }).frame(false)).on_hover_text(&label);
                        star.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), favorite, &label));
                        if star.clicked() { toggle = Some((family, !favorite)); }
                    }));
                }
            });
            if rows.is_empty() {
                ui.label(text(if state.favorites_only { "font.favorites.empty" } else { "font.search.empty" }));
            }
            let error_key = ui.make_persistent_id("font.favorites.save_error");
            if let Some((family, add)) = toggle {
                let mut updated = read_favorites(&path);
                updated.retain(|entry| !entry.eq_ignore_ascii_case(family));
                if add { updated.push(family.to_owned()); }
                let saved = path.parent().is_none_or(|parent| parent.as_os_str().is_empty() || std::fs::create_dir_all(parent).is_ok())
                    && serde_json::to_vec(&updated).is_ok_and(|body| dtool_core::native_io::write_atomic(&path, &body).is_ok());
                ui.data_mut(|data| {
                    data.insert_temp(error_key, !saved);
                    if saved { data.insert_temp(cache_key, updated); }
                });
            }
            if ui.data(|data| data.get_temp::<bool>(error_key).unwrap_or(false)) { ui.label(text("font.favorites.save_error")); }
            hovered
        });
    if let Some(family) = &picked {
        state.remember(family);
        state.filter.clear();
    }
    popup.response.on_hover_text(selected);
    (picked, popup.inner)
}

pub fn style_combo(ui: &mut Ui, id: &str, library: &FontLibrary, family: &str, style: &mut String) -> bool {
    let styles = style_names(library, family);
    ui.add_enabled_ui(!styles.is_empty(), |ui| {
        let mut changed = false;
        let selected_style = style.clone();
        egui::ComboBox::from_id_salt(id).selected_text(&selected_style).truncate().show_ui(ui, |ui| {
            for name in &styles {
                if dtool_design::list_row(ui, name == style, name.as_str()).clicked() {
                    changed = name != style;
                    *style = name.clone();
                    ui.close();
                }
            }
        }).response.on_hover_text(selected_style);
        changed
    }).inner
}

fn face_patch(face: &dtool_tools::dtool_text::FontFace) -> serde_json::Value {
    serde_json::json!({ "style": face.style, "weight": face.weight, "italic": face.italic })
}

pub(crate) fn family_patch(library: &FontLibrary, family: &str) -> serde_json::Value {
    let face = library.resolve(family, "Regular")
        .or_else(|| library.resolve_nearest(family, 400, false))
        .and_then(|id| library.face(id));
    let mut patch = face.map_or_else(
        || serde_json::json!({ "style": "Regular", "weight": 400, "italic": false }),
        face_patch,
    );
    patch["family"] = serde_json::json!(family);
    patch
}

pub(crate) fn style_patch(library: &FontLibrary, family: &str, style: &str) -> Option<serde_json::Value> {
    Some(face_patch(library.face(library.resolve(family, style)?)?))
}
