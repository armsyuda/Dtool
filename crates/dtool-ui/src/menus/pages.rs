use dtool_core::commands::{masters, pages};
use dtool_core::document::PageId;
use dtool_core::view::ViewTarget;
use dtool_core::{Command, ContextId, ObjectId, Session};
use dtool_strings::text;
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Masters,
    New,
    Add,
    Delete,
    Import,
    First,
    Previous,
    Next,
    Last,
    GoTo,
    PreviouslyViewed,
}

impl Action {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id.strip_prefix("document.pages.")? {
            "view_masters" => Self::Masters,
            "new" => Self::New,
            "add" => Self::Add,
            "delete" => Self::Delete,
            "add_from_file" => Self::Import,
            "first" => Self::First,
            "previous" => Self::Previous,
            "next" => Self::Next,
            "last" => Self::Last,
            "go_to" => Self::GoTo,
            "previously_viewed" => Self::PreviouslyViewed,
            _ => return None,
        })
    }

    pub fn enabled(self, session: &Session) -> bool {
        if !session.has_document || session.document.layout_artboard_origins.is_none() {
            return false;
        }
        let count = session.document.pages.len();
        let current = pages::viewed_index(session);
        match self {
            Self::Masters => !session.document.master_pages.roots.is_empty() && session.view_canvas_size.is_some(),
            Self::Delete => count > 1,
            Self::Import => cfg!(windows),
            Self::First | Self::Previous => count > 0 && current != Some(0),
            Self::Next | Self::Last => count > 0 && current != Some(count - 1),
            Self::PreviouslyViewed => pages::previous_viewed_page(session).is_some(),
            Self::GoTo => count > 0 && session.view_canvas_size.is_some(),
            _ => true,
        }
    }

    pub fn navigation(self) -> Option<Command> {
        let direction = match self {
            Self::First => "first",
            Self::Previous => "previous",
            Self::Next => "next",
            Self::Last => "last",
            Self::PreviouslyViewed => "previously_viewed",
            _ => return None,
        };
        Some(Command::with_params("page.navigate", json!({"direction": direction})))
    }
}

pub fn viewed_master(session: &Session) -> Option<ObjectId> {
    match ViewTarget::at_view_center(&session.document, &session.view, session.view_canvas_size?)? {
        ViewTarget::Artboard(id) if session.document.master_pages.roots.contains(&id) => Some(id),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Placement { Before, After, End, Replace, IntoSection }

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SpreadInsertion {
    #[default]
    Automatic,
    New,
    Extend,
}

fn section_label(doc: &dtool_core::Document, index: usize) -> String {
    let section = &doc.sections[index];
    let name = if section.name.trim().is_empty() { (index + 1).to_string() } else { section.name.clone() };
    let Some(range) = doc.section_page_range(index) else { return name; };
    format!("{name} ({}–{})", range.start + 1, range.end)
}

#[derive(PartialEq, Eq)]
enum ImportRange { All, Pages, Section }

struct ImportSource {
    path: std::path::PathBuf,
    count: usize,
    sections: Vec<(String, usize)>,
    range: ImportRange,
    pages: String,
    section: usize,
}

impl ImportSource {
    fn open(path: std::path::PathBuf) -> Result<Self, dtool_core::CommandError> {
        let mut document = dtool_core::commands::document_files::read_document(&path)?;
        let count = if document.layout_artboard_origins.is_none() {
            let mut preview = Session::new(document);
            preview.run(&Command::new("page.from_artboards"))?;
            document = preview.document;
            document.pages.len()
        } else {
            document.pages.len()
        };
        let sections = document.sections.iter().enumerate().map(|(index, _)| {
            let count = document.section_page_range(index).map_or(0, |range| range.len());
            (section_label(&document, index), count)
        }).collect();
        Ok(Self { path, count, sections, range: ImportRange::All, pages: String::new(), section: 0 })
    }

    fn selected_count(&self) -> usize {
        match self.range {
            ImportRange::All => self.count,
            ImportRange::Pages => dtool_core::longdoc::helpers::parse_page_ranges(&self.pages, self.count)
                .map_or(0, |pages| pages.len()),
            ImportRange::Section => self.sections.get(self.section).map_or(0, |(_, count)| *count),
        }
    }

    fn fields(&mut self, ui: &mut egui::Ui) {
        ui.label(text("panel.pages.import_source"));
        ui.label(self.path.file_name().unwrap_or(self.path.as_os_str()).to_string_lossy())
            .on_hover_text(self.path.display().to_string());
        ui.end_row();
        ui.label(text("panel.pages.import_range"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.range, ImportRange::All, text("panel.pages.import_all"));
            ui.selectable_value(&mut self.range, ImportRange::Pages, text("panel.pages.import_numbers"));
            if !self.sections.is_empty() {
                ui.selectable_value(&mut self.range, ImportRange::Section, text("panel.pages.import_section"));
            }
        });
        ui.end_row();
        match self.range {
            ImportRange::All => {}
            ImportRange::Pages => {
                ui.label(text("panel.pages.import_numbers"));
                ui.add(egui::TextEdit::singleline(&mut self.pages).hint_text("1,3-5").desired_width(120.0));
                ui.end_row();
            }
            ImportRange::Section => {
                ui.label(text("panel.pages.import_section"));
                egui::ComboBox::from_id_salt("page.import.section")
                    .selected_text(self.sections.get(self.section).map_or("", |(label, _)| label.as_str()))
                    .show_ui(ui, |ui| {
                        for (index, (label, _)) in self.sections.iter().enumerate() {
                            ui.selectable_value(&mut self.section, index, label);
                        }
                    });
                ui.end_row();
            }
        }
    }
}

pub struct Dialog {
    pub owner: ContextId,
    action: Action,
    number: usize,
    count: usize,
    end: usize,
    placement: Placement,
    spread: SpreadInsertion,
    target_section: usize,
    section_at_start: bool,
    master: Option<ObjectId>,
    import: Option<ImportSource>,
    pub error: Option<String>,
}

impl Dialog {
    pub fn new(owner: ContextId, action: Action, session: &Session) -> Self {
        let number = pages::viewed_index(session).unwrap_or(0) + 1;
        Self {
            owner, action, number, count: 1, end: number, placement: Placement::After,
            spread: SpreadInsertion::Automatic,
            target_section: session.document.sections.iter().rposition(|section| section.start_page <= number).unwrap_or(0),
            section_at_start: false,
            master: session.document.pages.get(number - 1).and_then(|page| session.document.master_pages.for_page(page.id))
                .map(|assignment| assignment.master),
            import: None,
            error: None,
        }
    }

    pub fn from_file(owner: ContextId, session: &Session, path: std::path::PathBuf) -> Result<Self, dtool_core::CommandError> {
        let mut dialog = Self::new(owner, Action::Import, session);
        dialog.import = Some(ImportSource::open(path)?);
        Ok(dialog)
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &Session) -> (bool, Option<Command>) {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

        let pages = &session.document.pages;
        if !self.action.enabled(session) || pages.is_empty() {
            return (false, None);
        }
        let (title, id) = match self.action {
            Action::Add => ("menu.document.pages.add", "page.add.dialog"),
            Action::Import => ("menu.document.pages.add_from_file", "page.import.dialog"),
            Action::Delete => ("menu.document.pages.delete", "page.delete.dialog"),
            Action::GoTo => ("menu.document.pages.go_to", "page.go_to.dialog"),
            _ => return (false, None),
        };
        let inserts = matches!(self.action, Action::Add | Action::Import);
        let import_count = self.import.as_ref().map_or(1, ImportSource::selected_count);
        let setup = &session.document.setup;
        let anchor = if self.placement == Placement::End { pages.len() } else { self.number };
        let extend_cover = self.action == Action::Add && self.spread == SpreadInsertion::Extend
            && anchor == 1 && setup.facing_pages && setup.start_on_right;
        let valid = !extend_cover && import_count > 0 && match self.action {
            Action::Delete => self.end <= pages.len() && self.end >= self.number && self.end - self.number + 1 < pages.len(),
            _ if inserts && self.placement == Placement::End => true,
            Action::Import if self.placement == Placement::Replace => self.number + import_count - 1 <= pages.len(),
            Action::Import if self.placement == Placement::IntoSection => session.document.section_page_range(self.target_section).is_some(),
            _ => self.number <= pages.len(),
        };
        let (event, (accept, cancel)) = show_modal(ctx, id, text(title), ClosePolicy::CancelOnly, valid, |ui| {
            egui::Grid::new(id).show(ui, |ui| {
                if let Some(source) = &mut self.import {
                    source.fields(ui);
                }
                if self.action == Action::Add {
                    ui.label(text("dialog.new_document.pages"));
                    ui.add(egui::DragValue::new(&mut self.count).range(1..=pages::MAX_ADDED_PAGES));
                    ui.end_row();
                    ui.label(text("panel.pages.spread"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.spread, SpreadInsertion::Automatic, text("panel.pages.spread_auto"));
                        ui.selectable_value(&mut self.spread, SpreadInsertion::New, text("panel.pages.spread_new"));
                        ui.selectable_value(&mut self.spread, SpreadInsertion::Extend, text("panel.pages.spread_extend"));
                    });
                    ui.end_row();
                }
                if inserts {
                    ui.label(text(if self.action == Action::Import { "panel.pages.import_placement" } else { "panel.pages.insertion" }));
                    ui.horizontal(|ui| {
                        for (value, key) in [
                            (Placement::Before, "panel.pages.before"),
                            (Placement::After, "panel.pages.after"),
                            (Placement::End, "panel.pages.at_end"),
                        ] {
                            ui.selectable_value(&mut self.placement, value, text(key));
                        }
                        if self.action == Action::Import {
                            ui.selectable_value(&mut self.placement, Placement::Replace, text("panel.pages.import_replace"));
                            if !session.document.sections.is_empty() {
                                ui.selectable_value(&mut self.placement, Placement::IntoSection, text("panel.pages.import_into_section"));
                            }
                        }
                    });
                    ui.end_row();
                }
                if self.placement == Placement::IntoSection {
                    let doc = &session.document;
                    ui.label(text("panel.pages.import_target_section"));
                    let label = if self.target_section < doc.sections.len() { section_label(doc, self.target_section) } else { String::new() };
                    egui::ComboBox::from_id_salt("page.import.target_section").selected_text(label).show_ui(ui, |ui| {
                        for index in 0..doc.sections.len() {
                            if doc.section_page_range(index).is_some() {
                                ui.selectable_value(&mut self.target_section, index, section_label(doc, index));
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(text("panel.pages.insertion"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.section_at_start, true, text("panel.pages.import_section_start"));
                        ui.selectable_value(&mut self.section_at_start, false, text("panel.pages.import_section_end"));
                    });
                    ui.end_row();
                } else if !inserts || self.placement != Placement::End {
                    ui.label(text(if self.action == Action::Delete || self.placement == Placement::Replace { "panel.pages.range_start" } else { "panel.pages.go_to_number" }));
                    ui.add(egui::DragValue::new(&mut self.number).range(1..=pages.len()));
                    ui.end_row();
                }
                if self.placement == Placement::Replace && import_count > 0 {
                    ui.label(text("panel.pages.replace_range"));
                    let end = self.number + import_count - 1;
                    let range = if end == self.number { end.to_string() } else { format!("{}–{}", self.number, end) };
                    if end <= pages.len() {
                        ui.label(range);
                    } else {
                        ui.colored_label(ui.visuals().error_fg_color, range).on_hover_text(text("error.out_of_range"));
                    }
                    ui.end_row();
                }
                if self.action == Action::Delete {
                    ui.label(text("panel.pages.range_end"));
                    ui.add(egui::DragValue::new(&mut self.end).range(1..=pages.len()));
                    ui.end_row();
                }
                if self.action == Action::Add && !session.document.master_pages.roots.is_empty() {
                    ui.label(text("panel.master"));
                    let doc = &session.document;
                    let label = self.master.and_then(|id| doc.get_object(id)).map_or(text("panel.pages.no_master"), masters::master_name);
                    egui::ComboBox::from_id_salt("page.add.master").selected_text(label).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.master, None, text("panel.pages.no_master"));
                        for root in doc.master_pages.roots.iter().filter_map(|id| doc.get_object(*id)) {
                            ui.selectable_value(&mut self.master, Some(root.id), masters::master_name(root));
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, text(error));
            }
            if extend_cover {
                ui.label(text("panel.pages.spread_cover"));
            }
            ui.horizontal(|ui| {
                let label = if self.action == Action::Delete { text("panel.pages.delete") }
                    else if self.placement == Placement::Replace { text("panel.pages.import_replace") } else { text("common.ok") };
                let accept = ui.add_enabled(valid, egui::Button::new(label)).clicked();
                let cancel = ui.button(text("button.cancel")).clicked();
                (accept, cancel)
            }).inner
        });
        if valid && (accept || event == DialogResult::Confirm) {
            return (false, self.command(session));
        }
        (!cancel && event != DialogResult::Cancel, None)
    }

    fn command(&self, session: &Session) -> Option<Command> {
        let pages = &session.document.pages;
        let mut params = match self.action {
            Action::GoTo => return Some(Command::with_params("page.navigate", json!({"number": self.number}))),
            Action::Delete => {
                let ids: Vec<PageId> = pages.get(self.number.checked_sub(1)?..self.end)?.iter().map(|page| page.id).collect();
                return Some(Command::with_params("page.delete", json!({"ids": ids})));
            }
            Action::Add => json!({"count": self.count}),
            Action::Import => {
                let source = self.import.as_ref()?;
                let mut params = json!({"path": source.path});
                match source.range {
                    ImportRange::All => {}
                    ImportRange::Pages => params["pages"] = json!(source.pages.trim()),
                    ImportRange::Section => params["section"] = json!(source.section + 1),
                }
                params
            }
            _ => return None,
        };
        let placement_key = match self.placement {
            Placement::Before => Some("before_id"),
            Placement::After => Some("after_id"),
            Placement::Replace => Some("replace_from_id"),
            Placement::End => None,
            Placement::IntoSection => {
                params["into_section"] = json!(self.target_section + 1);
                params["section_edge"] = json!(if self.section_at_start { "start" } else { "end" });
                None
            }
        };
        if let Some(key) = placement_key {
            params[key] = json!(pages.get(self.number.checked_sub(1)?)?.id);
        }
        if self.action == Action::Import {
            Some(Command::with_params("page.import", params))
        } else {
            match self.spread {
                SpreadInsertion::Automatic => {}
                SpreadInsertion::New => params["spread"] = json!("new"),
                SpreadInsertion::Extend => params["spread"] = json!("extend"),
            }
            if let Some(master) = self.master {
                params["master_id"] = json!(master);
            }
            Some(Command::with_params("page.add", params))
        }
    }
}

pub fn new_page(session: &Session) -> Command {
    let mut params = Value::Object(Default::default());
    if let Some(id) = pages::viewed_page(session) {
        params["after_id"] = json!(id);
        if let Some(assignment) = session.document.master_pages.for_page(id) {
            params["master_id"] = json!(assignment.master);
        }
    }
    Command::with_params("page.add", params)
}
