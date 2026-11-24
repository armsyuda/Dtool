use dtool_core::{Session, Workspace as CoreWorkspace};
use dtool_core::prefs::{Prefs, PrefsError};
use dtool_strings::text;

pub mod about;
pub mod captions;
pub mod canvas;
pub mod app_canvas;
pub mod place_tool;
pub mod picture_frame_bar;
pub mod canvas_input;
mod brush_editor;
mod compound_canvas;
pub mod canvas_tools;
pub mod canvas_curves;
pub mod ai_connection;
pub mod console;
pub mod feature_finder;
pub mod headless;
pub mod ui_host;
pub mod context {
    pub mod text_bar;
}
pub mod context_bar;
mod qr_editor;
mod artboard_bar;
mod qr_merge;
pub mod context_menu;
pub mod panel_actions;
pub mod dialog_chrome;
pub mod dialogs;
pub mod dock;
pub mod fill_stroke;
pub mod float;
pub mod fonts;
pub mod icon_cache;
pub mod guides_dialog;
pub mod history_panel;
pub mod input;
pub mod tablet_input;
pub mod hatch_editor;
pub mod home;
pub mod macro_panel;
pub mod mask_actions;
pub mod menu_tree;
pub mod navigator;
pub mod panel_ids;
pub mod longdoc_host;
pub mod panels;
pub mod prefs_dialog;
pub mod presets;
pub mod print;
pub mod print_dialog;
pub mod settings;
pub mod shortcut_commands;
pub mod shortcuts;
pub mod shortcuts_page;
pub mod snapping_dialog;
pub mod snapshots_panel;
pub mod table;
pub mod theme;
pub mod tooltips;
pub mod transform_panel;
pub mod ui_bridge;
pub mod menus {
    pub mod pages;
    pub mod file_menu;
    pub mod layer_convert;
    pub mod layer_lock;
    pub mod text_insert;
    pub mod text_format;
    pub mod vector_warp;
}
pub mod text_edit {
    pub mod paste;
}
pub mod window_menu;

#[derive(Default)]
pub struct Workspace {
    pub grid_and_axis: grid_dialog::GridAndAxisDialog,
    pub baseline_grid: grid_dialog::BaselineGridDialog,
    pub core: CoreWorkspace,
    pub layout: dock::Layout,
    pub prefs: Prefs,
    pub ui_settings: settings::UiSettings,
    pub prefs_dialog: prefs_dialog::PrefsDialog,
    pub console_panel: console::ConsolePanel,
    pub guides_dialog: guides_dialog::GuidesDialog,
    pub history_panel: history_panel::HistoryPanel,
    pub snapshots_panel: snapshots_panel::SnapshotsPanel,
    pub print_dialog: print_dialog::PrintDialog,
    pub macro_panel: macro_panel::MacroPanel,
    pub books_panel: panels::books::BooksPanel,
    pub fields_panel: panels::fields::FieldsPanel,
    pub anchors_panel: panels::anchors::AnchorsPanel,
    pub hyperlinks_panel: panels::hyperlinks::HyperlinksPanel,
    pub notes_panel: panels::notes::NotesPanel,
    pub toc_panel: panels::toc::TocPanel,
    pub swatches_panel: panels::swatches::SwatchesPanel,
    pub index_panel: panels::index::IndexPanel,
    pub longdoc_dialogs: dialogs::longdoc::LongdocDialogs,
    pub selection_context_bar: selection_ui::SelectionContextBar,
    pub fill_tool_bar: tools::fill_tool_bar::FillToolBar,
    pub trace_dialog: trace_dialog::TraceDialog,
    pub canvas_input: canvas_input::CanvasInput,
}

impl Workspace {
    pub fn session(&mut self) -> Option<&mut Session> {
        self.core.session_mut()
    }

    pub fn run_named(&mut self, name: &str, params: serde_json::Value) -> Result<serde_json::Value, dtool_core::CommandError> {
        match self.core.session_mut() {
            Some(session) => session.run_named(name, params),
            None => Err(dtool_core::CommandError::no_document()),
        }
    }

    pub fn show_with(&mut self, ui: &mut egui::Ui, shell: &mut CoreWorkspace) {
        std::mem::swap(&mut self.core, shell);
        self.show(ui);
        std::mem::swap(&mut self.core, shell);
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if menu_tree::install() {
            context_menu::install();
        }
        dtool_tools::frame_text::install_filler();
        dtool_tools::frame_text::install_flow_ranges();
        ui.menu_button(text("menu.view"), |ui| {
            if let Some(session) = self.core.session_mut() {
                ui.menu_button(text("menu.view.show"), |ui| grid_dialog::view_show_grid_items(ui, session));
                grid_dialog::view_grid_dialog_items(ui, session, &mut self.grid_and_axis, &mut self.baseline_grid);
            }
        });
        
        ui.menu_button(text("menu.vector"), |ui| {
            let session = self.core.session();
            let source = session.and_then(dtool_core::commands::image_trace::selected_source);
            if ui.add_enabled(source.is_some(), egui::Button::new(text("menu.vector.image_trace"))).clicked()
                && let Some((session, id)) = session.zip(source)
            {
                let _ = self.trace_dialog.open(session, id);
            }
        });

        if let Some(session) = self.core.session_mut() {
            self.grid_and_axis.show(ui.ctx(), session);
            self.baseline_grid.show(ui.ctx(), session);
            let _ = dtool_core::command::autosave_commands::poll(session, &std::format!("{}-workspace", std::process::id()), None);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));

        self.prefs_dialog.show(ui, &mut self.prefs, &mut self.ui_settings);
        if let Some(session) = self.core.session_mut() {
            self.guides_dialog.show(ui, session);
            self.trace_dialog.show(ui, session);
        }

        if self.core.session().and_then(|session| session.document()).is_some() {
            ui.label(text("app.ready"));
        } else {
            ui.label(text("app.no_document"));
        }

        self.console_panel.show(ui, &mut self.core);
        self.books_panel.show(ui, &mut self.core);
        self.fields_panel.show(ui, &mut self.core);
        self.anchors_panel.show(ui, &mut self.core);
        match self.hyperlinks_panel.show(ui, &mut self.core) {
            Some(panels::hyperlinks::HyperlinksRequest::Edit(index, link)) => self.longdoc_dialogs.open_edit_hyperlink(index, &link),
            Some(panels::hyperlinks::HyperlinksRequest::Insert) => self.longdoc_dialogs.insert_hyperlink.open = true,
            None => {}
        }
        self.longdoc_dialogs.show(ui, &mut self.core);
        self.notes_panel.show(ui, &mut self.core);
        self.toc_panel.show(ui, &mut self.core);
        self.swatches_panel.show(ui, &mut self.core);
        match self.index_panel.show(ui, &mut self.core) {
            Some(panels::index::IndexRequest::InsertMark) => self.longdoc_dialogs.index_mark.open = true,
            Some(panels::index::IndexRequest::AddTopic) => self.longdoc_dialogs.index_topic.open = true,
            None => {}
        }
        self.print_dialog.show(ui);
        if let Some(session) = self.core.session_mut() {
            self.macro_panel.show(ui, session);
        }

        
        let mut selection_context_bar = std::mem::take(&mut self.selection_context_bar);
        selection_context_bar.show(ui, self);
        self.selection_context_bar = selection_context_bar;

        let mut bound = None;
        if let (Some(context), Some(session)) = (self.core.active(), self.core.session()) {
            bound = context_bar::pick_bound(ui, session, context);
        }
        if let Some((edit, command)) = bound {
            let closed = context_bar::is_pen_close(&edit) && self.core.session_mut().is_some_and(|session| context_bar::close_pen_draft(&edit, &mut self.canvas_input, session));
            if let Some(command) = command.filter(|_| !closed) {
                let _ = self.core.run(dtool_core::Scope::active(), command);
            }
        }
        if let Some(session) = self.core.session_mut() {
            self.fill_tool_bar.show_session(ui, session);
        }
    }

    pub fn open_guides_manager(&mut self) {
        if self.run_named(guides_dialog::SHOW_GUIDES_MANAGER, serde_json::json!({})).is_ok() {
            self.guides_dialog.open();
        }
    }


    pub fn open_preferences(&mut self) {
        self.prefs_dialog.load(&mut self.prefs, &mut self.ui_settings);
        self.prefs_dialog.open = true;
    }

    pub fn pref_command(
        &mut self,
        name: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, PrefsError> {
        self.prefs_dialog.run(&mut self.prefs, &mut self.ui_settings, name, params)
    }

    pub fn open_print_dialog(&mut self) {
        self.print_dialog.open = true;
    }
}

pub mod grid_dialog;
pub mod resize_canvas_dialog;
pub mod resize_document_dialog;
mod resize_pixel_art_dialog;
pub mod rulers;

pub mod tabs;

pub mod status_bar;
pub mod status_bar_stats;

pub mod shell;

pub mod shape_flyout;
pub mod tools_panel;

pub mod persona;
pub mod batch_dialog;
pub mod batch_host;
pub mod font_combo;
pub mod font_manager;
pub mod find_panel;
pub mod find_host;
pub mod spelling_panel;
pub mod spell_host;
pub mod macro_library_panel;

pub mod persona_bar;

pub mod stroke_panel;
pub mod studio_editor;

pub mod profile_dialogs;

pub mod slices_panel;

pub mod selection_ui;
mod selection_refine_dialog;
mod selection_modify_dialog;
mod selection_sampled_colour;

pub mod doc_setup_colour;

pub mod trace_dialog;
pub mod style_keys;
pub mod os_clipboard;

pub mod tools {
    pub mod fill_tool_bar;
}
pub(crate) mod update_indicator;
