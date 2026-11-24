use crate::dialogs::edit_text_style::{EditAction, EditTextStyleDialog};
use crate::panels::character;
use dtool_core::text_styles::StyleKind;
use dtool_core::{Command, CommandError, Session, Workspace};
use dtool_design::{button, panel_frame, panel_header, space};
use dtool_protocol::CommandResult;
use dtool_strings::text;
use serde_json::{json, Map, Value};

pub const TEXT_STYLES_PANEL_KEY: &str = "text_styles";
pub const WINDOW_TEXT_STYLES: &str = "window.text.text_styles";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    ByType,
    ByName,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleRow {
    pub name: String,
    pub kind: StyleKind,
    pub depth: usize,
    pub based_on: Option<String>,
    pub current: bool,
    pub overridden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    Select { name: String, kind: StyleKind },
    Apply { name: String, kind: StyleKind, keep_overrides: bool },
    ApplyToParagraphs { name: String },
    Update { name: String, kind: StyleKind },
    Edit { name: String, kind: StyleKind },
    Duplicate { name: String, kind: StyleKind },
    SetBasedOn { name: String, kind: StyleKind, based_on: Option<String> },
    CreateBasedOn { name: String, kind: StyleKind },
    Delete { name: String, kind: StyleKind },
    Reapply,
    ReapplyBase,
    Create { kind: StyleKind },
    ClearCharacterStyles,
    SetNoStyle { kind: StyleKind },
    DeleteUnused,
    Detach { name: String, kind: StyleKind },
    Import { path: String, conflict: String },
    Menu(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextStylesPanel {
    pub sort: SortMode,
    pub selected: Option<(StyleKind, String)>,
    pub editor: Option<EditTextStyleDialog>,
    pub error: Option<String>,
}

fn kind_label(kind: StyleKind) -> &'static str {
    match kind {
        StyleKind::Character => "styles.kind.character",
        StyleKind::Paragraph => "styles.kind.paragraph",
    }
}

fn listed(session: &Session) -> Vec<(StyleKind, String, Option<String>, Map<String, Value>)> {
    session
        .document
        .text_styles
        .styles
        .iter()
        .map(|style| (style.kind, style.name.clone(), style.based_on.clone(), style.props.clone()))
        .collect()
}

fn unique_name(session: &Session, kind: StyleKind, base: &str) -> String {
    let store = &session.document.text_styles;
    if store.get(kind, base).is_none() {
        return base.to_string();
    }
    for index in 2..1000 {
        let candidate = format!("{base} {index}");
        if store.get(kind, &candidate).is_none() {
            return candidate;
        }
    }
    format!("{base} copy")
}

fn current_applied(session: &Session, kind: StyleKind) -> (Option<String>, bool) {
    let Some((_, start, end)) = character::active_range(session) else {
        return (None, false);
    };
    let Some(selection) = character::selection_at(session, session.active_story_id().map(|id| id.0).unwrap_or(0), start, end) else {
        return (None, false);
    };
    let key = dtool_core::text_styles::style_key(kind);
    let mut names = selection.segments.iter().map(|segment| segment.style.get(key).and_then(Value::as_str).map(str::to_string));
    let first = names.next().flatten();
    let same = names.all(|name| name == first);
    let overridden = selection.segments.iter().any(|segment| segment.style.get(dtool_core::text_styles::STYLE_OVERRIDES_KEY).and_then(Value::as_object).is_some_and(|map| !map.is_empty()));
    (if same { first } else { None }, overridden)
}

fn range_params(session: &Session) -> Option<Value> {
    range_params_kind(session, None)
}

fn range_params_kind(session: &Session, kind: Option<StyleKind>) -> Option<Value> {
    let (story, start, end) = character::active_range(session)?;
    let mut start = start;
    let mut end = end.max(start);
    if start == end && kind == Some(StyleKind::Character) {
        end = session.document.stories.iter().find(|item| item.id.0 == story)?.text.chars().count();
        start = 0;
    }
    Some(json!({ "story": story, "range": { "start": start, "end": end } }))
}

fn merge_params(base: Value, extra: Value) -> Value {
    let mut map = base.as_object().cloned().unwrap_or_default();
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            map.insert(key.clone(), value.clone());
        }
    }
    Value::Object(map)
}

fn visit(styles: &[(StyleKind, String, Option<String>, Map<String, Value>)], kind: StyleKind, parent: Option<&str>, depth: usize, seen: &mut Vec<String>, rows: &mut Vec<(StyleKind, String, usize, Option<String>)>) {
    let mut children: Vec<&(StyleKind, String, Option<String>, Map<String, Value>)> = styles
        .iter()
        .filter(|style| style.0 == kind && style.2.as_deref() == parent && !seen.contains(&style.1))
        .collect();
    children.sort_by(|a, b| a.1.cmp(&b.1));
    for child in children {
        seen.push(child.1.clone());
        rows.push((child.0, child.1.clone(), depth, child.2.clone()));
        visit(styles, kind, Some(&child.1), depth + 1, seen, rows);
    }
}

impl TextStylesPanel {
    pub fn rows(&self, session: &Session) -> Vec<StyleRow> {
        let styles = listed(session);
        let mut ordered: Vec<(StyleKind, String, usize, Option<String>)> = Vec::new();
        let kinds = match self.sort {
            SortMode::ByType => vec![StyleKind::Paragraph, StyleKind::Character],
            SortMode::ByName => {
                let mut kinds: Vec<StyleKind> = styles.iter().map(|style| style.0).collect();
                kinds.sort_by_key(|kind| match kind {
                    StyleKind::Paragraph => 0,
                    StyleKind::Character => 1,
                });
                kinds.dedup();
                kinds
            }
        };
        for kind in kinds {
            let mut seen = Vec::new();
            visit(&styles, kind, None, 0, &mut seen, &mut ordered);
            let mut orphans: Vec<_> = styles.iter().filter(|style| style.0 == kind && !seen.contains(&style.1)).cloned().collect();
            orphans.sort_by(|a, b| a.1.cmp(&b.1));
            for style in orphans {
                ordered.push((style.0, style.1, 0, style.2));
            }
        }
        if self.sort == SortMode::ByName {
            ordered.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0))));
        }
        let (para, para_over) = current_applied(session, StyleKind::Paragraph);
        let (char_style, char_over) = current_applied(session, StyleKind::Character);
        ordered
            .into_iter()
            .map(|(kind, name, depth, based_on)| {
                let (current, overridden) = match kind {
                    StyleKind::Paragraph => (para.as_deref() == Some(name.as_str()), para_over),
                    StyleKind::Character => (char_style.as_deref() == Some(name.as_str()), char_over),
                };
                StyleRow { name, kind, depth, based_on, current, overridden }
            })
            .collect()
    }

    pub fn command(&self, session: &Session, action: &PanelAction) -> Option<Command> {
        let range = range_params(session);
        match action {
            PanelAction::Select { .. } | PanelAction::Edit { .. } | PanelAction::Menu(_) => None,
            PanelAction::Apply { name, kind, keep_overrides } => {
                let params = merge_params(range_params_kind(session, Some(*kind))?, json!({ "id": name, "kind": kind, "keep_overrides": keep_overrides }));
                Some(Command::with_params("text.style.apply_named", params))
            }
            PanelAction::ApplyToParagraphs { name } => {
                let params = merge_params(range?, json!({ "id": name, "kind": StyleKind::Paragraph, "keep_overrides": false }));
                Some(Command::with_params("text.style.apply_named", params))
            }
            PanelAction::Update { name, kind } => {
                let params = merge_params(range?, json!({ "id": name, "kind": kind }));
                Some(Command::with_params("text.style.define_from_selection", params))
            }
            PanelAction::Duplicate { name, kind } => {
                let style = session.document.text_styles.get(*kind, name)?;
                let copy = unique_name(session, *kind, &format!("{name} copy"));
                Some(Command::with_params(
                    "text.style.define",
                    json!({ "id": copy, "kind": kind, "based_on": style.based_on, "next_style": style.next_style, "props": style.props }),
                ))
            }
            PanelAction::SetBasedOn { name, kind, based_on } => {
                let style = session.document.text_styles.get(*kind, name)?;
                Some(Command::with_params(
                    "text.style.define",
                    json!({ "id": name, "kind": kind, "based_on": based_on, "props": style.props }),
                ))
            }
            PanelAction::CreateBasedOn { name, kind } => {
                let range = range?;
                let new_name = unique_name(session, *kind, name);
                let params = merge_params(range, json!({ "id": new_name, "kind": kind, "based_on": name }));
                Some(Command::with_params("text.style.define_from_selection", params))
            }
            PanelAction::Delete { name, kind } => Some(Command::with_params("text.style.remove", json!({ "id": name, "kind": kind }))),
            PanelAction::Reapply => {
                let params = merge_params(range_params_kind(session, Some(StyleKind::Character))?, json!({ "kind": StyleKind::Character }));
                Some(Command::with_params("text.style.clear_override", params))
            }
            PanelAction::ReapplyBase => {
                let params = merge_params(range?, json!({ "kind": StyleKind::Paragraph }));
                Some(Command::with_params("text.style.clear_override", params))
            }
            PanelAction::Create { kind } => {
                let name = unique_name(session, *kind, match kind {
                    StyleKind::Paragraph => "Paragraph style",
                    StyleKind::Character => "Character style",
                });
                let params = merge_params(range?, json!({ "id": name, "kind": kind }));
                Some(Command::with_params("text.style.define_from_selection", params))
            }
            PanelAction::ClearCharacterStyles => {
                let params = merge_params(range_params_kind(session, Some(StyleKind::Character))?, json!({ "kind": StyleKind::Character }));
                Some(Command::with_params("text.style.set_no_style", params))
            }
            PanelAction::SetNoStyle { kind } => {
                let params = merge_params(range_params_kind(session, Some(*kind))?, json!({ "kind": kind }));
                Some(Command::with_params("text.style.set_no_style", params))
            }
            PanelAction::DeleteUnused => Some(Command::with_params("text.style.delete_unused", json!({}))),
            PanelAction::Detach { name, kind } => Some(Command::with_params("text.style.detach", json!({ "id": name, "kind": kind }))),
            PanelAction::Import { path, conflict } => {
                if !session.catalog.contains("text.style.import") || path.trim().is_empty() {
                    return None;
                }
                Some(Command::with_params("text.style.import", json!({ "path": path, "conflict": conflict })))
            }
        }
    }

    pub fn run(&mut self, session: &mut Session, action: PanelAction) -> CommandResult {
        match &action {
            PanelAction::Select { name, kind } => {
                self.selected = Some((*kind, name.clone()));
                return Ok(json!({ "selected": name, "kind": kind }));
            }
            PanelAction::Edit { name, kind } => {
                self.selected = Some((*kind, name.clone()));
                self.editor = session.document.text_styles.get(*kind, name).map(|style| EditTextStyleDialog::from_style(style));
                if self.editor.is_none() {
                    return Err(CommandError::not_found());
                }
                return Ok(json!({ "edit": name, "kind": kind }));
            }
            PanelAction::Menu(id) => {
                if let Some(mapped) = menu_action(id, self.selected.as_ref()) {
                    return self.run(session, mapped);
                }
                return Ok(json!({ "menu": id, "handled": false }));
            }
            _ => {}
        }
        if let PanelAction::Import { path, .. } = &action {
            if !session.catalog.contains("text.style.import") {
                return Err(CommandError { code: "NOT_IMPLEMENTED".into(), message_key: "styles.import.unavailable".into() });
            }
            if path.trim().is_empty() {
                return Err(CommandError { code: "INVALID_PARAMS".into(), message_key: "styles.import.need_path".into() });
            }
        }
        let Some(command) = self.command(session, &action) else {
            return Err(CommandError::not_implemented());
        };
        let result = session.run(&command);
        if result.is_ok() {
            if let PanelAction::Duplicate { kind, .. } | PanelAction::Create { kind } | PanelAction::CreateBasedOn { kind, .. } = &action {
                if let Some(name) = session.document.text_styles.names(*kind).into_iter().last() {
                    self.selected = Some((*kind, name));
                }
            }
            if let PanelAction::Delete { name, kind } = &action {
                if self.selected.as_ref() == Some(&(*kind, name.clone())) {
                    self.selected = None;
                }
            }
        }
        result
    }

    pub fn finish_edit(&mut self, session: &mut Session, action: EditAction) -> CommandResult {
        let Some(editor) = self.editor.as_ref() else {
            return Err(CommandError::invalid_params());
        };
        match action {
            EditAction::Cancel => {
                self.editor = None;
                Ok(json!({ "cancelled": true }))
            }
            EditAction::Ok | EditAction::ApplyToSelection => {
                let command = editor.define_command();
                let result = session.run(&command);
                if result.is_ok() && (action == EditAction::ApplyToSelection || editor.apply_to_selection) {
                    if let Some(apply) = self.command(session, &PanelAction::Apply { name: editor.name.clone(), kind: editor.kind, keep_overrides: false }) {
                        let applied = session.run(&apply);
                        if applied.is_err() {
                            return applied;
                        }
                    }
                }
                if result.is_ok() {
                    self.selected = Some((editor.kind, editor.name.clone()));
                    self.editor = None;
                }
                result
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let mut pending = None;
        let mut edit_action = None;
        panel_frame(ui, |ui| {
            ui.horizontal(|ui| {
                panel_header(ui, text("styles.panel"));
                ui.menu_button("☰", |ui| {
                    for (id, key) in PANEL_MENU {
                        if ui.button(text(key)).clicked() {
                            pending = Some(PanelAction::Menu(id.to_string()));
                            ui.close();
                        }
                    }
                });
                let mut by_type = self.sort == SortMode::ByType;
                if ui.checkbox(&mut by_type, text("styles.sort_by_type")).changed() {
                    self.sort = if by_type { SortMode::ByType } else { SortMode::ByName };
                }
            });
            let Some(session) = workspace.session_mut() else {
                ui.label(text("styles.no_document"));
                return;
            };
            let rows = self.rows(session);
            let mut last_kind = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for row in &rows {
                    if self.sort == SortMode::ByType && last_kind != Some(row.kind) {
                        ui.label(text(kind_label(row.kind)));
                        last_kind = Some(row.kind);
                    }
                    let selected = self.selected.as_ref() == Some(&(row.kind, row.name.clone()));
                    let mut label = format!("{}{}", "    ".repeat(row.depth), row.name);
                    if row.current {
                        label.push_str(" •");
                    }
                    if row.overridden && row.current {
                        label.push('+');
                    }
                    let response = ui.selectable_label(selected, label);
                    if response.clicked() {
                        pending = Some(PanelAction::Select { name: row.name.clone(), kind: row.kind });
                    }
                    if response.double_clicked() {
                        pending = Some(PanelAction::Apply { name: row.name.clone(), kind: row.kind, keep_overrides: false });
                    }
                    response.context_menu(|ui| {
                        if ui.button(text("styles.apply")).clicked() {
                            pending = Some(PanelAction::Apply { name: row.name.clone(), kind: row.kind, keep_overrides: false });
                            ui.close();
                        }
                        if ui.button(text("styles.apply_preserve_local")).clicked() {
                            pending = Some(PanelAction::Apply { name: row.name.clone(), kind: row.kind, keep_overrides: true });
                            ui.close();
                        }
                        if row.kind == StyleKind::Paragraph && ui.button(text("styles.apply_to_paragraphs")).clicked() {
                            pending = Some(PanelAction::ApplyToParagraphs { name: row.name.clone() });
                            ui.close();
                        }
                        if ui.button(text("styles.update")).clicked() {
                            pending = Some(PanelAction::Update { name: row.name.clone(), kind: row.kind });
                            ui.close();
                        }
                        if ui.button(dtool_strings::format("styles.edit", &[("name", row.name.as_str())])).clicked() {
                            pending = Some(PanelAction::Edit { name: row.name.clone(), kind: row.kind });
                            ui.close();
                        }
                        if ui.button(text("styles.duplicate")).clicked() {
                            pending = Some(PanelAction::Duplicate { name: row.name.clone(), kind: row.kind });
                            ui.close();
                        }
                        ui.menu_button(text("styles.based_on"), |ui| {
                            if ui.button(text("styles.based_on.none")).clicked() {
                                pending = Some(PanelAction::SetBasedOn { name: row.name.clone(), kind: row.kind, based_on: None });
                                ui.close();
                            }
                            for parent in session.document.text_styles.names(row.kind) {
                                if parent == row.name {
                                    continue;
                                }
                                if ui.button(&parent).clicked() {
                                    pending = Some(PanelAction::SetBasedOn { name: row.name.clone(), kind: row.kind, based_on: Some(parent) });
                                    ui.close();
                                }
                            }
                        });
                        if ui.button(text("styles.create_based_on")).clicked() {
                            pending = Some(PanelAction::CreateBasedOn { name: row.name.clone(), kind: row.kind });
                            ui.close();
                        }
                        if ui.button(text("styles.detach")).clicked() {
                            pending = Some(PanelAction::Detach { name: row.name.clone(), kind: row.kind });
                            ui.close();
                        }
                        if row.kind == StyleKind::Character && ui.button(text("styles.clear_character_styles")).clicked() {
                            pending = Some(PanelAction::ClearCharacterStyles);
                            ui.close();
                        }
                        if ui.button(text("styles.delete")).clicked() {
                            pending = Some(PanelAction::Delete { name: row.name.clone(), kind: row.kind });
                            ui.close();
                        }
                    });
                }
            });
            ui.add_space(space("sm"));
            ui.horizontal(|ui| {
                if button(ui, text("styles.reapply")).clicked() {
                    pending = Some(PanelAction::Reapply);
                }
                let can_delete = self.selected.is_some();
                if ui.add_enabled(can_delete, egui::Button::new(text("styles.delete"))).clicked() {
                    if let Some((kind, name)) = self.selected.clone() {
                        pending = Some(PanelAction::Delete { name, kind });
                    }
                }
            });
            if let Some(message) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(180, 60, 60), message);
            }
            if let Some(editor) = self.editor.as_mut() {
                edit_action = editor.show(ui.ctx(), session);
            }
        });
        if let Some(action) = pending {
            if let Some(session) = workspace.session_mut() {
                match self.run(session, action) {
                    Ok(_) => self.error = None,
                    Err(error) => self.error = Some(text(&error.message_key).to_string()),
                }
            }
        }
        if let Some(action) = edit_action {
            if let Some(session) = workspace.session_mut() {
                match self.finish_edit(session, action) {
                    Ok(_) => self.error = None,
                    Err(error) => self.error = Some(text(&error.message_key).to_string()),
                }
            }
        }
    }
}

pub fn menu_action(id: &str, selected: Option<&(StyleKind, String)>) -> Option<PanelAction> {
    match id {
        "text.text_styles.reapply_text_styles" => Some(PanelAction::Reapply),
        "text.text_styles.reapply_base_styles" => Some(PanelAction::ReapplyBase),
        "text.text_styles.create_paragraph_style" => Some(PanelAction::Create { kind: StyleKind::Paragraph }),
        "text.text_styles.create_character_style" => Some(PanelAction::Create { kind: StyleKind::Character }),
        "text.text_styles.update_paragraph_style" => selected.filter(|(kind, _)| *kind == StyleKind::Paragraph).map(|(_, name)| PanelAction::Update { name: name.clone(), kind: StyleKind::Paragraph }),
        "text.text_styles.update_character_style" => selected.filter(|(kind, _)| *kind == StyleKind::Character).map(|(_, name)| PanelAction::Update { name: name.clone(), kind: StyleKind::Character }),
        "text.text_styles.delete_paragraph_style" => selected.filter(|(kind, _)| *kind == StyleKind::Paragraph).map(|(_, name)| PanelAction::Delete { name: name.clone(), kind: StyleKind::Paragraph }),
        "text.text_styles.delete_character_style" => selected.filter(|(kind, _)| *kind == StyleKind::Character).map(|(_, name)| PanelAction::Delete { name: name.clone(), kind: StyleKind::Character }),
        "text.text_styles.set_no_paragraph_style" => Some(PanelAction::SetNoStyle { kind: StyleKind::Paragraph }),
        "text.text_styles.set_no_character_style" => Some(PanelAction::SetNoStyle { kind: StyleKind::Character }),
        "text.text_styles.delete_unused_styles" => Some(PanelAction::DeleteUnused),
        "text.text_styles.import_text_styles" => Some(PanelAction::Import { path: String::new(), conflict: "rename".into() }),
        _ => None,
    }
}

pub fn handles_menu(id: &str) -> bool {
    id == WINDOW_TEXT_STYLES || id.starts_with("text.text_styles")
}

const PANEL_MENU: &[(&str, &str)] = &[
    ("text.text_styles.reapply_text_styles", "menu.text.text_styles.reapply_text_styles"),
    ("text.text_styles.reapply_base_styles", "menu.text.text_styles.reapply_base_styles"),
    ("text.text_styles.create_paragraph_style", "menu.text.text_styles.create_paragraph_style"),
    ("text.text_styles.update_paragraph_style", "menu.text.text_styles.update_paragraph_style"),
    ("text.text_styles.delete_paragraph_style", "menu.text.text_styles.delete_paragraph_style"),
    ("text.text_styles.create_character_style", "menu.text.text_styles.create_character_style"),
    ("text.text_styles.update_character_style", "menu.text.text_styles.update_character_style"),
    ("text.text_styles.delete_character_style", "menu.text.text_styles.delete_character_style"),
    ("text.text_styles.import_text_styles", "menu.text.text_styles.import_text_styles"),
];
