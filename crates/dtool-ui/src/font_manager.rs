use dtool_core::command::Session;
use dtool_core::object::ObjectId;
use dtool_core::text_cmd::FontFaceName;
use dtool_design::{space, ControlState};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::{json, Value};

pub struct FontManagerState {
    pub open: bool,
    pub usages: Vec<FontUsageRow>,
    pub selected_index: Option<usize>,
    generation: Option<u64>,
    substitute_combo: crate::font_combo::FontComboState,
    sort_by_status: bool,
    sort_descending: bool,
    last_located: Option<(FontFaceName, ObjectId)>,
}

pub struct FontUsageRow {
    pub family: String,
    pub style: String,
    pub status: String,
    pub missing: bool,
    pub unsupported_chars: Vec<char>,
    pub substitute_unsupported_chars: Vec<char>,
    pub substitute_family: Option<String>,
    pub substitute_style: Option<String>,
}

impl FontManagerState {
    pub fn new() -> Self {
        Self {
            open: false,
            usages: Vec::new(),
            selected_index: None,
            generation: None,
            substitute_combo: Default::default(),
            sort_by_status: false,
            sort_descending: false,
            last_located: None,
        }
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
        self.selected_index = None;
        self.last_located = None;
    }

    pub fn set_usages(&mut self, usages: Vec<FontUsageRow>) {
        self.usages = usages;
        self.selected_index = None;
    }

    pub fn has_selection(&self) -> bool {
        self.selected_index.is_some()
    }

    pub fn selected_row(&self) -> Option<&FontUsageRow> {
        self.selected_index.and_then(|index| self.usages.get(index))
    }

    pub fn load_usages(&mut self, session: &mut Session) -> Result<(), CommandError> {
        let value = session.run(&Command::new("font.usage.query"))?;
        self.set_usages(rows_from_query(&value));
        self.generation = Some(session.generation);
        Ok(())
    }

    pub fn refresh_usages(&mut self, session: &mut Session) -> Result<(), CommandError> {
        if self.generation == Some(session.generation) {
            return Ok(());
        }
        let selected = self.selected_row().map(|row| (row.family.clone(), row.style.clone()));
        self.load_usages(session)?;
        self.selected_index = selected.and_then(|(family, style)| {
            self.usages.iter().position(|row| row.family == family && row.style == style)
        });
        Ok(())
    }

    pub fn open_for(&mut self, session: &mut Session) -> Result<(), CommandError> {
        self.load_usages(session)?;
        self.last_located = None;
        self.show();
        Ok(())
    }

    pub fn begin_replace(&self, replace: &mut ReplaceFontState) -> bool {
        let Some(row) = self.selected_row() else {
            return false;
        };
        replace.from_family = row.family.clone();
        replace.from_style = row.style.clone();
        replace.family.clear();
        replace.style.clear();
        replace.show();
        true
    }

    pub fn confirm_replace(&mut self, session: &mut Session, replace: &mut ReplaceFontState) -> Result<(), CommandError> {
        if !replace.open || replace.from_family.is_empty() || replace.family.trim().is_empty() {
            return Err(CommandError::invalid_params());
        }
        session.run(&Command::with_params(
            "font.replace",
            json!({
                "from_family": replace.from_family,
                "from_style": replace.from_style,
                "to_family": replace.family.trim(),
                "to_style": replace.style.trim(),
            }),
        ))?;
        replace.hide();
        self.load_usages(session)
    }

    pub fn cancel_replace(&self, replace: &mut ReplaceFontState) {
        replace.hide();
    }

    pub fn locate_selected(&mut self, session: &mut Session) -> Result<Vec<ObjectId>, CommandError> {
        let Some(row) = self.selected_row() else {
            return Err(CommandError::invalid_params());
        };
        let face = FontFaceName { family: row.family.clone(), style: row.style.clone() };
        let mut ids = objects_using_font(&session.document, &face.family, &face.style);
        ids.sort_by_key(|id| dtool_core::data_merge::layout::object_page_index(&session.document, *id));
        let Some(&first) = ids.first() else {
            self.last_located = None;
            return Err(CommandError::not_found());
        };
        let next = self.last_located.as_ref().filter(|(previous, _)| previous == &face)
            .and_then(|(_, previous)| ids.iter().position(|id| id == previous))
            .and_then(|index| ids.get(index + 1)).copied().unwrap_or(first);
        if session.selection.ids.as_slice() != [next] {
            session.run(&Command::with_params("selection.set", json!({ "ids": [next.0], "op": "Replace" })))?;
        }
        self.last_located = Some((face, next));
        Ok(vec![next])
    }
}

pub struct ReplaceFontState {
    pub open: bool,
    pub from_family: String,
    pub from_style: String,
    pub family: String,
    pub style: String,
    pub classification_filter: String,
    pub combo: crate::font_combo::FontComboState,
}

impl ReplaceFontState {
    pub fn new() -> Self {
        Self {
            open: false,
            from_family: String::new(),
            from_style: String::new(),
            family: String::new(),
            style: String::new(),
            classification_filter: String::new(),
            combo: Default::default(),
        }
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
    }
}

pub fn rows_from_query(value: &Value) -> Vec<FontUsageRow> {
    value
        .get("usages")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let family = item.get("family").and_then(|v| v.as_str())?.to_string();
            if family.is_empty() {
                return None;
            }
            let style = item.get("style").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let missing = item.get("missing").and_then(|v| v.as_bool()).unwrap_or(false);
            let unsupported = |key: &str| item[key].as_array().into_iter().flatten()
                .filter_map(|value| value.as_str()?.chars().next()).collect::<Vec<_>>();
            let unsupported_chars = unsupported("unsupported_chars");
            let substitute_unsupported_chars = unsupported("substitute_unsupported_chars");
            let (substitute_family, substitute_style) = match item.get("substitute") {
                Some(Value::String(name)) if !name.is_empty() => (Some(name.clone()), None),
                Some(Value::Object(map)) => (
                    map.get("family").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string()),
                    map.get("style").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string()),
                ),
                _ => (None, None),
            };
            Some(FontUsageRow {
                family,
                style,
                status: text(if missing { "fontmgr.status.missing" }
                    else if unsupported_chars.is_empty() { "fontmgr.status.ok" }
                    else { "fontmgr.status.unsupported" }).to_string(),
                missing,
                unsupported_chars,
                substitute_unsupported_chars,
                substitute_family,
                substitute_style,
            })
        })
        .collect()
}

pub fn objects_using_font(doc: &dtool_core::document::Document, family: &str, style: &str) -> Vec<ObjectId> {
    use dtool_core::object::ObjectKind;
    let mut matching = std::collections::HashSet::new();
    let mut frames = std::collections::HashSet::new();
    for story in &doc.stories {
        let spans: Vec<_> = story.ranges.iter().filter(|range| {
            range["frame"].is_null()
                && range["family"].as_str().is_some_and(|name| name.eq_ignore_ascii_case(family))
                && range["style"].as_str().unwrap_or("").eq_ignore_ascii_case(style)
        }).filter_map(|range| Some((range["start"].as_u64()? as usize, range["end"].as_u64()? as usize)))
            .filter(|(start, end)| start < end).collect();
        if spans.is_empty() { continue; }
        matching.insert(story.id);
        for range in dtool_core::longdoc::pages::flow_ranges(doc, story.id) {
            if range.start < range.end && spans.iter().any(|&(start, end)| start < range.end && range.start < end) {
                frames.insert(range.frame);
            }
        }
    }
    doc.objects
        .iter()
        .filter_map(|object| {
            let story = object.story_id()?;
            let uses_font = if matches!(object.kind, ObjectKind::FrameText { .. }) {
                frames.contains(&object.id)
            } else {
                matching.contains(&story)
            };
            uses_font.then_some(object.id)
        })
        .collect()
}

pub enum FontManagerAction {
    Locate,
    OpenReplace,
    Substitute { source: FontFaceName, target: Option<FontFaceName> },
    Close,
    ConfirmReplace,
    CancelReplace,
}

pub fn render_font_manager(
    ui: &mut egui::Ui,
    state: &mut FontManagerState,
    replace_state: &mut ReplaceFontState,
) -> Option<FontManagerAction> {
    if !state.open {
        return None;
    }
    let mut action = None;
    let mut window_open = true;
    egui::Window::new(text("fontmgr.title"))
        .min_width(640.0)
        .min_height(300.0)
        .resizable(true)
        .open(&mut window_open)
        .show(ui.ctx(), |ui| {
            action = render_font_manager_content(ui, state, replace_state);
        });
    if !window_open {
        return Some(FontManagerAction::Close);
    }
    action
}

fn render_font_manager_content(
    ui: &mut egui::Ui,
    state: &mut FontManagerState,
    replace_state: &mut ReplaceFontState,
) -> Option<FontManagerAction> {
    let mut action = None;
    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
        egui::Grid::new("font_manager.rows").num_columns(5).striped(true).show(ui, |ui| {
            for (label, sortable) in [("fontmgr.name", Some(false)), ("fontmgr.style", None),
                ("fontmgr.status", Some(true)), ("fontmgr.subst", None), ("fontmgr.subst_style", None)] {
                if let Some(by_status) = sortable {
                    let active = state.sort_by_status == by_status;
                    let caption = if active {
                        format!("{} {}", text(label), if state.sort_descending { "▼" } else { "▲" })
                    } else {
                        text(label).to_owned()
                    };
                    if ui.button(caption).clicked() {
                        state.sort_by_status = by_status;
                        state.sort_descending = active && !state.sort_descending;
                    }
                } else {
                    ui.strong(text(label));
                }
            }
            ui.end_row();
            let mut rows: Vec<_> = state.usages.iter().enumerate().collect();
            rows.sort_by(|(_, left), (_, right)| {
                let category = if state.sort_by_status { left.status.cmp(&right.status) } else { std::cmp::Ordering::Equal };
                let order = category.then_with(|| left.family.cmp(&right.family)).then_with(|| left.style.cmp(&right.style));
                if state.sort_descending { order.reverse() } else { order }
            });
            for (idx, row) in rows {
                let selected = state.selected_index == Some(idx);
                let name = ui.selectable_label(selected, &row.family);
                let style = ui.selectable_label(selected, &row.style);
                if (name.clicked() || style.clicked()) && !selected {
                    state.selected_index = Some(idx);
                    state.last_located = None;
                }
                let (characters, message) = if row.missing {
                    (&row.substitute_unsupported_chars, text("fontmgr.status.substitute_unsupported"))
                } else {
                    (&row.unsupported_chars, text("fontmgr.status.unsupported"))
                };
                if characters.is_empty() {
                    ui.label(&row.status);
                } else {
                    let mut details = characters.iter().take(16).map(|ch| format!("{ch} (U+{:04X})", *ch as u32)).collect::<Vec<_>>().join(", ");
                    if characters.len() > 16 { details.push_str(", …"); }
                    let caption = if row.missing { format!("{} · {} ({})", row.status, message, characters.len()) }
                        else { format!("{} ({})", message, characters.len()) };
                    ui.label(egui::RichText::new(caption).color(ui.visuals().warn_fg_color)).on_hover_text(details);
                }
                let family = row.substitute_family.as_deref().unwrap_or("");
                let mut face_style = row.substitute_style.clone().unwrap_or_default();
                if row.missing {
                    let library = dtool_tools::dtool_text::FontLibrary::shared();
                    let source = || FontFaceName { family: row.family.clone(), style: row.style.clone() };
                    ui.push_id(("substitute_family", idx), |ui| {
                        if let Some(family) = crate::font_combo::font_combo(ui, "family", &mut state.substitute_combo, &library, family) {
                            let style = library.resolve(&family, "Regular")
                                .or_else(|| library.resolve_nearest(&family, 400, false))
                                .and_then(|id| library.face(id)).map(|face| face.style.clone());
                            if let Some(style) = style {
                                action = Some(FontManagerAction::Substitute {
                                    source: source(), target: Some(FontFaceName { family, style }),
                                });
                                state.selected_index = Some(idx);
                            }
                        }
                    });
                    ui.push_id(("substitute_style", idx), |ui| {
                        if crate::font_combo::style_combo(ui, "style", &library, family, &mut face_style) {
                            action = Some(FontManagerAction::Substitute {
                                source: source(), target: Some(FontFaceName { family: family.to_owned(), style: face_style }),
                            });
                            state.selected_index = Some(idx);
                        }
                    });
                } else {
                    ui.label(family);
                    ui.label(face_style);
                }
                ui.end_row();
            }
        });
    });
    ui.add_space(space("md"));
    ui.horizontal(|ui| {
        let selected = state.has_selection();
        let locate_state = ControlState::enabled(selected);
        if dtool_design::button_with(ui, text("fontmgr.locate"), &locate_state).clicked() {
            action = Some(FontManagerAction::Locate);
        }
        let replace_ctrl = ControlState::enabled(selected);
        if dtool_design::button_with(ui, text("fontmgr.replace"), &replace_ctrl).clicked() {
            action = Some(FontManagerAction::OpenReplace);
        }
        if let Some(row) = state.selected_row().filter(|row| row.substitute_family.is_some()) {
            if dtool_design::button(ui, text("fontmgr.subst.clear")).clicked() {
                action = Some(FontManagerAction::Substitute {
                    source: FontFaceName { family: row.family.clone(), style: row.style.clone() }, target: None,
                });
            }
        }
        if dtool_design::button(ui, text("common.close")).clicked() {
            action = Some(FontManagerAction::Close);
        }
    });
    if replace_state.open {
        if let Some(replace_action) = render_replace_font_dialog(ui, replace_state) {
            action = Some(replace_action);
        }
    }
    action
}

fn render_replace_font_dialog(ui: &mut egui::Ui, state: &mut ReplaceFontState) -> Option<FontManagerAction> {
    let mut action = None;
    let mut window_open = true;
    egui::Window::new(text("fontmgr.replace"))
        .resizable(true)
        .open(&mut window_open)
        .show(ui.ctx(), |ui| {
            let library = dtool_tools::dtool_text::FontLibrary::shared();
            ui.horizontal(|ui| {
                ui.label(text("fontmgr.name"));
                if let Some(family) = crate::font_combo::font_combo(ui, "fontmgr.replace.family", &mut state.combo, &library, &state.family) {
                    state.family = family;
                    state.style = crate::font_combo::style_names(&library, &state.family).into_iter().next().unwrap_or_default();
                }
            });
            ui.add_space(space("sm"));
            ui.horizontal(|ui| {
                ui.label(text("fontmgr.subst_style"));
                crate::font_combo::style_combo(ui, "fontmgr.replace.style", &library, &state.family, &mut state.style);
            });
            ui.add_space(space("md"));
            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("button.ok")).clicked() {
                    action = Some(FontManagerAction::ConfirmReplace);
                }
                if dtool_design::button(ui, text("button.cancel")).clicked() {
                    action = Some(FontManagerAction::CancelReplace);
                }
            });
        });
    if !window_open {
        return Some(FontManagerAction::CancelReplace);
    }
    action
}
