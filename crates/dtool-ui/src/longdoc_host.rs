use crate::dialogs::longdoc::{index_mark_shortcut_pressed, LongdocDialogs};
use crate::panels;
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::{json, Value};

pub const PANELS: [(&str, &str); 7] = [
    ("anchors", "menu.window.references.anchors"),
    ("cross_references", "menu.window.references.cross_references"),
    ("fields", "menu.window.references.fields"),
    ("hyperlinks", "menu.window.references.hyperlinks"),
    ("index", "menu.window.references.index"),
    ("notes", "menu.window.references.notes"),
    ("table_of_contents", "menu.window.references.table_of_contents"),
];

pub enum MenuRoute {
    Panel(&'static str),
    Dialog(&'static str),
    Run(&'static str, Value, Option<&'static str>),
    WordCount,
}

pub fn route(id: &str) -> Option<MenuRoute> {
    if let Some(key) = id.strip_prefix("window.references.") {
        return PANELS.iter().find(|(k, _)| *k == key).map(|(k, _)| MenuRoute::Panel(k));
    }
    Some(match id {
        "document.section_manager" => MenuRoute::Dialog("section_manager"),
        "text.interactive.insert_anchor" => MenuRoute::Dialog("insert_anchor"),
        "text.interactive.show_anchors" => MenuRoute::Panel("anchors"),
        "text.interactive.insert_hyperlink" => MenuRoute::Dialog("insert_hyperlink"),
        "text.interactive.show_hyperlinks" => MenuRoute::Panel("hyperlinks"),
        "text.cross_references.update" | "text.cross_references.update_all" => MenuRoute::Run("xref.update_all", json!({}), Some("cross_references")),
        "text.notes.insert_footnote" => MenuRoute::Run("note.insert", json!({ "kind": "footnote" }), Some("notes")),
        "text.notes.insert_endnote" => MenuRoute::Run("note.insert", json!({ "kind": "endnote" }), Some("notes")),
        "text.notes.insert_sidenote" => MenuRoute::Run("note.insert", json!({ "kind": "sidenote" }), Some("notes")),
        "text.notes.convert_notes" => MenuRoute::Panel("notes"),
        "text.notes.go_to_note_body" => MenuRoute::Run("note.go", json!({ "to": "body" }), Some("notes")),
        "text.notes.go_to_note_reference" => MenuRoute::Run("note.go", json!({ "to": "reference" }), Some("notes")),
        "text.notes.show_note_marks" => MenuRoute::Run("note.marks.toggle", json!({}), None),
        "text.table_of_contents.insert" => MenuRoute::Run("toc.insert", json!({}), Some("table_of_contents")),
        "text.table_of_contents.update" => MenuRoute::Run("toc.update", json!({}), Some("table_of_contents")),
        "text.table_of_contents.update_all" => MenuRoute::Run("toc.update_all", json!({}), Some("table_of_contents")),
        "text.index.insert_index_mark" => MenuRoute::Dialog("index_mark"),
        "text.index.insert_index" => MenuRoute::Run("index.insert", json!({}), Some("index")),
        "text.index.update_index" => MenuRoute::Run("index.update", json!({}), Some("index")),
        "text.index.show_index_marks" => MenuRoute::Run("index.marks.toggle", json!({}), None),
        "view.show.word_count" => MenuRoute::WordCount,
        _ => return None,
    })
}

pub fn handles(id: &str) -> bool {
    route(id).is_some()
}

pub struct LongdocHost {
    pub open: Vec<&'static str>,
    pub show_word_count: bool,
    pub anchors: panels::anchors::AnchorsPanel,
    pub xrefs: panels::xrefs::XrefsPanel,
    pub fields: panels::fields::FieldsPanel,
    pub hyperlinks: panels::hyperlinks::HyperlinksPanel,
    pub index: panels::index::IndexPanel,
    pub notes: panels::notes::NotesPanel,
    pub toc: panels::toc::TocPanel,
    pub dialogs: LongdocDialogs,
}

impl Default for LongdocHost {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            show_word_count: true,
            anchors: Default::default(),
            xrefs: Default::default(),
            fields: Default::default(),
            hyperlinks: Default::default(),
            index: Default::default(),
            notes: Default::default(),
            toc: Default::default(),
            dialogs: Default::default(),
        }
    }
}

impl LongdocHost {
    pub fn is_open(&self, key: &str) -> bool {
        self.open.iter().any(|k| *k == key)
    }

    pub fn open_panel(&mut self, key: &'static str) {
        if !self.is_open(key) {
            self.open.push(key);
        }
    }

    pub fn toggle_panel(&mut self, key: &'static str) {
        if self.is_open(key) {
            self.open.retain(|k| *k != key);
        } else {
            self.open.push(key);
        }
    }

    pub fn open_dialog(&mut self, key: &str) {
        match key {
            "section_manager" => self.dialogs.section_manager.open = true,
            "insert_anchor" => self.dialogs.insert_anchor.open = true,
            "insert_hyperlink" => self.dialogs.insert_hyperlink.open = true,
            "index_mark" => self.dialogs.index_mark.open = true,
            "index_topic" => self.dialogs.index_topic.open = true,
            _ => {}
        }
    }

    pub fn handle_menu(&mut self, id: &str, workspace: &mut CoreWorkspace) -> Option<Result<(), CommandError>> {
        let result = match route(id)? {
            MenuRoute::Panel(key) => {
                self.toggle_panel(key);
                Ok(())
            }
            MenuRoute::Dialog(key) => {
                self.open_dialog(key);
                Ok(())
            }
            MenuRoute::WordCount => {
                self.show_word_count = !self.show_word_count;
                Ok(())
            }
            MenuRoute::Run(name, params, panel) => {
                let ready = workspace.session().is_some_and(|s| s.can_execute_named(name));
                match (ready, panel) {
                    (false, Some(key)) => {
                        self.open_panel(key);
                        Ok(())
                    }
                    _ => workspace.run(Scope::active(), Command::with_params(name, params)).1.map(|_| ()),
                }
            }
        };
        Some(result)
    }

    fn show_panel(&mut self, ui: &mut egui::Ui, key: &str, workspace: &mut CoreWorkspace) {
        match key {
            "anchors" => {
                self.anchors.state.open = true;
                self.anchors.show(ui, workspace);
            }
            "cross_references" => self.xrefs.show(ui, workspace),
            "fields" => self.fields.show(ui, workspace),
            "hyperlinks" => {
                self.hyperlinks.state.open = true;
                match self.hyperlinks.show(ui, workspace) {
                    Some(panels::hyperlinks::HyperlinksRequest::Edit(index, link)) => self.dialogs.open_edit_hyperlink(index, &link),
                    Some(panels::hyperlinks::HyperlinksRequest::Insert) => self.dialogs.insert_hyperlink.open = true,
                    None => {}
                }
            }
            "index" => match self.index.show(ui, workspace) {
                Some(panels::index::IndexRequest::InsertMark) => self.dialogs.index_mark.open = true,
                Some(panels::index::IndexRequest::AddTopic) => self.dialogs.index_topic.open = true,
                None => {}
            },
            "notes" => self.notes.show(ui, workspace),
            "table_of_contents" => self.toc.show(ui, workspace),
            _ => {}
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, workspace: &mut CoreWorkspace, status_bar: &mut crate::status_bar::StatusBar) {
        dtool_tools::frame_text::install_flow_ranges();
        if !self.show_word_count {
            status_bar.stats_text = None;
        }
        for (key, title) in PANELS {
            if !self.is_open(key) {
                continue;
            }
            let mut open = true;
            egui::Window::new(text(title).to_string())
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.show_panel(ui, key, workspace));
                });
            if !open {
                self.open.retain(|k| *k != key);
            }
        }
        if self.dialogs.any_open() || index_mark_shortcut_pressed(ctx) {
            egui::Window::new(text("menu.window.references").to_string())
                .show(ctx, |ui| self.dialogs.show_open(ui, workspace));
        }
    }
}
