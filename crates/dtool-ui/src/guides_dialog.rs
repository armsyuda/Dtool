use dtool_core::document::PageId;
use dtool_core::guides::{Guide, GuideKind, PercentAxis};
use dtool_core::guides::column_layout::{ColumnLayout, Style};
use dtool_core::{CommandError, Session};
use dtool_design::{button, dialog_frame, dropdown, numeric_field, numeric_value, panel_header, space, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use egui::Ui;

pub const SHOW_GUIDES_MANAGER: &str = "view.guides_manager";
pub const MARGINS_AND_GUIDES: &str = "view.margins_and_guides";
pub const LEGACY_COMMAND: &str = "ShowGuidesManagerCommand";
pub const LEGACY_XAML: &str = "ui__dialogs__guidesmanagerdialog.xaml";
pub const MENU_KEY: &str = "menu.view.guides_manager";
pub const KINDS: [GuideKind; 3] = [GuideKind::Horizontal, GuideKind::Vertical, GuideKind::Column];

pub fn menu_label() -> &'static str {
    text(MENU_KEY)
}

pub fn kind_key(kind: GuideKind) -> &'static str {
    match kind {
        GuideKind::Horizontal => "dialog.guides.kind.horizontal",
        GuideKind::Vertical => "dialog.guides.kind.vertical",
        GuideKind::Column => "dialog.guides.kind.column",
    }
}

pub fn kind_index(kind: GuideKind) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(0)
}

pub fn kind_at(index: usize) -> GuideKind {
    KINDS.get(index).copied().unwrap_or(GuideKind::Horizontal)
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuideRow {
    pub id: u64,
    pub kind: usize,
    pub position: f64,
    pub name: String,
}

impl GuideRow {
    pub fn from_guide(guide: &Guide) -> Self {
        Self {
            id: guide.id.0,
            kind: kind_index(guide.kind),
            position: guide.position as f64,
            name: guide.name.clone().unwrap_or_default(),
        }
    }
}


fn units_suffix(units: dtool_core::document::Units) -> &'static str {
    match units {
        dtool_core::document::Units::Px => "px",
        dtool_core::document::Units::Mm => "mm",
        dtool_core::document::Units::Cm => "cm",
        dtool_core::document::Units::In => "in",
        dtool_core::document::Units::Pt => "pt",
    }
}

pub const GUIDE_NUMBER_KEY: &str = "dialog.guides.number";

pub fn guide_number_header() -> String {
    if dtool_strings::has(GUIDE_NUMBER_KEY) {
        text(GUIDE_NUMBER_KEY).to_string()
    } else {
        String::new()
    }
}

pub fn guide_number(row: &GuideRow) -> String {
    row.id.to_string()
}

pub fn guide_rows(session: &Session) -> Vec<GuideRow> {
    session.guides.list().iter().map(GuideRow::from_guide).collect()
}

fn optional_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn run_guide_command(session: &mut Session, name: &str, params: serde_json::Value) -> Result<serde_json::Value, CommandError> {
    let command = dtool_core::commands::ruler_units::normalize(session, &dtool_core::Command::with_params(name, params))?;
    session.run(&command)
}

pub fn add_guide(session: &mut Session, kind: GuideKind, position: f32, name: &str) -> Result<u64, CommandError> {
    add_scoped_guide(session, kind, position, name, None)
}

fn add_scoped_guide(session: &mut Session, kind: GuideKind, position: f32, name: &str, page: Option<PageId>) -> Result<u64, CommandError> {
    let mut params = serde_json::json!({ "kind": kind, "position": position, "page_id": page });
    if let Some(n) = optional_name(name) {
        params["name"] = serde_json::Value::String(n);
    }
    let result = run_guide_command(session, "guides.add", params)?;
    result
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() })
}

fn position_field(ui: &mut Ui, id: egui::Id, position: f64, axis: Option<PercentAxis>, units: &str) -> NumericEvent {
    let shown = axis.map_or(position, |axis| axis.to_percent(position));
    let spec = NumericSpec::new("").suffix(if axis.is_some() { "%" } else { units }).decimals(6).width(96.0);
    let (_, event) = numeric_field(ui, id, NumericValue::Value(shown), &spec);
    let convert = |value| match axis {
        Some(axis) => axis.from_percent(value),
        None => ((value as f32).is_finite()).then_some(value),
    };
    match event {
        NumericEvent::Commit(value) => convert(value).map_or(NumericEvent::Cancel, NumericEvent::Commit),
        NumericEvent::Preview(value) => convert(value).map_or(NumericEvent::Cancel, NumericEvent::Preview),
        other => other,
    }
}

pub fn update_guide(session: &mut Session, row: &GuideRow) -> Result<(), CommandError> {
    let current = session.guides.list().iter().find(|g| g.id.0 == row.id).cloned()
        .ok_or_else(|| CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() })?;
    let mut params = serde_json::json!({ "id": row.id });
    let kind = kind_at(row.kind);
    if kind != current.kind {
        params["kind"] = serde_json::json!(kind);
    }
    let position = row.position as f32;
    if position != current.position {
        params["position"] = serde_json::json!(position);
    }
    let name = optional_name(&row.name);
    if name != current.name {
        params["name"] = serde_json::json!(name);
    }
    if params.as_object().map_or(0, |m| m.len()) <= 1 {
        return Ok(());
    }
    run_guide_command(session, "guides.update", params).map(|_| ())
}

pub fn rename_guide(session: &mut Session, id: u64, name: &str) -> Result<(), CommandError> {
    let params = serde_json::json!({ "id": id, "name": optional_name(name) });
    run_guide_command(session, "guides.update", params).map(|_| ())
}

pub fn move_guide(session: &mut Session, id: u64, position: f32) -> Result<(), CommandError> {
    session.run_named("guides.update", serde_json::json!({ "id": id, "position": position })).map(|_| ())
}

pub fn delete_guide(session: &mut Session, id: u64) -> Result<(), CommandError> {
    session.run_named("guides.delete", serde_json::json!({ "id": id })).map(|_| ())
}

#[derive(Debug, Clone, Default)]
struct ColumnGuideEditor {
    page: Option<dtool_core::document::PageId>,
    draft: Option<ColumnLayout>,
}

impl ColumnGuideEditor {
    fn show(&mut self, ui: &mut Ui, session: &Session) -> Option<dtool_core::Command> {
        let pages = &session.document.pages;
        let viewed = dtool_core::commands::pages::viewed_page(session);
        let index = |id| pages.iter().position(|page| Some(page.id) == id);
        let initial = index(self.page).or_else(|| index(viewed)).or_else(|| (!pages.is_empty()).then_some(0))?;
        let mut selected = initial;
        let page_label = |index: usize| {
            let number = (index + 1).to_string();
            let label = dtool_strings::format("panel.pages.number", &[("number", &number)]);
            let name = &pages[index].name;
            if name.is_empty() { label } else { format!("{label} · {name}") }
        };
        ui.horizontal(|ui| {
            ui.strong(text("dialog.guides.column_layout"));
            egui::ComboBox::from_id_salt("column-guides-page").width(115.0).selected_text(page_label(initial)).show_ui(ui, |ui| {
                for index in 0..pages.len() {
                    ui.selectable_value(&mut selected, index, page_label(index));
                }
            });
        });
        let page = &pages[selected];
        if self.page != Some(page.id) {
            self.page = Some(page.id);
            self.draft = None;
        }
        let current = page.column_guides.unwrap_or_default();
        let mut grid = self.draft.unwrap_or(current);
        ui.horizontal(|ui| {
            for (key, count) in [("dialog.guides.columns", &mut grid.columns), ("dialog.guides.rows", &mut grid.rows)] {
                ui.label(text(key));
                let mut value = f64::from(*count);
                let spec = NumericSpec::new("").range(1.0..=f64::from(ColumnLayout::MAX_COUNT)).decimals(0).width(44.0);
                let id = ui.id().with((key, page.id));
                numeric_value(ui, id, &mut value, &spec);
                *count = value.round() as u16;
            }
            ui.label(text("frame.gutter"));
            let mut value = f64::from(grid.gutter);
            let spec = NumericSpec::new("").range(0.0..=1_000_000.0).suffix(units_suffix(session.document.units)).decimals(3).width(86.0);
            let id = ui.id().with(("column-guides-gutter", page.id));
            numeric_value(ui, id, &mut value, &spec);
            grid.gutter = value as f32;
        });
        let mut layout = None;
        ui.horizontal(|ui| {
            for (style, key) in [(Style::Filled, "dialog.guides.filled"), (Style::Outline, "menu.view.mode.outline")] {
                ui.selectable_value(&mut grid.style, style, text(key));
            }
            self.draft = (grid != current).then_some(grid);
            if ui.add_enabled(self.draft.is_some(), egui::Button::new(text("button.apply"))).clicked() {
                layout = Some(Some(grid));
            }
            if ui.add_enabled(page.column_guides.is_some() || self.draft.is_some(), egui::Button::new(text("dialog.guides.clear_layout"))).clicked() {
                self.draft = None;
                layout = Some(None);
            }
        });
        layout.map(|layout| dtool_core::Command::with_params(dtool_core::commands::pages::COLUMN_GUIDES_SET,
            serde_json::json!({ "id": page.id, "column_guides": layout })))
    }
}

#[derive(Debug, Clone, Default)]
pub struct GuidesDialog {
    pub open: bool,
    pub new_kind: usize,
    pub new_position: f64,
    pub new_name: String,
    pub error: Option<String>,
    pub preview: Option<GuideRow>,
    pub preview_cancelled: bool,
    margin_draft: Option<dtool_core::doc_setup::Margins>,
    name_drafts: std::collections::HashMap<u64, String>,
    column_guides: ColumnGuideEditor,
    percent_positions: bool,
    new_on_page: bool,
}

impl GuidesDialog {
    pub(crate) fn percent_positions(&self) -> bool { self.percent_positions }

    pub(crate) fn set_percent_positions(&mut self, percent: bool) { self.percent_positions = percent; }

    pub fn begin_preview(&mut self, row: GuideRow) {
        if !self.preview_cancelled {
            self.preview = Some(row);
        }
    }

    pub fn cancel_preview(&mut self) {
        if self.preview.take().is_some() {
            self.preview_cancelled = true;
        }
    }

    pub fn finish_preview(&mut self, session: &mut Session, row: &GuideRow) -> Result<(), CommandError> {
        let cancelled = std::mem::take(&mut self.preview_cancelled);
        self.preview = None;
        if cancelled {
            return Ok(());
        }
        let result = update_guide(session, row);
        self.error = result.as_ref().err().map(|e| e.message_key.clone());
        result
    }

    pub fn preview_line(&self) -> Option<(GuideKind, f32)> {
        if !self.open {
            return None;
        }
        self.preview.as_ref().map(|row| (kind_at(row.kind), row.position as f32))
    }

    pub(crate) fn preview_guide(&self, session: &Session) -> Option<Guide> {
        let (kind, position) = self.preview_line()?;
        let id = dtool_core::guides::GuideId(self.preview.as_ref()?.id);
        let mut guide = session.guides.get(id)?.clone();
        guide.kind = kind;
        guide.position = position;
        Some(guide)
    }

    pub fn open(&mut self) {
        self.open = true;
        self.error = None;
        self.margin_draft = None;
        self.name_drafts.clear();
        self.column_guides = ColumnGuideEditor::default();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.preview = None;
        self.preview_cancelled = false;
        self.margin_draft = None;
        self.name_drafts.clear();
        self.column_guides = ColumnGuideEditor::default();
    }

    pub fn add(&mut self, session: &mut Session) -> Result<u64, CommandError> {
        self.add_in_scope(session, None)
    }

    fn add_in_scope(&mut self, session: &mut Session, page: Option<PageId>) -> Result<u64, CommandError> {
        let result = add_scoped_guide(session, kind_at(self.new_kind), self.new_position as f32, &self.new_name, page);
        match &result {
            Ok(_) => {
                self.new_name.clear();
                self.error = None;
            }
            Err(e) => self.error = Some(e.message_key.clone()),
        }
        result
    }

    pub fn delete(&mut self, session: &mut Session, id: u64) -> Result<(), CommandError> {
        let result = delete_guide(session, id);
        self.error = result.as_ref().err().map(|e| e.message_key.clone());
        result
    }

    fn commit(&mut self, session: &mut Session, row: &GuideRow) {
        if let Err(e) = update_guide(session, row) {
            self.error = Some(e.message_key);
        }
    }

    pub fn show(&mut self, ui: &mut Ui, session: &mut Session) {
        if !self.open {
            return;
        }
        let kind_labels: Vec<String> = KINDS.iter().map(|k| text(kind_key(*k)).to_string()).collect();
        let kind_options: Vec<&str> = kind_labels.iter().map(|s| s.as_str()).collect();
        let base = guide_rows(session);
        let mut rows = base.clone();
        if let Some(preview) = &self.preview {
            match rows.iter_mut().find(|row| row.id == preview.id) {
                Some(row) => *row = preview.clone(),
                None => self.preview = None,
            }
        }
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        let mut preview_row: Option<GuideRow> = None;
        let mut released: Option<GuideRow> = None;
        let mut commits: Vec<GuideRow> = Vec::new();
        let mut renames = Vec::new();
        let mut delete_id: Option<u64> = None;
        let mut add_clicked = false;
        let mut add_page = None;
        let viewed_page = dtool_core::commands::pages::viewed_page(session).or_else(|| session.document.pages.first().map(|page| page.id));
        let percent_positions = self.percent_positions;
        let mut next_percent_positions = percent_positions;
        let mut percent_axes = std::collections::HashMap::new();
        let mut percent_axis = |doc: &dtool_core::Document, page, kind: GuideKind| {
            if !percent_positions { return None; }
            *percent_axes.entry((page, kind.axis())).or_insert_with(|| PercentAxis::new(doc, page, kind))
        };
        let mut apply_margins = false;
        let mut column_command = None;
        let stored_margins = session.document.setup.margins.into();
        let position_units = units_suffix(session.document.units);
        let reset_margin_edits = self.margin_draft.is_none();
        let margins = self.margin_draft.get_or_insert(stored_margins);
        let mut close = false;
        egui::Area::new(egui::Id::new("dtool.dialog.guides_manager"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                dialog_frame(ui, |ui| {
                    panel_header(ui, text("dialog.guides.margins_and_guides"));
                    let edit = crate::dialogs::sides_editor(
                        ui, margins, session.document.units, "doc_setup.margins", reset_margin_edits,
                    );
                    let next = edit.pending.map_or(*margins, |(_, draft)| draft);
                    apply_margins = ui.add_enabled(!edit.invalid && next != stored_margins, egui::Button::new(text("button.apply"))).clicked();
                    if apply_margins {
                        *margins = next;
                        if let Some((id, _)) = edit.pending {
                            dtool_design::numeric_cancel_edit(ui.ctx(), id);
                        }
                    }
                    if edit.invalid {
                        ui.colored_label(ui.visuals().error_fg_color, text("error.invalid_params"));
                    }
                    ui.separator();
                    ui.strong(text("ruler.origin.title"));
                    crate::rulers::origin_controls(ui, session);
                    ui.separator();
                    column_command = self.column_guides.show(ui, session);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong(text("dialog.guides.title"));
                        let toggle = ui.checkbox(&mut next_percent_positions, text("dialog.guides.percent"));
                        dtool_design::tooltip(toggle, text("dialog.guides.percent_help"));
                    });
                    ui.add_space(space("sm"));
                    if rows.is_empty() {
                        ui.label(text("dialog.guides.empty"));
                    }
                    egui::ScrollArea::vertical().max_height(240.0).auto_shrink([true, true]).show(ui, |ui| {
                        egui::Grid::new("dtool.dialog.guides_manager.rows").show(ui, |ui| {
                            ui.label(guide_number_header());
                            ui.label(text("dialog.guides.kind"));
                            ui.label(text("dialog.guides.name"));
                            ui.label(text("dialog.guides.position"));
                            ui.end_row();
                            for (index, row) in rows.iter_mut().enumerate() {
                                let before = base.get(index).cloned().unwrap_or_else(|| row.clone());
                                row.name.clone_from(&before.name);
                                let page = session.guides.get(dtool_core::guides::GuideId(row.id)).and_then(|guide| guide.page_id);
                                let number = page
                                    .and_then(|id| session.document.pages.iter().position(|page| page.id == id))
                                    .map(|index| (index + 1).to_string());
                                let label = number.map_or_else(|| guide_number(row), |number| {
                                    format!("{} · {}", guide_number(row), dtool_strings::format("panel.pages.number", &[("number", &number)]))
                                });
                                ui.label(label);
                                let salt = format!("dtool.dialog.guides_manager.kind.{}", row.id);
                                let kind_changed = dropdown(ui, &salt, &mut row.kind, &kind_options).changed();
                                let mut name = self.name_drafts.remove(&row.id).unwrap_or_else(|| before.name.clone());
                                let name_resp = ui.add_sized([150.0, ui.spacing().interact_size.y],
                                    egui::TextEdit::singleline(&mut name).id_salt(("guide-name", row.id)));
                                if escape && name_resp.has_focus() {
                                    name_resp.surrender_focus();
                                } else if !escape && name_resp.lost_focus() && name != before.name {
                                    renames.push((row.id, name));
                                } else if name_resp.has_focus() && name != before.name {
                                    self.name_drafts.insert(row.id, name);
                                }
                                let axis = percent_axis(&session.document, page, kind_at(row.kind));
                                let event = position_field(ui, egui::Id::NULL.with(("guide_position", row.id, percent_positions)),
                                    row.position, axis, position_units);
                                let delete = button(ui, text("dialog.guides.delete")).clicked();
                                ui.end_row();
                                if delete {
                                    delete_id = Some(row.id);
                                } else if let NumericEvent::Commit(position) = event {
                                    row.position = position;
                                    released = Some(row.clone());
                                } else if let NumericEvent::Preview(position) = event {
                                    row.position = position;
                                    preview_row = Some(row.clone());
                                } else if kind_changed {
                                    commits.push(row.clone());
                                }
                            }
                        });
                    });
                    ui.add_space(space("md"));
                    ui.horizontal(|ui| {
                        ui.label(text("dialog.guides.scope"));
                        ui.selectable_value(&mut self.new_on_page, false, text("dialog.guides.document"));
                        if viewed_page.is_some() {
                            ui.selectable_value(&mut self.new_on_page, true, text("dialog.guides.current_page"));
                        }
                    });
                    add_page = viewed_page.filter(|_| self.new_on_page);
                    ui.horizontal(|ui| {
                        dropdown(ui, "dtool.dialog.guides_manager.new_kind", &mut self.new_kind, &kind_options);
                        ui.label(text("dialog.guides.name"));
                        ui.text_edit_singleline(&mut self.new_name);
                        ui.label(text("dialog.guides.position"));
                        let axis = percent_axis(&session.document, add_page, kind_at(self.new_kind));
                        let id = egui::Id::NULL.with(("guide_new_position", percent_positions));
                        if let NumericEvent::Commit(position) = position_field(ui, id, self.new_position, axis, position_units) {
                            self.new_position = position;
                        }
                        add_clicked = button(ui, text("dialog.guides.add")).clicked();
                    });
                    if let Some(key) = &self.error {
                        ui.label(text(key));
                    }
                    ui.add_space(space("md"));
                    if button(ui, text("button.close")).clicked() {
                        close = true;
                    }
                });
            });
        if apply_margins {
            let result = session.run_named("doc.setup.apply", serde_json::json!({"margins": margins}));
            self.error = result.err().map(|error| error.message_key);
        }
        if let Some(command) = column_command {
            match session.run(&command) {
                Ok(_) => { self.column_guides.draft = None; self.error = None; }
                Err(error) => self.error = Some(error.message_key),
            }
        }
        if let Some(row) = preview_row {
            self.begin_preview(row);
        }
        if escape {
            self.preview = None;
            self.preview_cancelled = false;
        }
        if let Some(row) = released {
            let _ = self.finish_preview(session, &row);
        }
        for row in commits {
            self.commit(session, &row);
        }
        for (id, name) in renames {
            if Some(id) != delete_id {
                self.error = rename_guide(session, id, &name).err().map(|error| error.message_key);
            }
        }
        if let Some(id) = delete_id {
            let _ = self.delete(session, id);
        }
        if add_clicked {
            let _ = self.add_in_scope(session, add_page);
        }
        self.percent_positions = next_percent_positions;
        if close {
            self.close();
        }
    }
}
