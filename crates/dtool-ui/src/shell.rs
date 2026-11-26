use dtool_core::Session;
use dtool_protocol::{Command, CommandOutcome};
use serde_json::{Value, json};
use dtool_core::workspace::{ContextId, Workspace};
use crate::settings::UiSettings;
use crate::tabs;
use crate::status_bar::StatusBar;
use dtool_design::menu_item;
use dtool_strings::text;
use dtool_tools::move_tool::{nudge_args, NudgeDirection};
use crate::dock::{DockSide, Layout};
use crate::shortcuts::{Focus, Modifiers, ShortcutMap, TabAction};

pub const LAYER_SELECT_MENU_SOURCE: &str = "menudump_JMT02_r3.txt";

static EXPORT_DIALOG: std::sync::OnceLock<std::sync::Mutex<crate::dialogs::export::ExportDialog>> = std::sync::OnceLock::new();

pub fn export_dialog() -> &'static std::sync::Mutex<crate::dialogs::export::ExportDialog> {
    EXPORT_DIALOG.get_or_init(|| std::sync::Mutex::new(crate::dialogs::export::ExportDialog::new()))
}

fn export_action(id: Option<dtool_core::workspace::ContextId>, session: &Session, command: &Command) -> dtool_protocol::CommandResult {
    let mut dialog = export_dialog().lock().map_err(|_| dtool_protocol::CommandError::new("EXPORT_FAILED"))?;
    let previous_context = dialog.context.clone();
    let same_owner = dialog.owner().is_none() || dialog.owner() == id || command.name == crate::dialogs::export::UI_EXPORT_OPEN;
    if same_owner { dialog.set_session_context(session); }
    let reply = match id {
        Some(id) => dialog.run_for(id, &session.document, &command.name, &command.params),
        None => dialog.run(&session.document, &command.name, &command.params).map(|result| result.map_err(|error| error.to_command_error())),
    };
    match reply {
        Some(result) => { if result.is_err() { dialog.context = previous_context; } result },
        None => { dialog.context = previous_context; Err(dtool_protocol::CommandError::invalid_params()) },
    }
}

pub fn install_export_host() -> bool {
    dtool_core::commands::ui::register_export_action_for(export_action)
}

pub fn export_shortcut_pressed(ctx: &egui::Context, map: &ShortcutMap) -> bool {
    let bindings = map.bindings_for(crate::shortcuts::EXPORT_OPEN_COMMAND);
    if bindings.is_empty() {
        return crate::dialogs::export::open_shortcut_pressed(ctx);
    }
    ctx.input(|i| {
        bindings.iter().any(|binding| {
            egui::Key::from_name(&binding.key).is_some_and(|key| {
                i.key_pressed(key)
                    && i.modifiers.ctrl == binding.modifiers.ctrl
                    && i.modifiers.alt == binding.modifiers.alt
                    && i.modifiers.shift == binding.modifiers.shift
            })
        })
    })
}

pub const LAYER_SELECT_ITEMS: [&str; 13] = [
    "menu.layer.select.next",
    "menu.layer.select.previous",
    "menu.layer.select.parent",
    "menu.layer.select.top",
    "menu.layer.select.bottom",
    "",
    "menu.layer.select.all",
    "menu.layer.select.all_current",
    "menu.layer.select.deselect",
    "",
    "menu.layer.select.cycle_box",
    "menu.layer.select.set_box",
    "",
];

pub const LAYER_SELECT_SAME_ITEMS: [(&str, &str); 11] = [
    ("menu.layer.select.same.fill_colour", "FillColour"),
    ("menu.layer.select.same.stroke_colour", "StrokeColour"),
    ("menu.layer.select.same.fill_stroke_colour", "FillAndStrokeColour"),
    ("", ""),
    ("menu.layer.select.same.stroke_weight", "StrokeWeight"),
    ("menu.layer.select.same.transparency", "Transparency"),
    ("menu.layer.select.same.blend_mode", "BlendMode"),
    ("menu.layer.select.same.shape", "Shape"),
    ("", ""),
    ("menu.layer.select.same.width", "Width"),
    ("menu.layer.select.same.height", "Height"),
];

pub const NUDGE_KEYS: [(egui::Key, NudgeDirection); 4] = [
    (egui::Key::ArrowLeft, NudgeDirection::Left),
    (egui::Key::ArrowRight, NudgeDirection::Right),
    (egui::Key::ArrowUp, NudgeDirection::Up),
    (egui::Key::ArrowDown, NudgeDirection::Down),
];

pub const TAB_RULE: &str = "BP-DOC-C155";
pub const TAB_EVIDENCE: [&str; 5] = [
    "B13_tab_canvas_before",
    "B13_tab_canvas_ui_hidden",
    "B13_tab_canvas_ui_restored",
    "B13_tab_in_field_before",
    "B13_tab_in_field_after",
];

pub fn layer_select_command(key: &str, session: &Session) -> Option<Command> {
    let (name, params) = match key {
        "menu.layer.select.next" => ("selection.cycle", serde_json::json!({ "direction": "Next" })),
        "menu.layer.select.previous" => ("selection.cycle", serde_json::json!({ "direction": "Previous" })),
        "menu.layer.select.parent" => ("selection.exit", serde_json::json!({})),
        "menu.layer.select.all" => {
            let ids: Vec<u64> = session
                .document
                .objects()
                .iter()
                .filter(|obj| obj.pickable && !obj.locked)
                .map(|obj| obj.id.0)
                .collect();
            ("selection.set", serde_json::json!({ "ids": ids }))
        }
        "menu.layer.select.deselect" => ("selection.set", serde_json::json!({ "ids": [] })),
        _ => return None,
    };
    Some(Command { name: name.to_string(), params })
}

pub fn select_same_command(attribute: &str) -> Command {
    Command {
        name: "edit.select_same".to_string(),
        params: serde_json::json!({ "attribute": attribute }),
    }
}

pub fn nudge_command(direction: NudgeDirection, shift: bool) -> Command {
    let args = nudge_args(direction, shift);
    let mut params = serde_json::json!({ "dx": args.dx, "dy": args.dy });
    if let Some(ids) = args.ids {
        params["ids"] = serde_json::to_value(ids).unwrap_or(Value::Null);
    }
    Command {
        name: "selection.nudge".to_string(),
        params,
    }
}

pub fn nudge_from_input(ui: &egui::Ui) -> Option<Command> {
    if ui.ctx().memory(|memory| memory.focused().is_some()) {
        return None;
    }
    ui.input(|input| nudge_key(input).map(|(direction, shift)| nudge_command(direction, shift)))
}

pub(crate) fn nudge_key(input: &egui::InputState) -> Option<(NudgeDirection, bool)> {
    NUDGE_KEYS.iter().find_map(|(key, direction)| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key { key: pressed_key, pressed: true, modifiers, .. } if pressed_key == key =>
                Some((*direction, modifiers.shift)),
            _ => None,
        })
    })
}

fn menu_entry(ui: &mut egui::Ui, key: &str, command: Option<Command>, session: &Session) -> Option<Command> {
    let enabled = command.as_ref().map(|c| session.can_execute(c)).unwrap_or(false);
    let clicked = ui.add_enabled_ui(enabled, |ui| menu_item(ui, text(key))).inner.clicked();
    if clicked {
        ui.close();
        command
    } else {
        None
    }
}

pub fn layer_select_menu(ui: &mut egui::Ui, session: &Session) -> Option<Command> {
    let mut picked = None;
    ui.menu_button(text("menu.layer.select"), |ui| {
        for key in LAYER_SELECT_ITEMS {
            if key.is_empty() {
                ui.separator();
                continue;
            }
            if let Some(command) = menu_entry(ui, key, layer_select_command(key, session), session) {
                picked = Some(command);
            }
        }
        ui.menu_button(text("menu.layer.select.same"), |ui| {
            for (key, attribute) in LAYER_SELECT_SAME_ITEMS {
                if key.is_empty() {
                    ui.separator();
                    continue;
                }
                if let Some(command) = menu_entry(ui, key, Some(select_same_command(attribute)), session) {
                    picked = Some(command);
                }
            }
        });
    });
    picked
}

pub const CONTEXT_OBJECT: &str = "Object";
pub const CONTEXT_TEXT_EDITING: &str = "TextEditing";
pub const CONTEXT_PASTEBOARD: &str = "Pasteboard";
pub const PREFERENCES_MENU_ID: &str = "edit.settings";
pub const CONSOLE_MENU_ID: &str = "window.scripting.log_console";
pub const RESIZE_DOCUMENT_MENU_ID: &str = "document.setup.resize.document";
pub const CLOSE_MENU_ID: &str = "file.close";
pub const FONT_MANAGER_MENU_ID: &str = "document.font_manager";
pub const RESOURCE_MANAGER_MENU_ID: &str = "document.resource_manager";
pub const PEN_CLOSE_CURVE: &str = "pen.close_curve";
pub const CLOSE_ALL_MENU_ID: &str = "file.close_all";
pub(crate) const EXIT_MENU_ID: &str = "file.exit";
pub const SAVE_MENU_ID: &str = "file.save";
pub const DOCUMENT_SETUP_MENU_ID: &str = "document.setup.document_setup";
pub(crate) const CUSTOMISE_TOOLS_MENU_ID: &str = "view.tools.customise";
pub(crate) const CUSTOMISE_TOOLBAR_MENU_ID: &str = "view.top_toolbar.customise";

type TextBarStamp = (ContextId, u64, u64, Option<(u64, Option<u64>, usize, usize)>, Vec<u64>);

struct ZoomDrag {
    owner: ContextId,
    start: egui::Pos2,
    end: egui::Pos2,
    zoom: f32,
    scroll: [f32; 2],
    marquee: bool,
    moved: bool,
}

impl ZoomDrag {
    fn update(&mut self, end: egui::Pos2) {
        self.end = end;
        const DRAG_THRESHOLD: f32 = 6.0;
        self.moved |= self.start.distance_sq(end) > DRAG_THRESHOLD * DRAG_THRESHOLD;
    }

    fn apply(
        &self, view: &mut dtool_core::ViewState, rect: egui::Rect, finished: bool, limits: dtool_core::view::ZoomLimits,
    ) -> Option<egui::Rect> {
        let local = |pos: egui::Pos2| [pos.x - rect.min.x, pos.y - rect.min.y];
        if self.marquee {
            let end = self.end.clamp(rect.min, rect.max);
            if !finished {
                return Some(egui::Rect::from_two_pos(self.start, end));
            }
            dtool_tools::zoom_tool_marquee_with_limits(view, local(self.start), local(end), [rect.width(), rect.height()], limits);
        } else {
            const POINTS_PER_DOUBLING: f32 = 160.0;
            let factor = ((self.end.x - self.start.x) / POINTS_PER_DOUBLING).exp2();
            view.zoom = self.zoom;
            view.scroll = self.scroll;
            limits.zoom_about(view, self.zoom * factor, local(self.start));
        }
        None
    }
}

pub struct AppShell {
    last_drawn: Option<(ContextId, u64)>,
    last_snapshot: Value,
    theme_applied: Option<String>,
    pub snapping_dialog: crate::snapping_dialog::SnappingDialog,
    pub console_panel: crate::console::ConsolePanel,
    pub console_open: bool,
    pub rulers: crate::rulers::Rulers,
    pub guides_dialog: crate::guides_dialog::GuidesDialog,
    pub guides_owner: Option<ContextId>,
    pub layer_effects: crate::dialogs::layer_effects::LayerEffectsDialog,
    pub layer_effects_owner: Option<ContextId>,
    pub pending_context_dialog: Option<String>,
    pub blend_options: crate::dialogs::blend_options::BlendOptionsDialog,
    pub blend_options_owner: Option<ContextId>,
    tone_blend: Option<crate::dialogs::tone_blend::Dialog>,
    pub feature_finder: crate::feature_finder::FeatureFinder,
    pub prefs: dtool_core::prefs::Prefs,
    core_prefs_seen: Option<dtool_core::prefs::Prefs>,
    prefs_applied: Vec<dtool_core::ContextId>,
    dock_seen: u64,
    presented_dock_sizes: std::collections::BTreeMap<&'static str, (Option<f32>, f32)>,
    pub wells: crate::fill_stroke::FillStrokeWells,
    wells_stamp: Option<(ContextId, u64, u64, Vec<u64>)>,
    pub text_bar: crate::context::text_bar::TextBarState,
    pub picture_frame_bar: crate::picture_frame_bar::PictureFrameBar,
    text_bar_stamp: Option<TextBarStamp>,
    font_families: Option<Vec<String>>,
    pub font_preview: Option<String>,
    pub prefs_dialog: crate::prefs_dialog::PrefsDialog,
    pub canvas_size: [f32; 2],
    pub about: crate::about::AboutDialog,
    pub about_chrome: crate::dialog_chrome::DialogChrome,
    pub window_title_sent: Option<String>,
    pub client_title_bar: bool,
    pub context_target: &'static str,
    status_inputs: Option<(String, bool, bool, bool)>,
    refresh_count: u64,
    pub workspace: Workspace,
    pub persona_tools: Vec<crate::tools_panel::ToolId>,
    tool_palette_open: bool,
    tool_drag: Option<(crate::tools_panel::ToolId, egui::Pos2, bool)>,
    pub persona_toolbar: Vec<String>,
    persona_revision: u64,
    pub menu_persona: String,
    pub recent_files: crate::menu_tree::recent::RecentFiles,
    pub recovery_offer: crate::dialogs::RecoveryOfferState,
    pub grid_and_axis: crate::grid_dialog::GridAndAxisDialog,
    pub baseline_grid: crate::grid_dialog::BaselineGridDialog,
    pub resource_manager: crate::dialogs::resource_manager::ResourceManagerDialog,
    pub package_dialog: crate::dialogs::package::PackageDialogState,
    pub resize_document: crate::resize_document_dialog::ResizeDocumentDialog,
    pub resize_document_owner: Option<ContextId>,
    resize_canvas: Option<crate::resize_canvas_dialog::ResizeCanvasDialog>,
    resize_pixel_art: Option<crate::resize_pixel_art_dialog::ResizePixelArtDialog>,
    selection_refine: Option<crate::selection_refine_dialog::RefineDialog>,
    selection_modify: Option<crate::selection_modify_dialog::ModifyDialog>,
    selection_sampled_colour: Option<crate::selection_sampled_colour::SampledColourDialog>,
    pub settings: UiSettings,
    pub pending_close_id: Option<ContextId>,
    pub close_queue: std::collections::VecDeque<ContextId>,
    exit_review: Option<crate::dialogs::ExitReview>,
    exit_approved: bool,
    pub document_setup: crate::dialogs::document_setup::DocumentSetupDialog,
    pub document_setup_owner: Option<ContextId>,
    pub close_dialog: Option<crate::dialogs::CloseDocumentDialogState>,
    pub mcp_confirm: Option<(u64, crate::dialogs::McpConfirmDialogState)>,
    pub status_bar: StatusBar,
    pub layout: Layout,
    pub workspace_default_layout: Layout,
    pub slices_panel: crate::slices_panel::SlicesPanel,
    pub history_panel: crate::history_panel::HistoryPanel,
    pub snapshots_panel: crate::snapshots_panel::SnapshotsPanel,
    pub panel_registry: crate::panels::PanelRegistry,
    layout_persisted: String,
    pub shortcuts: ShortcutMap,
    shortcut_seen: u64,
    pub batch_host: crate::batch_host::BatchHost,
    pub font_manager: crate::font_manager::FontManagerState,
    pub font_replace: crate::font_manager::ReplaceFontState,
    font_manager_owner: Option<dtool_core::ContextId>,
    page_menu_dialog: Option<crate::menus::pages::Dialog>,
    pub find_host: crate::find_host::FindHost,
    pub spell_host: crate::spell_host::SpellHost,
    hidden_ui: bool,
    pub file_dialog: Option<crate::dialogs::FileDialogState>,
    #[cfg(windows)]
    print_dialog: Option<crate::print::dialog::PrintDialog>,
    pub new_document_dialog: Option<crate::dialogs::NewDocumentDialogState>,
    last_new_document: Option<crate::dialogs::NewDocumentDialogState>,
    template_browser: crate::home::HomeScreen,
    studio_windows: crate::studio_editor::StudioWindows,
    pub paste_special_dialog: Option<crate::dialogs::PasteSpecialDialogState>,
    pub navigation_tool: dtool_tools::NavigationTool,
    zoom_drag: Option<ZoomDrag>,
    pan_drag: Option<(dtool_core::workspace::ContextId, egui::PointerButton, egui::Pos2)>,
    pub locked_delete: crate::dialogs::locked_delete::LockedDeleteDialog,
    pub longdoc: crate::longdoc_host::LongdocHost,
    pub app_canvas: crate::app_canvas::AppCanvas,
    pub float_manager: crate::float::FloatManager,
    pub modal_blocking: bool,
    pub float_canvases: std::collections::HashMap<ContextId, crate::app_canvas::AppCanvas>,
    pub place_tool: crate::place_tool::PlaceTool,
    trace_dialog: crate::trace_dialog::TraceDialog,
    trace_owner: Option<dtool_core::workspace::ContextId>,
    compound_edit: Option<crate::dialogs::compound::CompoundEdit>,
    move_duplicate: Option<crate::dialogs::move_duplicate::MoveDuplicateDialog>,
    compound_canvas: crate::compound_canvas::CanvasEditor,
    pub preset_store: crate::presets::PresetStore,
    pub add_preset: crate::presets::AddPresetDialog,
    pub manage_presets: crate::presets::ManagePresetsDialog,
    pub frame: u64,
    pub spell_warm_requested: bool,
    pub input: crate::input::InputHandler,
    pub focus: Focus,
    pub canvas_id: Option<egui::Id>,
    prev_widget_focus: Option<egui::Id>,
    pub startup_errors: Vec<String>,
    pub dialogs: crate::dialogs::DialogRegistry,
    ime_owner: Option<(ContextId, String, bool)>,
    context_model_view: [f32; 2],
    context_model_zoom: f32,
    context_model_pen_points: usize,
    pub icon_cache: crate::icon_cache::IconCache,
    update_indicator: crate::update_indicator::UpdateIndicator,
    icon_ppp: f32,
    context_model: Option<(ContextId, u64, String, Vec<dtool_core::ObjectId>, crate::context_bar::ContextBarModel)>,
    pub align_popup: crate::context_bar::align::AlignmentPopup,
    pub align_popup_open: bool,
    brush_editor: Option<crate::brush_editor::Editor>,
    align_popup_key: Option<(ContextId, dtool_core::ObjectId)>,
}

impl AppShell {
    pub fn new() -> Self {
        let root=crate::settings::UiSettings::default_path().parent().unwrap_or(std::path::Path::new(".")).join("personas");
        Self::with_persona_directory(root)
    }

    pub fn with_persona_directory(dir:std::path::PathBuf)->Self {
        let last_layout_path = Layout::last_layout_path();
        let modified = |path: &std::path::Path| std::fs::metadata(path).and_then(|file| file.modified()).ok();
        let restore_last_layout = modified(&last_layout_path) > modified(&dir.join("state.json"));
        let _=crate::persona::install(crate::persona::PersonaStore{dir});
        let menu_missing = if crate::menu_tree::install() { Vec::new() } else { crate::menu_tree::missing_b11_keys() };
        dtool_tools::frame_text::install_filler();
        dtool_tools::frame_text::install_flow_ranges();
        dtool_tools::frame_text::install_canvas_text();
        dtool_export::macros::install();
        let _ = crate::context_menu::install();
        let _ = install_export_host();
        crate::canvas::frame_text_input::set_edit_route(crate::input::EditRoute::Core);
        let workspace_default_layout = Layout::load_file(&Layout::workspace_default_path())
            .unwrap_or_else(|_| crate::presets::default_layout(crate::presets::WorkspacePersona::Vector));
        let saved_layout = Layout::load_file(&last_layout_path).ok();
        if restore_last_layout && let Some(layout) = &saved_layout {
            let _ = crate::persona::save_live_dock(layout);
        }
        let mut layout = saved_layout.unwrap_or_else(|| workspace_default_layout.clone());
        let panel_registry = crate::panels::PanelRegistry::with_existing_bodies();
        crate::panels::register_named_panels(&mut layout, &panel_registry);
        let layout_persisted = if Layout::last_layout_path().exists() {
            serde_json::to_string(&layout).unwrap_or_default()
        } else { String::new() };

        let mut shell = Self {
            last_drawn: None,
            last_snapshot: Value::Null,
            theme_applied: None,
            snapping_dialog: Default::default(),
            console_panel: Default::default(),
            console_open: false,
            rulers: Default::default(),
            guides_dialog: Default::default(),
            guides_owner: None,
            layer_effects: Default::default(),
            layer_effects_owner: None,
            pending_context_dialog: None,
            blend_options: Default::default(),
            blend_options_owner: None,
            tone_blend: None,
            feature_finder: Default::default(),
            prefs: Default::default(),
            core_prefs_seen: None,
            prefs_applied: Vec::new(),
            dock_seen: 0,
            presented_dock_sizes: Default::default(),
            wells: Default::default(),
            wells_stamp: None,
            text_bar: Default::default(),
            picture_frame_bar: Default::default(),
            text_bar_stamp: None,
            font_families: None,
            font_preview: None,
            prefs_dialog: Default::default(),
            canvas_size: [800.0, 600.0],
            about: Default::default(),
            about_chrome: Default::default(),
            window_title_sent: None,
            client_title_bar: false,
            context_target: CONTEXT_PASTEBOARD,
            status_inputs: None,
            refresh_count: 0,
            persona_tools: Vec::new(),
            tool_palette_open: false,
            tool_drag: None,
            persona_toolbar: Vec::new(),
            persona_revision: 0,
            menu_persona: "vector".to_string(),
            recent_files: crate::menu_tree::recent::RecentFiles::at(crate::menu_tree::recent::default_path()),
            recovery_offer: crate::dialogs::RecoveryOfferState::scan(),
            grid_and_axis: crate::grid_dialog::GridAndAxisDialog::default(),
            baseline_grid: crate::grid_dialog::BaselineGridDialog::default(),
            resource_manager: crate::dialogs::resource_manager::ResourceManagerDialog::default(),
            package_dialog: crate::dialogs::package::PackageDialogState::default(),
            resize_document: crate::resize_document_dialog::ResizeDocumentDialog::new(),
            resize_document_owner: None,
            resize_canvas: None,
            resize_pixel_art: None,
            selection_refine: None,
            selection_modify: None,
            selection_sampled_colour: None,
            workspace: Workspace::new(std::sync::Arc::new(dtool_core::SharedServices {})),
            settings: UiSettings::default(),
            pending_close_id: None,
            exit_review: None,
            exit_approved: false,
            close_queue: Default::default(),
            document_setup: Default::default(),
            document_setup_owner: None,
            close_dialog: None,
            mcp_confirm: None,
            status_bar: StatusBar::new(),
            layout,
            workspace_default_layout,
            panel_registry,
            slices_panel: crate::slices_panel::SlicesPanel::default(),
            history_panel: crate::history_panel::HistoryPanel::default(),
            snapshots_panel: crate::snapshots_panel::SnapshotsPanel::default(),
            layout_persisted,
            shortcuts: ShortcutMap::load(),
            shortcut_seen: 0,
            batch_host: Default::default(),
            font_manager: crate::font_manager::FontManagerState::new(),
            font_replace: crate::font_manager::ReplaceFontState::new(),
            font_manager_owner: None,
            page_menu_dialog: None,
            find_host: Default::default(),
            spell_host: Default::default(),
            hidden_ui: false,
            file_dialog: None,
            #[cfg(windows)]
            print_dialog: None,
            new_document_dialog: None,
            last_new_document: crate::dialogs::NewDocumentDialogState::load_last_used(),
            template_browser: Default::default(),
            studio_windows: Default::default(),
            paste_special_dialog: None,
            navigation_tool: dtool_tools::NavigationTool::View,
            zoom_drag: None,
            pan_drag: None,
            locked_delete: crate::dialogs::locked_delete::LockedDeleteDialog::new(),
            longdoc: Default::default(),
            app_canvas: Default::default(),
            float_manager: Default::default(),
            modal_blocking: false,
            float_canvases: Default::default(),
            place_tool: Default::default(),
            trace_dialog: Default::default(),
            trace_owner: None,
            compound_edit: None,
            move_duplicate: None,
            compound_canvas: Default::default(),
            preset_store: crate::presets::PresetStore::load_startup(),
            add_preset: Default::default(),
            manage_presets: Default::default(),
            frame: 0,
            spell_warm_requested: false,
            input: crate::input::InputHandler::new(),
            focus: Focus::Canvas,
            canvas_id: None,
            prev_widget_focus: None,
            startup_errors: Vec::new(),
            dialogs: crate::dialogs::DialogRegistry::default(),
            ime_owner: None,
            context_model_view: [0.0, 0.0],
            context_model_zoom: 0.0,
            context_model_pen_points: 0,
            icon_cache: crate::icon_cache::IconCache::new(),
            update_indicator: crate::update_indicator::UpdateIndicator::default(),
            icon_ppp: 0.0,
            context_model: None,
            align_popup: crate::context_bar::align::AlignmentPopup::default(),
            align_popup_open: false,
            brush_editor: None,
            align_popup_key: None,
        };
        shell.note_menu_install(menu_missing);
        shell.install_core_prefs();
        shell.install_session_dock();
        let _ = crate::shortcuts::install(shell.shortcuts.clone());
        shell.recent_files.adopt_core_store(&mut shell.workspace, dtool_core::Workspace::recent_default_path());
        shell
    }

    pub fn note_menu_install(&mut self, missing: Vec<String>) {
        if missing.is_empty() {
            return;
        }
        eprintln!("dtool: menu provider not installed; missing string keys: {}", missing.join(", "));
        self.startup_errors.push(format!("menu_provider_missing: {}", missing.join(", ")));
        self.status_bar.set_hint_key("error.internal_error");
    }

    pub fn reconcile(&mut self, session: &Session, outcome: &CommandOutcome) {
        let Some(id) = self.workspace.active() else { return; };
        if self.last_drawn != Some((id, outcome.doc_version)) {
            self.refresh(session);
        }
    }

    pub fn refresh(&mut self, session: &Session) {
        let snapshot = session.query(None).unwrap_or(Value::Null);
        self.last_drawn = self.workspace.active().map(|id| (id, session.generation));
        self.render(snapshot);
    }

    fn render(&mut self, snapshot: Value) {
        self.last_snapshot = snapshot;
        self.app_canvas.invalidate();
        self.refresh_count += 1;
    }

    pub fn ai_connection(&self) -> std::sync::Arc<dtool_core::ai_connection::AiConnection> {
        self.workspace.ai_connection()
    }

    pub fn sync_mcp_switch(&self, prefs: &dtool_core::prefs::Prefs) -> bool {
        crate::ai_connection::sync(prefs, &self.workspace.ai_connection())
    }

    pub fn show_console(&mut self, ctx: &egui::Context) {
        if !self.console_open {
            return;
        }
        let mut open = self.console_open;
        let panel = &mut self.console_panel;
        let workspace = &mut self.workspace;
        egui::Window::new(text("panel.console")).open(&mut open).show(ctx, |ui| panel.show(ui, workspace));
        self.console_open = open;
    }

    pub fn context_bar_model(&mut self) -> Option<&crate::context_bar::ContextBarModel> {
        let id = self.workspace.active()?;
        let session = self.workspace.session()?;
        let view_size = self.canvas_size;
        let pen = &self.app_canvas.input.pen;
        let pen_points = pen.nodes.len();
        let zoom = session.view.zoom;
        use crate::canvas_input::{tool_kind, ToolKind};

        let fresh = !matches!(tool_kind(&session.active_tool), ToolKind::Gradient(_) | ToolKind::Transparency)
            && self.context_model_view == view_size
            && self.context_model_zoom == zoom
            && self.context_model_pen_points == pen_points
            && self.context_model.as_ref().is_some_and(|(cached, generation, tool, selection, model)| {
                use dtool_core::context_toolbar::{CURVE_ENERGY, PEN_CONSTRUCTION_SNAP};
                let toggles_current = [CURVE_ENERGY, PEN_CONSTRUCTION_SNAP].into_iter().all(|id| {
                    let displayed = model.item(id).and_then(|item| match item.control {
                        crate::context_bar::ContextControl::Toggle(value) => Some(value),
                        _ => None,
                    });
                    displayed == session.tool_options.flag(tool, id)
                });
                *cached == id && *generation == session.generation && *tool == session.active_tool
                    && *selection == session.selection.ids
                    && toggles_current
            });
        if !fresh {
            let mut model = crate::context_bar::ContextBarModel::stamped(session, id).with_view_size(session, view_size);
            if pen.active() {
                model.set_pen_close_enabled(pen.can_close());
            }
            self.context_model_view = view_size;
            self.context_model_zoom = zoom;
            self.context_model_pen_points = pen_points;
            self.context_model = Some((id, session.generation, session.active_tool.clone(), session.selection.ids.clone(), model));
        }
        self.context_model.as_ref().map(|(_, _, _, _, model)| model)
    }

    pub fn context_bar_stamp(&self) -> Option<(ContextId, u64)> {
        let id = self.workspace.active()?;
        self.workspace.session().map(|session| (id, session.generation))
    }

    pub fn apply_context_command(&mut self, mut command: Command) -> dtool_core::CommandResult {
        if command.name == dtool_core::commands::pixel_presets::EDITOR_OPEN {
            return self.open_brush_editor(None).map(|()| Value::Null);
        }
        if command.name == dtool_core::commands::selection_refine::REFINE {
            return self.open_selection_refine().map(|()| Value::Null);
        }
        if command.name == crate::context_bar::align::ALIGN_POPUP_COMMAND {
            self.align_popup_open = !self.align_popup_open;
            return Ok(Value::Null);
        }
        let pen = self.workspace.session().is_some_and(|session| {
            matches!(crate::canvas_input::tool_kind(&session.active_tool), crate::canvas_input::ToolKind::Pen { .. })
        });
        if pen && command.name == "path.close" {
            let edit = crate::context_bar::ContextEdit { id: crate::context_bar::PEN_CLOSE_CURVE.to_string(), value: Value::Null };
            let input = &mut self.app_canvas.input;
            if let Some(session) = self.workspace.session_mut() {
                if crate::context_bar::close_pen_draft(&edit, input, session) {
                    self.context_model = None;
                    return Ok(Value::Null);
                }
            }
        }
        if command.name == dtool_core::commands::path_nodes::PATH_CLOSE && command.params["preserve_transform"].is_null()
            && self.workspace.session().and_then(|session| {
                session.selection.ids.first().and_then(|id| session.document.get_object(*id))
            }).is_some_and(|object| dtool_core::geometry::object_matrix(object) != dtool_core::geometry::IDENTITY)
        {
            command.params["preserve_transform"] = serde_json::json!(true);
        }
        let result = self.run_and_reconcile(command);
        self.context_model = None;
        result
    }

    fn open_brush_editor(&mut self, preset: Option<String>) -> Result<(), dtool_core::CommandError> {
        self.brush_editor = Some(crate::brush_editor::Editor::open(&mut self.workspace, preset)?);
        Ok(())
    }

    pub fn show_align_popup(&mut self, ctx: &egui::Context) {
        use crate::context_bar::align::AlignTarget;
        let key = self.workspace.active().zip(self.workspace.session().and_then(|session| session.selection.explicit_key()));
        if key != self.align_popup_key {
            if key.is_some() {
                self.align_popup.target = AlignTarget::Key;
            } else if self.align_popup.target == AlignTarget::Key {
                self.align_popup.target = AlignTarget::Selection;
            }
            self.align_popup_key = key;
        }
        if !self.align_popup_open {
            return;
        }
        let count = self.workspace.session().map_or(0, |session| session.selection.ids.len());
        let mut open = true;
        let mut picked = None;
        let popup = &mut self.align_popup;
        let icons = &mut self.icon_cache;
        let mut window = egui::Window::new(text("menu.layer.arrange.align"))
            .id(egui::Id::new("dtool.align.window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false);
        if let Some(position) = ctx.data_mut(|data| data.remove_temp::<egui::Pos2>(egui::Id::new(crate::context_bar::align::ALIGN_LAUNCHER_POSITION))) {
            window = window.current_pos(position);
        }
        window.show(ctx, |ui| {
            picked = popup.show_with_icons(ui, count, icons);
        });
        self.align_popup_open = open;
        if let Some(command) = picked {
            if let Err(error) = self.run_and_reconcile(command) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
    }

    pub fn show_context_region(&mut self, ui: &mut egui::Ui) {
        self.show_align_popup(ui.ctx());
        if !self.context_toolbar_visible() { return; }
        let current = self.context_bar_stamp();
        let has_model = self.context_bar_model().is_some();
        use crate::canvas_input::{tool_kind, ToolKind};
        let text_tool = self.workspace.session().is_some_and(|session| {
            matches!(tool_kind(&session.active_tool), ToolKind::ArtText | ToolKind::FrameText)
        });
        let show_text = self.refresh_text_bar() || text_tool;
        let show_frame = self.refresh_picture_frame_bar();
        let show_node = self.workspace.session().is_some_and(|session| session.active_tool == "tool.node");
        let show_move_paint = self.workspace.session().is_some_and(|session| {
            tool_kind(&session.active_tool) == ToolKind::Move && !session.selection.ids.is_empty()
        });
        if show_node || show_move_paint { self.refresh_wells(); }
        let mut context_paint = None;
        if !show_text && self.font_preview.is_some() {
            let _ = self.font_preview_hover(None);
        }
        let mut picked = None;
        let style_picker_loaded = self.app_canvas.input.style_picker_loaded();
        let mut unload_style = false;
        let mut measure_action = None;
        let mut blend_action = None;
        let mut gradient_action = None;
        let mut brush_action = None;
        let mut artboard_actions = Vec::new();
        let mut setup_action = false;
        let setup_available = self.workspace.session().is_some_and(|session| {
            session.selection.ids.is_empty() || crate::artboard_bar::setup_available(session)
        });
        let mut warp_action = None;
        let mut corner_action = None;
        let context_bar = egui::Panel::top("dtool.shell.context_bar")
            .exact_size(dtool_design::size("bar.context.height"))
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(dtool_design::theme::surface(ui.ctx(), "context_bar")).inner_margin(4.0))
            .show(ui, |ui| ui.horizontal_centered(|ui| {
                let style = ui.style_mut();
                style.spacing.interact_size.y = dtool_design::size("bar.context.control.height");
                style.spacing.button_padding.y = 3.0;
                for kind in [egui::TextStyle::Body, egui::TextStyle::Button, egui::TextStyle::Monospace] {
                    if let Some(font) = style.text_styles.get_mut(&kind) {
                        font.size = dtool_design::size("bar.context.font_size");
                    }
                }
                if setup_available && !show_node {
                    setup_action = ui.button(text("menu.document.setup.document_setup")).clicked();
                    ui.separator();
                }
                if show_node && let Some(session) = self.workspace.session() {
                    self.app_canvas.input.nodes.sync_toolbar(session);
                    (picked, context_paint) = crate::context_bar::show_node_controls(
                        ui, session, &mut self.app_canvas.input.nodes, &self.wells, &mut self.icon_cache,
                    );
                    return;
                }
                if show_move_paint && let Some(session) = self.workspace.session() {
                    let can_paint = session.selection.ids.iter().any(|id| {
                        session.document.get_object(*id).is_some_and(|object| !object.locked)
                    });
                    context_paint = crate::context_bar::show_paint_wells(ui, session, &self.wells, can_paint);
                    ui.separator();
                }
                if let Some(session) = self.workspace.session().filter(|session| session.active_tool == "tool.blend") {
                    blend_action = self.app_canvas.input.show_blend_bar(ui, session);
                    return;
                }
                if let Some(session) = self.workspace.session().filter(|session| matches!(session.active_tool.as_str(), "tool.measure" | "tool.area")) {
                    measure_action = self.app_canvas.input.show_measure_bar(ui, session);
                    return;
                }
                if let Some(session) = self.workspace.session().filter(|session| session.active_tool == "tool.artboard") {
                    let viewport = self.workspace.active().and_then(|id| self.workspace.get(id))
                        .and_then(|context| context.session.view_canvas_size);
                    artboard_actions = crate::artboard_bar::show(ui, session, viewport);
                    return;
                }
                ui.horizontal_centered(|ui| {
                    if let Some(session) = self.workspace.session() {
                        crate::qr_editor::context_button(ui, session);
                        crate::qr_merge::context_button(ui, session);
                        warp_action = crate::menus::vector_warp::show_selected_group(ui, session, &mut self.app_canvas.input.warp_mesh);
                    }
                    if show_text {
                        self.show_art_text_direction(ui);
                    }
                    if show_frame {
                        self.show_picture_frame_controls(ui);
                    }
                    if self.workspace.session().is_some_and(|session| session.active_tool == "tool.style_picker") {
                        unload_style = dtool_design::button_with(ui, text("ctx.style_picker.unload"),
                            &dtool_design::ControlState::enabled(style_picker_loaded)).clicked();
                    }
                    if let Some(session) = self.workspace.session().filter(|session| {
                        !session.quick_mask.active
                            && matches!(tool_kind(&session.active_tool), ToolKind::Gradient(_) | ToolKind::Transparency)
                    }) {
                        gradient_action = self.app_canvas.input.show_gradient_controls(ui, session);
                    }
                    let show_brush_tip = self.workspace.session().filter(|session| {
                        tool_kind(&session.active_tool) == ToolKind::VectorBrush
                    }).is_some_and(|session| {
                        brush_action = crate::context_bar::show_vector_brush_tip(ui, session);
                        true
                    });
                    let rounded = self.workspace.session().and_then(|session| {
                        crate::rounded_corners::selected(session).map(|object| (session, object))
                    });
                    if let Some((session, object)) = rounded {
                        corner_action = crate::rounded_corners::show(ui, session, object);
                        ui.separator();
                    }
                    let model = self.context_model.as_ref().filter(|_| has_model).map(|(_, _, _, _, model)| {
                        let mask_gradient = self.workspace.session().is_some_and(|session| {
                            session.quick_mask.active && matches!(tool_kind(&session.active_tool), ToolKind::Gradient(_))
                        });
                        let focused_appearance = self.workspace.session().is_some_and(|session| {
                            matches!(tool_kind(&session.active_tool), ToolKind::Gradient(_))
                                && session.appearance_focus.is_some_and(|focus| session.selection.ids.as_slice() == [focus.node])
                        });
                        if mask_gradient || focused_appearance || show_text || rounded.is_some() || model.tool.as_deref() == Some("tool.move") {
                            let mut model = model.clone();
                            model.items.retain(|item| {
                                !(show_text && item.id.starts_with("text.")
                                    || item.id == "move.align" || item.id.starts_with("move.align.")
                                    || rounded.is_some() && item.id == dtool_core::context_toolbar::SHAPE_CORNER_RADIUS
                                    || (mask_gradient || focused_appearance) && item.id == "gradient.target")
                            });
                            if mask_gradient {
                                for item in model.items.iter_mut().filter(|item| item.id == "gradient.type") {
                                    if let crate::context_bar::ContextControl::Choice { value, values } = &mut item.control {
                                        values.retain(|kind| kind != "bitmap");
                                        if value == "bitmap" {
                                            *value = match model.tool.as_deref() {
                                                Some("tool.gradient.radial") => "radial",
                                                Some("tool.gradient.conical") => "conical",
                                                _ => "linear",
                                            }.to_owned();
                                        }
                                    }
                                }
                            }
                            std::borrow::Cow::Owned(model)
                        } else {
                            std::borrow::Cow::Borrowed(model)
                        }
                    }).filter(|model| !model.is_empty());
                    if show_text {
                        let model = model.map(std::borrow::Cow::into_owned);
                        picked = self.show_text_controls(ui, model.as_ref().map(|model| (model, current)));
                    } else if let Some(model) = model.as_deref() {
                        picked = crate::context_bar::show_toolbar_options(
                            ui, model, current, &mut self.icon_cache, show_frame || show_brush_tip,
                        );
                    }
                });
            }));
        self.toolbar_customisation_menu(ui, context_bar.response.rect, "context");
        if let Some(role) = context_paint {
            self.apply_tool_colour_action(crate::panels::colour::ToolColourAction::Edit(role));
        }
        if unload_style {
            self.app_canvas.input.clear_style_picker();
        }
        let merge_action = self.workspace.session().and_then(|session| crate::qr_merge::show(ui.ctx(), session));
        if let Some(command) = merge_action {
            let name = command.name.clone();
            let result = self.run_and_reconcile(command);
            crate::qr_merge::reply(ui.ctx(), &name, &result);
        }
        let qr_action = self.workspace.session().and_then(|session| crate::qr_editor::show(ui.ctx(), session));
        if setup_action {
            if let Some(session) = self.workspace.session().filter(|session| crate::artboard_bar::setup_available(session)) {
                crate::artboard_bar::open_setup(ui.ctx(), session);
            } else {
                self.open_document_setup();
            }
        }
        if let Some(session) = self.workspace.session() {
            let (global, setup) = crate::artboard_bar::show_setup(ui.ctx(), session);
            artboard_actions.extend(setup);
            if global { self.open_document_setup(); }
        }
        artboard_actions.extend(self.workspace.session().and_then(|session| crate::artboard_bar::custom_dialog(ui.ctx(), session)));
        for command in artboard_actions {
            if let Err(error) = self.run_and_reconcile(command) { self.status_bar.set_hint_key(&error.message_key); }
        }
        if let Some(command) = qr_action.or(corner_action).or(blend_action).or(measure_action).or(warp_action).or(brush_action) {
            if let Err(error) = self.run_and_reconcile(command) { self.status_bar.set_hint_key(&error.message_key); }
        }
        if let Some(command) = gradient_action.or(picked) {
            let tracks_zoom = command.name == crate::context_bar::OPTION_SET && command.params["id"] == "zoom.level";
            let previous_zoom = tracks_zoom.then(|| self.workspace.session().map(|session| session.view.zoom)).flatten();
            match self.apply_context_pick(command) {
                Ok(_) if previous_zoom.is_some_and(|zoom| self.workspace.session().is_some_and(|session| session.view.zoom != zoom)) => {
                    self.app_canvas.preview_zoom(ui);
                }
                Err(error) => self.status_bar.set_hint_key(&error.message_key),
                _ => {}
            }
        }
    }

    pub fn refresh_text_bar(&mut self) -> bool {
        let Some(id) = self.workspace.active() else {
            self.text_bar = Default::default();
            self.text_bar_stamp = None;
            if self.font_preview.take().is_some() {
                dtool_tools::dtool_text::font_preview::cancel_font_preview();
            }
            return false;
        };
        let Some(context) = self.workspace.get_mut(id) else {
            return false;
        };
        let session = &mut context.session;
        let stamp: TextBarStamp = (
            id,
            session.generation,
            session.document.version,
            session.text_edit.as_ref().map(|edit| (edit.story.0, edit.object.map(|id| id.0), edit.caret, edit.anchor)),
            session.selection.ids.iter().map(|object| object.0).collect(),
        );
        if self.text_bar_stamp.as_ref() != Some(&stamp) {
            let state = crate::context::text_bar::TextBarState::load(session).unwrap_or_default();
            let changed_target = self.text_bar.targets != state.targets
                || self.text_bar_stamp.as_ref().is_some_and(|old| old.0 != id);
            if changed_target && self.font_preview.take().is_some() {
                dtool_tools::dtool_text::font_preview::cancel_font_preview();
            }
            self.text_bar = state;
            self.text_bar_stamp = Some(stamp);
        }
        self.text_bar.has_controls()
    }

    pub fn text_bar_edit(&mut self, edit: &crate::context::text_bar::TextBarEdit) -> Result<bool, dtool_core::CommandError> {
        if !self.refresh_text_bar() {
            return Ok(false);
        }
        if matches!(edit, crate::context::text_bar::TextBarEdit::Family(_)) {
            self.font_preview_hover(None)?;
        }
        let session = self.workspace.session_mut().ok_or_else(dtool_core::CommandError::no_document)?;
        let applied = self.text_bar.apply(session, edit)?;
        if applied {
            self.reconcile_active();
            self.text_bar_stamp = None;
            self.refresh_text_bar();
        }
        Ok(applied)
    }

    pub fn font_preview_hover(&mut self, family: Option<&str>) -> Result<bool, dtool_core::CommandError> {
        if self.font_preview.as_deref() == family {
            return Ok(false);
        }
        let command = match family {
            Some(family) => self.text_bar.preview_command(family),
            None => Some(dtool_core::Command::with_params("text.font.preview.cancel", serde_json::json!({}))),
        };
        let Some(command) = command else { return Ok(false); };
        let session = self.workspace.session_mut().ok_or_else(dtool_core::CommandError::no_document)?;
        let result = session.run(&command);
        self.font_preview = match (&result, family) {
            (Ok(_), Some(family)) => Some(family.to_string()),
            _ => None,
        };
        self.reconcile_active();
        result.map(|_| true)
    }

    fn show_text_controls(
        &mut self,
        ui: &mut egui::Ui,
        trailing: Option<(&crate::context_bar::ContextBarModel, Option<crate::context_bar::Stamp>)>,
    ) -> Option<Command> {
        let families = self
            .font_families
            .get_or_insert_with(|| dtool_tools::dtool_text::FontLibrary::shared().families().into_iter().map(str::to_owned).collect())
            .clone();
        let (edit, hover, command) = self.text_bar.ui_with_hover_icons(ui, &families, &mut self.icon_cache, trailing);
        if edit.is_none() {
            let hovered = match hover {
                crate::context::text_bar::FontHover::Open(family) => family,
                crate::context::text_bar::FontHover::Closed => None,
            };
            if let Err(error) = self.font_preview_hover(hovered.as_deref()) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
        if let Some(edit) = edit {
            if let Err(error) = self.text_bar_edit(&edit) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
        command
    }

    fn show_art_text_direction(&mut self, ui: &mut egui::Ui) {
        use dtool_core::context_toolbar::{ART_TEXT_VERTICAL, OPTION_SET};
        let Some(session) = self.workspace.session() else { return };
        let target = {
            let id = session.text_edit.as_ref().and_then(|edit| edit.object).or_else(|| {
                let [id] = session.selection.ids.as_slice() else { return None };
                Some(*id)
            });
            id.and_then(|id| session.document.get_object(id))
                .filter(|object| matches!(object.kind, dtool_core::ObjectKind::ArtText { .. }))
        };
        let (vertical, enabled, command) = if let Some(object) = target {
            let vertical = object.is_vertical_art_text();
            (vertical, !object.locked, Command::with_params("text.art.vertical.set",
                serde_json::json!({ "id": object.id.0, "vertical": !vertical })))
        } else if let Some(vertical) = session.tool_options.flag(&session.active_tool, ART_TEXT_VERTICAL) {
            (vertical, true, Command::with_params(OPTION_SET,
                serde_json::json!({ "id": ART_TEXT_VERTICAL, "value": !vertical })))
        } else {
            return;
        };
        let label = text("context.text.vertical");
        let button = ui.add_enabled(enabled, egui::Button::new(label).selected(vertical));
        button.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, vertical, label));
        if button.clicked() && let Err(error) = self.run_and_reconcile(command) {
            self.status_bar.set_hint_key(&error.message_key);
        }
        ui.separator();
    }

    pub fn refresh_picture_frame_bar(&mut self) -> bool {
        match self.workspace.session() {
            Some(session) => self.picture_frame_bar.reload(session),
            None => self.picture_frame_bar = Default::default(),
        }
        self.picture_frame_bar.target().is_some()
    }

    pub fn picture_frame_bar_edit(&mut self, edit: crate::picture_frame_bar::FrameBarEdit) -> dtool_core::CommandResult {
        if !self.refresh_picture_frame_bar() {
            return Err(dtool_core::CommandError::invalid_params());
        }
        if edit == crate::picture_frame_bar::FrameBarEdit::PickImage {
            let object = self.picture_frame_bar.target().ok_or_else(dtool_core::CommandError::invalid_params)?;
            self.open_file_dialog(crate::dialogs::FileDialogMode::ReplaceImage(object.0));
            return Ok(serde_json::json!({ "dialog": "frame.replace_image" }));
        }
        let session = self.workspace.session_mut().ok_or_else(dtool_core::CommandError::no_document)?;
        let result = self.picture_frame_bar.apply(session, edit);
        self.reconcile_active();
        result
    }

    fn show_picture_frame_controls(&mut self, ui: &mut egui::Ui) {
        let edit = self.picture_frame_bar.ui(ui);
        if let Some(edit) = edit {
            if let Err(error) = self.picture_frame_bar_edit(edit) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
    }

    fn refresh_wells(&mut self) {
        use crate::fill_stroke::{FillStrokeWells, PaintOwners};
        let Some(context) = self.workspace.active().and_then(|id| self.workspace.get(id)) else {
            self.wells = FillStrokeWells::default();
            self.wells_stamp = None;
            return;
        };
        let session = &context.session;
        let stamp = (context.id, session.generation, session.document.version,
            session.selection.ids.iter().map(|id| id.0).collect());
        if self.wells_stamp.as_ref() == Some(&stamp) {
            return;
        }
        let mut paints: Vec<_> = session.selection.ids.iter()
            .filter_map(|id| session.document.get_object(*id))
            .map(|object| (object.fill.clone(), object.stroke.clone()))
            .collect();
        let owners: Vec<_> = paints.iter_mut().map(|(fill, stroke)| PaintOwners {
            source: Some(fill), frame: None, cell: None, edge: Some(stroke), glyph: None,
        }).collect();
        self.wells = FillStrokeWells::new_from_selection(&owners);
        self.wells.set_display(Some(&session.document), None);
        self.wells_stamp = Some(stamp);
    }

    fn show_tool_paint_editor(&mut self, ctx: &egui::Context) {
        if self.wells.active_popup.is_none() {
            return;
        }
        let mut open = true;
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new(text("colour.tool.edit_paint"))
            .id(egui::Id::new("dtool.shell.tool_paint"))
            .open(&mut open)
            .default_pos(ctx.content_rect().left_top() + egui::vec2(44.0, 160.0))
            .default_width(284.0)
            .vscroll(true)
            .show(ctx, |ui| {
                self.wells.show_popup(ui);
                ui.horizontal(|ui| {
                    confirmed = dtool_design::button(ui, text("dialog.ok")).clicked();
                    cancelled = dtool_design::button(ui, text("dialog.cancel")).clicked();
                });
            });
        if confirmed {
            let command = self.workspace.session().and_then(|session| {
                self.wells.commit_state(&session.document, &session.selection.ids)
            });
            if let Some(command) = command {
                let _ = self.wells_command(command);
            }
        } else if !open || cancelled {
            self.wells.cancel_state();
        }
    }

    pub fn wells_bar_action(&mut self, action: &str) -> Option<dtool_core::CommandResult> {
        let session = self.workspace.session()?;
        let command = self.wells.bar_command(action, &session.selection.ids)?;
        Some(self.wells_command(command))
    }

    fn wells_command(&mut self, command: Command) -> dtool_core::CommandResult {
        let result = self.run_and_reconcile(command);
        self.wells_stamp = None;
        if let Err(error) = &result {
            self.status_bar.set_hint_key(&error.message_key);
        }
        result
    }

    pub fn apply_context_pick(&mut self, mut command: Command) -> dtool_core::CommandResult {
        if command.name == crate::context_bar::OPTION_SET
            && command.params["id"] == "gradient.type"
            && command.params["value"] == "bitmap"
            && command.params["params"]["path"].is_null()
        {
            #[cfg(windows)]
            {
                let path = crate::dialogs::pick_file_path_with_extensions(
                    text("ctx.gradient.bitmap.open"), text("dialog.file.filter.image"),
                    &crate::place_tool::PLACE_IMAGE_EXTENSIONS, None,
                ).map_err(|_| dtool_core::CommandError::new("IO.OTHER"))?;
                let Some(path) = path else { return Ok(Value::Null) };
                let path = path.to_str().ok_or_else(dtool_core::CommandError::invalid_params)?;
                command.params["params"] = json!({"path": path});
            }
            #[cfg(not(windows))]
            return Err(dtool_core::CommandError::not_implemented());
        }
        let view_size = self.canvas_size;
        if command.name == crate::context_bar::VIEW_ZOOM_FIT {
            command.params = crate::context_bar::action_params(crate::context_bar::VIEW_ZOOM_FIT, view_size);
        } else if view_size.iter().all(|side| side.is_finite() && *side > 0.0) {
            if matches!(command.name.as_str(), "view.zoom.in" | "view.zoom.out" | "view.zoom.actual")
                && command.params.is_null()
            {
                command.params = json!({ "view_size": view_size });
            } else if command.name == crate::context_bar::OPTION_SET && command.params["id"] == "zoom.level" {
                command.params["params"] = json!({ "view_size": view_size });
            }
        }
        self.apply_context_command(command)
    }

    pub fn start_release_check(&mut self, ctx: &egui::Context) {
        self.update_indicator.start(ctx);
    }

    /// The same header remains present before a document is opened.
    pub fn show_persona_header(&mut self, ui: &mut egui::Ui) {
        let current = crate::persona::host_action(&Command::new("ui.persona.query")).ok();
        if let Some(id) = current.as_ref().and_then(|v| v["id"].as_str()) {
            self.menu_persona = id.to_string();
        }
        if !self.top_toolbar_visible() { return; }
        let home_open = self.workspace.active().is_none() && self.new_document_dialog.is_some()
            && self.file_dialog.is_none() && self.exit_review.is_none();
        let mut action = None;
        let mut toolbar_edit = None;
        let header = egui::Panel::top("dtool.shell.persona_header")
            .exact_size(dtool_design::size("bar.toolbar.height"))
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(dtool_design::theme::surface(ui.ctx(), "bars")).inner_margin(4.0))
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = dtool_design::size("bar.toolbar.control.height");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.update_indicator.show(ui, &mut self.icon_cache, home_open);
                    action = crate::persona_bar::workspace_icons(ui, self.workspace.session(), &mut self.icon_cache);
                    if self.menu_persona == "pixel"
                        && let Some(session) = self.workspace.session()
                        && let Some(command) = crate::persona_bar::quick_mask_icons(ui, session, &mut self.icon_cache)
                    {
                        action = Some(command);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let persona_action = crate::persona_bar::pick_strip_icons(ui, current.as_ref(), &mut self.icon_cache);
                        if persona_action.is_some() {
                            action = persona_action;
                        }
                        ui.separator();
                        if let Some(session) = self.workspace.session() {
                            if let Some(command) = crate::persona_bar::view_mode_switch(ui, session, &mut self.icon_cache) {
                                action = Some(command);
                            }
                            ui.separator();
                        }
                        let toolbar = current.as_ref().and_then(|value| value.pointer("/layout/toolbar"))
                            .and_then(|value| serde_json::from_value::<Vec<String>>(value.clone()).ok())
                            .unwrap_or_else(|| crate::persona::DEFAULT_TOOLBAR.map(String::from).into());
                        let draft = self.studio_windows.toolbar_draft(&self.menu_persona);
                        let items = draft.as_deref().map_or(toolbar.as_slice(), Vec::as_slice);
                        let (toolbar_action, drop) = crate::persona_bar::toolbar_icons(
                            ui, self.workspace.session(), items, &mut self.icon_cache, draft.is_some(),
                        );
                        if let Some(command) = toolbar_action {
                            action = Some(command);
                        }
                        toolbar_edit = drop;
                    });
                });
            });
        if let Some(draft) = self.studio_windows.toolbar_draft(&self.menu_persona) {
            let outside = ui.input(|input| input.pointer.primary_released()
                && input.pointer.interact_pos().is_some_and(|pos| !header.response.rect.contains(pos)));
            if toolbar_edit.is_none() && outside
                && let Some(item) = egui::DragAndDrop::payload::<crate::studio_editor::ToolbarCatalogItem>(ui.ctx())
                && item.placed
            {
                egui::DragAndDrop::take_payload::<crate::studio_editor::ToolbarCatalogItem>(ui.ctx());
                toolbar_edit = Some(crate::persona_bar::ToolbarEdit::Remove(item.id));
            }
            if let Some(edit) = toolbar_edit {
                edit.apply(draft);
            }
        }
        self.toolbar_customisation_menu(ui, header.response.rect, "header");
        if let Some(command) = action {
            if command.name == "ui.persona.set" && command.params["id"].as_str() == Some("home") {
                self.open_new_document_dialog();
                return;
            }
            if !matches!(command.name.as_str(), dtool_core::commands::toggles::SNAP_TOGGLE | dtool_core::commands::view_flags::VIEW_FLAGS_SET) {
                self.commit_ime_before_switch();
            }
            if let Some(bridge) = crate::menu_tree::bridge_action(&command.name) {
                if let Err(error) = self.run_bridge_action(bridge) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
                return;
            }
            match command.name.as_str() {
                "doc.new" => { self.open_new_document_dialog(); return; }
                "doc.open" => { self.browse_document(); return; }
                "doc.save" => { self.save_active_document(); return; }
                "ui.export.open" => { self.open_export_dialog(); return; }
                crate::context_bar::align::ALIGN_POPUP_COMMAND => {
                    let _ = self.apply_context_command(command);
                    return;
                }
                crate::guides_dialog::SHOW_GUIDES_MANAGER | crate::guides_dialog::MARGINS_AND_GUIDES => { self.open_guides_manager(); return; }
                crate::snapping_dialog::MENU_ID => { self.snapping_dialog.open(); return; }
                _ => {}
            }
            let result = if self.workspace.session().is_some() {
                self.run_and_reconcile(command)
            } else {
                crate::persona::host_action(&command).inspect(|_| {
                    if let Some(layout) = crate::persona::live_dock() { self.layout = layout; }
                })
            };
            if let Err(error) = result { self.status_bar.set_hint_key(&error.message_key); }
        }
    }

    pub fn tools_column_visible(&self) -> bool {
        (self.settings.toolbars.tools
            || (self.tool_palette_open && self.workspace.session().is_some()))
            && !self.ui_hidden()
    }

    pub fn context_toolbar_visible(&self) -> bool {
        self.settings.toolbars.context && !self.ui_hidden()
    }

    pub fn top_toolbar_visible(&self) -> bool {
        self.settings.toolbars.top && !self.ui_hidden()
    }

    pub fn show_tools_column(&mut self, ui: &mut egui::Ui) {
        if !self.tools_column_visible() { return; }
        let has_document = self.workspace.session().is_some();
        let active = self.workspace.session().map(|session| session.active_tool.clone()).unwrap_or_default();
        let host_tools = if has_document { None } else { crate::persona::live_tools() };
        let tools = if has_document { Some(self.persona_tools.as_slice()) } else { host_tools.as_deref() };
        let mut picked = None;
        let mut colour_action = None;
        let mut rows = Vec::new();
        let (mut columns, standalone) = crate::persona::live_tool_appearance();
        let column = egui::Panel::left("dtool.shell.tools")
            .exact_size(dtool_design::size("bar.tools.width") * f32::from(columns))
            .resizable(false)
            .frame(egui::Frame::new().fill(dtool_design::theme::surface(ui.ctx(), "bars")).inner_margin(4.0))
            .show(ui, |ui| {
                let colour_height = if has_document { crate::panels::colour::tool_swatches_height(ui) } else { 0.0 };
                let tools_height = (ui.available_height() - colour_height).max(0.0);
                egui::ScrollArea::vertical().id_salt("tool.column").max_height(tools_height)
                    .auto_shrink([false, true]).show_viewport(ui, |ui, viewport| {
                        picked = ui.add_enabled_ui(has_document, |ui| {
                            crate::tools_panel::editable_column(ui, &active, &mut self.icon_cache, tools, (columns.into(), &standalone), &mut rows)
                        }).inner;
                        if has_document && let Some((_, start, _)) = self.tool_drag.as_ref() {
                            crate::tools_panel::scroll_dragged_tool(ui, viewport, *start);
                        }
                    });
                if has_document {
                    ui.separator();
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        colour_action = self.panel_registry.colour.tool_swatches(ui, &self.workspace);
                    });
                }
            }).response.rect;
        self.toolbar_customisation_menu(ui, column, "tools");
        if has_document && self.edit_tools_column(ui.ctx(), column, &rows, &mut columns, &standalone) {
            picked = None;
        }
        self.refresh_wells();
        if let Some(action) = colour_action {
            self.apply_tool_colour_action(action);
        }
        if let Some(tool) = picked {
            if let Err(error) = self.pick_tool(tool) { self.status_bar.set_hint_key(&error.message_key); }
        }
    }

    fn toolbar_customisation_menu(&mut self, ui: &mut egui::Ui, rect: egui::Rect, bar: &str) {
        let ctx = ui.ctx();
        let id = egui::Id::new(("dtool.toolbar.context_menu", bar));
        let trigger = ctx.input(|input| {
            input.pointer.button_clicked(egui::PointerButton::Secondary)
                .then(|| input.pointer.interact_pos()).flatten()
        }).filter(|pos| rect.contains(*pos) && ctx.layer_id_at(*pos) == Some(ui.layer_id()));
        if let Some(position) = trigger {
            ctx.data_mut(|data| data.insert_temp(id, position));
        }
        let Some(position) = ctx.data(|data| data.get_temp::<egui::Pos2>(id)) else { return };
        egui::Popup::new(id, ctx.clone(), position, ui.layer_id())
            .kind(egui::PopupKind::Menu)
            .style(egui::containers::menu::menu_style)
            .open_memory(trigger.is_some().then_some(true.into()))
            .show(|ui| {
                use crate::menu_tree::command_menu_item;
                crate::menu_tree::configure_command_menu(ui);
                ui.set_width(230.0);
                if command_menu_item(ui, text("studio.toolbar.customise"), true).clicked() {
                    crate::persona_bar::request_studio(ctx, crate::studio_editor::StudioRequest::Toolbar);
                    ui.close();
                }
                if command_menu_item(ui, text("tools.customise.title"), self.workspace.session().is_some()).clicked() {
                    if let Err(error) = self.open_tool_palette(ctx) {
                        self.status_bar.set_hint_key(&error.message_key);
                    }
                    ui.close();
                }
            });
    }

    fn edit_tools_column(&mut self, ctx: &egui::Context, column: egui::Rect,
        rows: &[(crate::tools_panel::ToolId, egui::Rect)], columns: &mut u8,
        standalone: &[crate::tools_panel::ToolId]) -> bool
    {
        use crate::tools_panel::ToolId;
        let mut candidate = self.persona_tools.clone();
        let old_columns = *columns;
        let groups = crate::tools_panel::default_flyouts();
        let mut pinned = standalone.to_vec();
        let mut available = Vec::new();
        let mut inserted = None;
        if self.tool_palette_open {
            let mut bounds = ctx.content_rect();
            bounds.min.x = (column.right() + 8.0).min(bounds.max.x - 100.0).max(bounds.min.x);
            egui::Window::new(text("tools.customise.title")).id(egui::Id::new("dtool.tools.customise"))
                .open(&mut self.tool_palette_open).default_pos(column.right_top() + egui::vec2(8.0, 0.0))
                .default_width(440.0)
                .constrain_to(bounds)
                .show(ctx, |ui| {
                    ui.label(text("tools.customise.hint"));
                    ui.horizontal(|ui| {
                        ui.label(text("tools.customise.columns"));
                        ui.add(egui::DragValue::new(columns).range(1..=4));
                        if ui.button(text("pref.reset")).clicked() && let Some(defaults) = crate::persona::reset_tools() {
                            candidate = defaults;
                            pinned.clear();
                            *columns = 1;
                        }
                    });
                    let search_id = ui.id().with("tool.catalog.search");
                    let mut search = ui.data_mut(|data| data.get_temp::<String>(search_id)).unwrap_or_default();
                    let search_changed = ui.add(egui::TextEdit::singleline(&mut search)
                        .hint_text(text("pref.search")).desired_width(220.0)).changed();
                    let filter = search.trim().to_lowercase();
                    if search_changed {
                        ui.data_mut(|data| data.insert_temp(search_id, search));
                    }
                    let scroll = egui::ScrollArea::vertical().max_height(280.0);
                    let scroll = if search_changed { scroll.vertical_scroll_offset(0.0) } else { scroll };
                    scroll.show(ui, |ui| {
                        let mut any_match = false;
                        ui.horizontal_wrapped(|ui| {
                            for id in crate::tools_panel::listed_tool_ids() {
                                if !filter.is_empty() && !id.contains(&filter)
                                    && !text(id).to_lowercase().contains(&filter)
                                {
                                    continue;
                                }
                                let tool = ToolId(id);
                                let grouped = groups.iter().any(|group| candidate.contains(&group.parent) && group.children.contains(&tool));
                                let placed = candidate.contains(&tool) && (!grouped || pinned.contains(&tool));
                                any_match = true;
                                let button = crate::tools_panel::palette_button(ui, &mut self.icon_cache, id, placed);
                                if button.enabled() {
                                    available.push((tool.clone(), button.rect.intersect(ui.clip_rect())));
                                    if button.clicked() { inserted = Some((tool, None, true)); }
                                }
                            }
                        });
                        if !any_match {
                            ui.label(text("tools.customise.empty"));
                        }
                    });
                });
        }
        let (down, up, hover, held) = ctx.input(|input| {
            let mut down = None;
            let mut up = None;
            for event in &input.events {
                if let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = event {
                    if *pressed { down = Some(*pos); } else { up = Some(*pos); }
                }
            }
            (down, up, input.pointer.hover_pos(), input.pointer.primary_down())
        });
        let row_at = |point| rows.iter().find(|(_, rect)| rect.contains(point));
        if let Some(point) = down {
            self.tool_drag = available.iter().find(|(_, rect)| rect.contains(point)).map(|(id, _)| (id.clone(), point, true))
                .or_else(|| row_at(point).map(|(id, _)| (id.clone(), point, false)));
        }
        let mut removed = None;
        let mut was_drag = false;
        let after = |point: egui::Pos2, rect: egui::Rect| {
            if *columns > 1 { point.x > rect.center().x } else { point.y > rect.center().y }
        };
        if let Some((id, start, from_palette)) = self.tool_drag.as_ref()
            && let Some(point) = up.or(hover).filter(|point| point.distance(*start) > 5.0)
        {
            let target = row_at(point).map(|(tool, rect)| (tool.clone(), after(point, *rect)));
            if up.is_some() {
                was_drag = true;
                if column.contains(point) {
                    if target.as_ref().is_none_or(|(target, _)| target != id) {
                        inserted = Some((id.clone(), target, *from_palette));
                    }
                } else if self.tool_palette_open && !from_palette {
                    removed = Some(id.clone());
                }
            } else if let Some((_, rect)) = row_at(point) {
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dtool.tools.drop")));
                let stroke = egui::Stroke::new(2.0, crate::canvas_input::SELECTION_BLUE);
                if *columns > 1 {
                    painter.vline(if after(point, *rect) { rect.right() } else { rect.left() }, rect.y_range(), stroke);
                } else {
                    painter.hline(rect.x_range(), if after(point, *rect) { rect.bottom() } else { rect.top() }, stroke);
                }
            }
        }
        if up.is_some() || !held { self.tool_drag = None; }
        if let Some(id) = removed {
            let group = groups.iter().find(|group| group.parent == id);
            let in_flyout = groups.iter().any(|group| candidate.contains(&group.parent) && group.children.contains(&id));
            pinned.retain(|tool| tool != &id);
            if !in_flyout {
                candidate.retain(|tool| tool != &id && group.is_none_or(|group| !group.children.contains(tool) || pinned.contains(tool)));
            }
        }
        if let Some((id, target, from_palette)) = inserted {
            let adding_group = from_palette && !candidate.contains(&id);
            if from_palette && !pinned.contains(&id) && groups.iter().any(|group| group.children.contains(&id)) {
                pinned.push(id.clone());
            }
            candidate.retain(|tool| tool != &id);
            let index = target.and_then(|(target, after)| candidate.iter().position(|tool| *tool == target)
                .map(|index| index + usize::from(after))).unwrap_or(candidate.len());
            candidate.insert(index, id.clone());
            if adding_group && let Some(group) = groups.iter().find(|group| group.parent == id)
                && !group.children.iter().any(|tool| candidate.contains(tool) && !pinned.contains(tool))
            {
                let missing: Vec<_> = group.children.iter().filter(|tool| !candidate.contains(tool)).cloned().collect();
                candidate.splice(index + 1..index + 1, missing);
            }
        }
        if candidate != self.persona_tools || *columns != old_columns || pinned != standalone {
            match crate::persona::save_live_tools(&candidate, *columns, &pinned) {
                Ok(()) => self.persona_tools = candidate,
                Err(error) => self.status_bar.set_hint_key(&error.message_key),
            }
        }
        was_drag
    }

    fn apply_tool_colour_action(&mut self, action: crate::panels::colour::ToolColourAction) {
        use crate::fill_stroke::{PaintOwners, PAINT_DEFAULT};
        use dtool_core::paint::PaintRole;
        use crate::panels::colour::{ColourTarget, ToolColourAction};
        let panel = &mut self.panel_registry.colour;
        match action {
            ToolColourAction::Open(target) => {
                panel.set_target(target);
                crate::panels::open_panel(&mut self.layout, &crate::dock::PanelId::Colour);
            }
            ToolColourAction::Edit(target) => {
                let Some(object) = self.workspace.session().and_then(|session| {
                    session.selection.ids.first().and_then(|id| session.document.get_object(*id))
                }) else {
                    return;
                };
                let mut fill = object.fill.clone();
                let mut stroke = object.stroke.clone();
                let owners = PaintOwners {
                    source: Some(&mut fill), frame: None, cell: None, edge: Some(&mut stroke), glyph: None,
                };
                let role = match target { ColourTarget::Fill => PaintRole::SourceObject, ColourTarget::Stroke => PaintRole::Edge };
                self.wells.open_popup_for(&owners, role);
            }
            ToolColourAction::Clear(target) => {
                panel.clear_paint(&mut self.workspace, target);
                self.reconcile_active();
            }
            ToolColourAction::Swap => {
                panel.swap_paints(&mut self.workspace);
                self.reconcile_active();
            }
            ToolColourAction::Defaults => {
                if self.wells_bar_action(PAINT_DEFAULT).is_none() {
                    let defaults = dtool_core::defaults::ObjectDefaults::default();
                    dtool_core::commands::defaults::set_paints(Some(defaults.fill), Some(defaults.stroke.paint));
                }
            }
        }
    }

    pub fn pick_tool(&mut self, tool: &str) -> dtool_core::CommandResult {
        if let Some(command) = crate::tools_panel::boolean_tool_command(tool) {
            return self.run_and_reconcile(Command::with_params(command, serde_json::json!({})));
        }
        let session = self.workspace.session_mut().ok_or_else(dtool_core::CommandError::no_document)?;
        if session.active_tool != tool {
            crate::canvas::frame_text_input::commit_pending(session);
        }
        let result = crate::tools_panel::handle_tool_command(session, &Command::with_params("ui.tool.set", serde_json::json!({"id": tool})));
        if result.is_ok() && tool == "tool.slice" {
            let panel = crate::dock::PanelId::Other("slices".into());
            if !self.layout.is_panel_shown(&panel) {
                crate::panels::open_panel(&mut self.layout, &panel);
            }
        }
        result
    }

    pub fn open_about(&mut self) {
        self.about.open = true;
        self.about_chrome.reset();
    }

    pub fn show_about(&mut self, ctx: &egui::Context) -> crate::dialog_chrome::DialogResult {
        use crate::dialog_chrome::{ClosePolicy, DialogMode, DialogResult};
        if !self.about.open {
            return DialogResult::Open;
        }
        let (result, close) = self.about_chrome.show(ctx, "dtool.dialog.about", text("about.title"), DialogMode::Modal, ClosePolicy::Free, false, crate::about::body);
        let result = if close { DialogResult::Cancel } else { result };
        if result != DialogResult::Open {
            self.about.open = false;
        }
        result
    }

    pub fn run_text_styles_menu(&mut self, id: &str) -> dtool_core::CommandResult {
        use crate::panels::text_styles::{PanelAction, TEXT_STYLES_PANEL_KEY};
        use dtool_core::text_styles::StyleKind;
        let panel = crate::dock::PanelId::Other(TEXT_STYLES_PANEL_KEY.to_string());
        if !self.layout.is_panel_shown(&panel) {
            crate::panels::toggle_panel(&mut self.layout, &panel);
        }
        let session = self.workspace.session_mut().ok_or_else(dtool_core::CommandError::no_document)?;
        let styles = &mut self.panel_registry.text_styles;
        let kind = if id.contains("paragraph") { Some(StyleKind::Paragraph) } else if id.contains("character") { Some(StyleKind::Character) } else { None };
        if let Some(kind) = kind.filter(|kind| styles.selected.as_ref().is_none_or(|(selected, _)| selected != kind)) {
            if let Some(name) = caret_style(session, kind) {
                styles.selected = Some((kind, name));
            }
        }
        let result = styles.run(session, PanelAction::Menu(id.to_string()));
        self.reconcile_active();
        result
    }

    pub fn open_layer_effects(&mut self) -> bool {
        let Some(context) = self.workspace.active() else { return false; };
        let Some(id) = self.workspace.session().and_then(crate::mask_actions::target) else { return false; };
        self.layer_effects_owner = Some(context);
        self.layer_effects.open_for(id.0);
        true
    }

    pub fn open_blend_options(&mut self) -> bool {
        let Some(context) = self.workspace.active() else { return false; };
        let Some(session) = self.workspace.session() else { return false; };
        if !self.blend_options.open_selection(session) { return false; }
        self.blend_options_owner = Some(context);
        true
    }

    fn open_tone_blend(&mut self) {
        let Some(owner) = self.workspace.active() else { return; };
        let Some(session) = self.workspace.session_mut() else { return; };
        match crate::dialogs::tone_blend::Dialog::open(owner, session) {
            Ok(dialog) => self.tone_blend = Some(dialog),
            Err(error) => self.status_bar.set_hint_key(&error.message_key),
        }
    }

    fn show_selected_layer(&mut self, rename: bool) -> Option<dtool_core::object::ObjectId> {
        let owner = self.workspace.active()?;
        let session = self.workspace.session()?;
        let id = if rename { crate::panels::layers::rename_target(session) }
            else { crate::panels::layers::reveal_target(session) }?;
        let doc = &session.document;
        if dtool_core::commands::persona::layout_active() && doc.layout_artboard_origins.is_some() {
            if let Some(master) = dtool_core::artboard::owner(doc, id)
                .filter(|board| doc.master_pages.roots.contains(&board.id))
            {
                if crate::menus::pages::viewed_master(session) != Some(master.id) {
                    self.navigate_to_master(master.id).ok()?;
                }
            } else if !doc.master_pages.roots.contains(&id)
                && let Some(page) = doc.pages.get(dtool_core::data_merge::layout::object_page_index(doc, id))
                && Some(page.id) != dtool_core::commands::pages::viewed_page(session)
            {
                let from_master = crate::menus::pages::viewed_master(session).is_some();
                self.run_and_reconcile(Command::with_params("page.navigate", json!({"id": page.id}))).ok()?;
                if from_master {
                    self.panel_registry.pages.show_master_tab(false);
                }
            }
        }
        let session = self.workspace.session()?;
        self.panel_registry.layers.reveal(owner, &session.document, id);
        if rename {
            self.panel_registry.layers.begin_rename(owner, &session.document, id);
        }
        self.hidden_ui = false;
        crate::panels::open_panel(&mut self.layout, &crate::dock::PanelId::Layers);
        Some(id)
    }

    pub fn open_guides_manager(&mut self) -> bool {
        let Some(id) = self.workspace.active() else { return false; };
        self.guides_owner = Some(id);
        self.guides_dialog.open();
        true
    }

    fn grid_dialog_open_for(&self, menu: crate::grid_dialog::GridMenu, owner: ContextId) -> bool {
        match menu {
            crate::grid_dialog::GridMenu::Axis => self.grid_and_axis.open && self.grid_and_axis.owner == Some(owner),
            crate::grid_dialog::GridMenu::Baseline => self.baseline_grid.open && self.baseline_grid.owner == Some(owner),
        }
    }

    fn open_grid_dialog(&mut self, menu: crate::grid_dialog::GridMenu) -> bool {
        let Some(owner) = self.workspace.active() else { return false; };
        if self.grid_dialog_open_for(menu, owner) { return true; }
        let Some(context) = self.workspace.get_mut(owner) else { return false; };
        let grid = crate::grid_dialog::grid_query(&mut context.session);
        match menu {
            crate::grid_dialog::GridMenu::Axis => self.grid_and_axis.open_for(owner, &grid),
            crate::grid_dialog::GridMenu::Baseline => self.baseline_grid.open_for(owner, &grid),
        }
        true
    }

    pub(crate) fn open_move_duplicate(&mut self, ctx: &egui::Context) -> bool {
        if self.move_duplicate.is_some() { return false; }
        self.move_duplicate = crate::dialogs::move_duplicate::MoveDuplicateDialog::open(&self.workspace, ctx);
        self.move_duplicate.is_some()
    }

    pub fn input_blocked(&self, ctx: &egui::Context) -> bool {
        self.modal_blocking || crate::dialog_chrome::modal_open(ctx)
    }

    pub fn float_title(&self, id: ContextId) -> Option<String> {
        let context = self.workspace.get(id)?;
        let name = if context.title.is_empty() { text("doc.untitled").to_string() } else { context.title.clone() };
        Some(if context.is_modified() { format!("{name}*") } else { name })
    }

    pub fn retain_floating_documents(&mut self) {
        let workspace = &self.workspace;
        self.float_manager.floating.retain(|id| workspace.get(*id).is_some());
        let floating = &self.float_manager.floating;
        self.float_canvases.retain(|id, _| floating.contains(id));
    }

    pub fn show_floating_documents(&mut self, ctx: &egui::Context) {
        self.retain_floating_documents();
        let ids: Vec<ContextId> = self.float_manager.floating.iter().copied().collect();
        for id in ids {
            let Some(title) = self.float_title(id) else { continue; };
            let Some(context) = self.workspace.get_mut(id) else { continue; };
            let canvas = self.float_canvases.entry(id).or_default();
            canvas.blend_preview = (self.blend_options_owner == Some(id))
                .then(|| self.blend_options.preview(&context.session)).flatten();
            canvas.tone_preview = self.tone_blend.as_ref().and_then(|dialog| dialog.preview(id, &context.session));
            canvas.layer_opacity_preview = self.panel_registry.layers.opacity_preview(ctx, &context.session);
            let keep = self.float_manager.show_viewport(ctx, id, &title, [800.0, 600.0], |ui| {
                let response = ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click_and_drag());
                canvas.input.bind_context(Some(id.0));
                canvas.frame(ui, &response, &mut context.session, true);
                context.view_state = context.session.view.clone();
            });
            if !keep { self.float_manager.dock(id); }
        }
    }

    pub fn window_title(&self) -> String {
        let Some(context) = self.workspace.active().and_then(|id| self.workspace.get(id)) else {
            return text("app.title").to_string();
        };
        let name = if context.title.is_empty() { text("doc.untitled").to_string() } else { context.title.clone() };
        if context.is_modified() { format!("{name}*") } else { name }
    }

    pub fn sync_window_title(&mut self, ctx: &egui::Context) -> bool {
        let title = self.window_title();
        if self.window_title_sent.as_deref() == Some(title.as_str()) {
            return false;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
        self.window_title_sent = Some(title);
        true
    }

    pub fn sync_status_chrome(&mut self) {
        self.status_bar.visible = self.settings.status_bar_visible;
        self.status_bar.zoom_text = self.workspace.active().and_then(|id| self.workspace.get(id)).map(|context| {
            let actual = context.session.actual_zoom();
            crate::status_bar::zoom_label(context.view_state.zoom / actual)
        });
    }

    pub fn activate_document_index(&mut self, index: usize) -> bool {
        let Some(id) = self.workspace.contexts().get(index).map(|context| context.id) else { return false; };
        if self.activate_context(id).is_err() { return false; }
        self.reconcile_active();
        true
    }

    pub fn run_bridge_action(&mut self, action: &str) -> dtool_core::CommandResult {
        let result = crate::ui_bridge::UiBridge::invoke_action(&mut self.workspace, &mut self.locked_delete, action);
        if result.is_ok() { self.reconcile_active(); }
        result
    }

    pub fn run_menu_binding(&mut self, binding: crate::menu_tree::MenuBinding) -> dtool_core::CommandResult {
        let size = self.canvas_size;
        let command = match self.workspace.session_mut() {
            Some(session) => binding.command(session, size),
            None => return Err(dtool_core::CommandError::no_document()),
        };
        self.run_and_reconcile(command)
    }

    pub fn install_session_dock(&mut self) {
        crate::panels::session_route::install();
        let path = crate::presets::PresetStore::persisted_path();
        if let Some(dock) = crate::panels::session_route::dock_mut(&mut self.workspace) {
            dock.persist_path = Some(path);
        }
        if let Some(layout) = crate::persona::live_dock() {
            self.layout = layout;
        }
        self.layout.sync_tree_visibility();
        self.push_dock();
    }

    pub fn push_dock(&mut self) {
        let layout = self.layout.clone();
        let store = self.preset_store.clone();
        if let Some(dock) = crate::panels::session_route::dock_mut(&mut self.workspace) {
            dock.layout = layout;
            dock.store = store;
            dock.revision = dock.revision.wrapping_add(1);
            self.dock_seen = dock.revision;
        }
    }

    pub fn pull_dock(&mut self) -> bool {
        let Some(dock) = crate::panels::session_route::dock(&self.workspace) else {
            return false;
        };
        if dock.revision == self.dock_seen {
            return false;
        }
        self.layout = dock.layout.clone();
        self.preset_store = dock.store.clone();
        self.dock_seen = dock.revision;
        true
    }

    pub fn sync_dock(&mut self) {
        if !self.pull_dock() {
            self.push_dock();
        }
    }

    pub fn install_core_prefs(&mut self) {
        dtool_core::commands::prefs_cmd::install(self.prefs_dialog.prefs_path().to_path_buf());
        self.adopt_core_prefs();
        self.apply_prefs_to_workspace();
    }

    pub fn apply_prefs_to_workspace(&mut self) {
        let prefs = self.prefs.clone();
        self.workspace.apply_prefs(&prefs);
        self.prefs_applied.clear();
        self.apply_prefs_to_new_contexts();
    }

    pub fn apply_prefs_to_new_contexts(&mut self) {
        let ids: Vec<dtool_core::ContextId> = self.workspace.contexts().iter().map(|context| context.id).collect();
        self.prefs_applied.retain(|id| ids.contains(id));
        for id in ids {
            if self.prefs_applied.contains(&id) {
                continue;
            }
            if let Some(context) = self.workspace.get_mut(id) {
                dtool_core::commands::prefs_cmd::apply_to_session(&mut context.session, &self.prefs);
            }
            self.prefs_applied.push(id);
        }
    }

    pub fn adopt_core_prefs(&mut self) -> bool {
        let Some(prefs) = dtool_core::commands::prefs_cmd::snapshot() else {
            return false;
        };
        if self.core_prefs_seen.as_ref() == Some(&prefs) {
            return false;
        }
        self.core_prefs_seen = Some(prefs.clone());
        if prefs == self.prefs {
            return false;
        }
        self.settings.text_contrast = prefs.text_contrast;
        self.prefs = prefs;
        self.sync_mcp_switch(&self.prefs);
        self.apply_prefs_to_workspace();
        true
    }

    pub fn is_pref_command(name: &str) -> bool {
        matches!(name, dtool_core::prefs::CMD_QUERY | dtool_core::prefs::CMD_SET | dtool_core::prefs::CMD_PERFORMANCE_SET)
    }

    pub fn pref_command(&mut self, name: &str, params: &serde_json::Value) -> Result<serde_json::Value, dtool_core::prefs::PrefsError> {
        let result = self.prefs_dialog.run(&mut self.prefs, &mut self.settings, name, params);
        if result.is_ok() && name != dtool_core::prefs::CMD_QUERY {
            self.push_core_prefs();
            self.sync_mcp_switch(&self.prefs);
        }
        result
    }

    pub fn open_preferences(&mut self) {
        let mut loaded_settings = self.settings.clone();
        self.prefs_dialog.load(&mut self.prefs, &mut loaded_settings);
        self.prefs.text_contrast = self.settings.text_contrast;
        self.prefs_dialog.open = true;
        self.sync_mcp_switch(&self.prefs);
    }

    pub fn show_preferences(&mut self, ui: &mut egui::Ui) {
        if !self.prefs_dialog.open {
            return;
        }
        self.prefs_dialog.show(ui, &mut self.prefs, &mut self.settings);
        self.push_core_prefs();
        self.sync_mcp_switch(&self.prefs);
    }

    /// Mirror the shell's prefs (Preferences window / pref.set) into the core prefs_cmd store,
    /// so MCP pref.query sees them without a session. No-op while they already match.
    pub fn push_core_prefs(&mut self) -> bool {
        if self.core_prefs_seen.as_ref() == Some(&self.prefs) {
            return false;
        }
        if !dtool_core::commands::prefs_cmd::replace(self.prefs.clone()) {
            return false;
        }
        self.core_prefs_seen = Some(self.prefs.clone());
        self.apply_prefs_to_workspace();
        true
    }

    pub fn text_editing(&self) -> bool {
        self.workspace.session().is_some_and(|session| session.text_edit.is_some())
    }

    pub fn context_target_for(session: &Session) -> &'static str {
        if session.text_edit.is_some() {
            CONTEXT_TEXT_EDITING
        } else if !session.selection.ids.is_empty() {
            CONTEXT_OBJECT
        } else {
            CONTEXT_PASTEBOARD
        }
    }

    pub fn sync_status_inputs(&mut self, ctx: &egui::Context) {
        let tool = self.workspace.session().map(|session| session.active_tool.clone()).unwrap_or_default();
        let m = ctx.input(|input| input.modifiers);
        let next = (tool, m.shift, m.alt, m.ctrl || m.command);
        if self.status_inputs.as_ref() == Some(&next) {
            return;
        }
        self.status_bar.set_tool(&next.0);
        self.status_bar.set_modifiers(next.1, next.2, next.3);
        self.status_inputs = Some(next);
    }

    pub fn apply_theme(&mut self, ctx: &egui::Context) -> bool {
        let ppp = ctx.pixels_per_point();
        if self.icon_ppp != ppp {
            self.icon_cache.clear();
            self.icon_ppp = ppp;
        }
        let key = format!("{:?}|{:?}|{:?}|{:?}|{:?}|{}", self.settings.ui_style, self.settings.background_grey, self.settings.ui_brightness, self.settings.text_contrast, self.settings.high_contrast, self.prefs.accent_colour);
        if self.theme_applied.as_deref() == Some(key.as_str()) {
            return false;
        }
        let mut palette = crate::theme::palette_for(&self.settings);
        if let Some(colour) = dtool_design::tokens::parse_hex(&self.prefs.accent_colour) {
            let [r, g, b, _] = colour.to_srgba_unmultiplied();
            palette.accent = egui::Color32::from_rgb(r, g, b);
        }
        crate::theme::apply(ctx, &palette);
        self.theme_applied = Some(key);
        self.icon_cache.clear();
        true
    }

    pub fn last_drawn_generation(&self) -> u64 {
        self.last_drawn.map(|key| key.1).unwrap_or(0)
    }

    pub fn last_drawn(&self) -> Option<(ContextId, u64)> {
        self.last_drawn
    }

    fn apply_reference_layout(&mut self, id: &str) -> Result<(), String> {
        let mut store = self.preset_store.clone();
        let mut layout = self.layout.clone();
        let prefix = text("workspace.previous_layout");
        if id == "previous" {
            layout = store.presets.iter().rev().find(|p| p.name.starts_with(prefix)).map(|p| p.layout.clone()).ok_or_else(|| "error.not_found".to_string())?;
            store.saved_layout = Some(layout.clone());
        } else {
            let persona = crate::presets::WorkspacePersona::from_key(id).ok_or_else(||"error.invalid_params".to_string())?;
            let mut n = 1;
            let mut name = prefix.to_string();
            while store.contains(&name) { n += 1; name = format!("{prefix} ({n})"); }
            store.save(&name, &layout);
            store.choose_workspace(persona, &mut layout);
        }
        store.persist(&crate::presets::PresetStore::persisted_path()).map_err(|_|"error.io.other".to_string())?;
        crate::persona::save_live_dock(&layout).map_err(|e|e.message_key)?;
        self.layout = layout;
        self.preset_store = store;

        crate::panels::register_named_panels(&mut self.layout, &self.panel_registry);
        self.persist_layout().map_err(|_|"error.io.other".to_string())?;
        self.push_dock();
        Ok(())
    }

    pub fn run_panels_action(&mut self, id: &str) {
        if let Err(error) = self.apply_panels_action(id) {
            self.status_bar.set_hint_key(&error.message_key);
        }
    }

    fn apply_panels_action(&mut self, id: &str) -> Result<(), dtool_core::CommandError> {
        use dtool_core::CommandError;
        self.pull_dock();
        if let Some(reference) = id.strip_prefix("window.panels.reference.") {
            return self.apply_reference_layout(reference).map_err(|message_key| {
                let mut error = match message_key.as_str() {
                    "error.not_found" => CommandError::not_found(),
                    "error.invalid_params" => CommandError::invalid_params(),
                    _ => CommandError::new("IO_ERROR"),
                };
                error.message_key = message_key;
                error
            });
        }
        let saved = if let Some(side) = id.strip_prefix("window.panels.show_").and_then(DockSide::from_key) {
            self.layout.toggle_side(side);
            self.persist_layout()
        } else {
            match id {
                "window.panels.hide" => {
                    crate::window_menu::menu_window_panels_hide(&mut self.layout);
                    self.persist_layout()
                }
                "window.panels.reset" => self.reset_layout(),
                _ => return Err(CommandError::invalid_params()),
            }
        };
        self.push_dock();
        saved.map_err(|_| CommandError { code: "IO_ERROR".into(), message_key: "error.io.other".into() })
    }

    pub fn poll_mcp_confirms(&mut self) -> Option<dtool_core::workspace::HostConfirms> {
        let Some(confirms) = self.workspace.host_confirms().cloned() else {
            self.mcp_confirm = None;
            return None;
        };
        confirms.set_ui_attached(true);
        let pending = confirms.pending();
        if let Some((id, _)) = &self.mcp_confirm {
            if !pending.iter().any(|item| item.id == *id) {
                self.mcp_confirm = None;
                self.status_bar.set_hint_key("error.confirm_expired");
            }
        }
        if self.mcp_confirm.is_none() {
            if let Some(item) = pending.first() {
                self.mcp_confirm = Some((item.id, crate::dialogs::McpConfirmDialogState::for_host(&item.class, &item.message_key)));
            }
        }
        Some(confirms)
    }

    pub fn answer_mcp_confirm(&mut self, confirms: &dtool_core::workspace::HostConfirms, approve: bool) -> bool {
        let Some((id, _)) = self.mcp_confirm.take() else { return false; };
        let answered = confirms.answer(id, approve);
        if !answered {
            self.status_bar.set_hint_key("error.confirm_expired");
        }
        answered
    }

    pub fn show_mcp_confirms(&mut self, ui: &mut egui::Ui) {
        let Some(confirms) = self.poll_mcp_confirms() else { return; };
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        let answer = match self.mcp_confirm.as_mut() {
            Some((_, state)) => crate::dialogs::show_mcp_confirm_dialog(ui, state),
            None => None,
        };
        if let Some(approve) = answer {
            self.answer_mcp_confirm(&confirms, approve);
        }
    }

    pub fn request_close(&mut self, id: ContextId) {
        if self.workspace.active() == Some(id) {
            self.commit_ime_before_switch();
        }
        if let Ok(dtool_core::workspace::CloseCheck::NeedsAnswer) = self.workspace.request_close(id) {
            if self.pending_close_id.is_none() {
                self.pending_close_id = Some(id);
            } else if self.pending_close_id != Some(id) && !self.close_queue.contains(&id) {
                self.close_queue.push_back(id);
            }
        }
    }

    pub fn close_active(&mut self) {
        if let Some(id) = self.workspace.active() {
            self.request_close(id);
        }
    }

    pub fn queue_close(&mut self, ids: Vec<ContextId>) {
        for id in ids {
            if self.pending_close_id != Some(id) && !self.close_queue.contains(&id) {
                self.close_queue.push_back(id);
            }
        }
        self.advance_close_queue();
    }

    pub fn close_all_documents(&mut self) {
        self.commit_ime_before_switch();
        let ids = self.workspace.close_all();
        self.queue_close(ids);
    }

    fn request_exit(&mut self) {
        if self.exit_review.is_none() {
            self.commit_ime_before_switch();
            self.exit_review = Some(Default::default());
        }
    }

    pub fn handle_window_close(&mut self, ctx: &egui::Context) {
        if self.exit_approved || !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        let home_only = self.workspace.active().is_none() && self.new_document_dialog.is_some() && self.file_dialog.is_none();
        if home_only || !self.dialog_open(ctx) {
            self.request_exit();
        }
        if self.exit_review.is_some() && ctx.input(|input| input.viewport().minimized == Some(true)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }
    }

    pub fn close_other_documents(&mut self, keep: ContextId) {
        if self.workspace.active() != Some(keep) {
            self.commit_ime_before_switch();
        }
        let ids = self.workspace.close_others(keep);
        self.queue_close(ids);
    }

    pub fn advance_close_queue(&mut self) {
        while self.pending_close_id.is_none() {
            let Some(id) = self.close_queue.pop_front() else { return; };
            if self.workspace.get(id).is_some() {
                self.request_close(id);
            }
        }
    }

    pub fn active_untitled(&self) -> bool {
        self.workspace.active().and_then(|id| self.workspace.get(id)).is_some_and(|context| context.path.is_none())
    }

    pub fn open_document_setup(&mut self) {
        if self.document_setup.is_open() { return; }
        let Some(id) = self.workspace.active() else { return; };
        let Some(context) = self.workspace.get(id) else { return; };
        self.document_setup.open(&context.session);
        self.document_setup_owner = Some(id);
    }

    pub fn show_document_setup(&mut self, ui: &mut egui::Ui) {
        if !self.document_setup.open {
            self.document_setup_owner = None;
            return;
        }
        let Some(context) = self.document_setup_owner.and_then(|id| self.workspace.get_mut(id)) else {
            self.document_setup.open = false;
            self.document_setup_owner = None;
            return;
        };
        let previous = &self.document_setup.original_setup;
        let already_has_cover = previous.facing_pages && previous.start_on_right;
        let result = self.document_setup.show(ui, &mut context.session);
        context.view_state = context.session.view.clone();
        if let Some(result) = result {
            let doc = &context.session.document;
            if result == crate::dialogs::document_setup::DialogResult::Ok
                && !already_has_cover
                && doc.layout_artboard_origins.is_some()
                && dtool_core::data_merge::layout::has_right_cover(doc)
            {
                let _ = context.session.run(&Command::with_params("page.navigate", json!({"number": 1})));
                context.view_state = context.session.view.clone();
            }
            self.reconcile_active();
        }
    }

    pub fn open_resize_document(&mut self) {
        let Some(id) = self.workspace.active() else { return; };
        let Some(context) = self.workspace.get(id) else { return; };
        self.resize_document.open_for(&context.session.document);
        self.resize_document_owner = Some(id);
    }

    fn open_resize_canvas(&mut self) {
        let Some(id) = self.workspace.active() else { return; };
        let Some(context) = self.workspace.get(id) else { return; };
        self.resize_canvas = crate::resize_canvas_dialog::ResizeCanvasDialog::new(id, &context.session.document);
    }

    fn open_selection_refine(&mut self) -> Result<(), dtool_core::CommandError> {
        let owner = self.workspace.active().ok_or_else(dtool_core::CommandError::no_document)?;
        let session = self.workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
        self.selection_refine = Some(crate::selection_refine_dialog::RefineDialog::new(owner, session)?);
        self.app_canvas.input.cancel();
        Ok(())
    }

    fn show_selection_refine(&mut self, ctx: &egui::Context) {
        use crate::selection_refine_dialog::Action;
        let Some(mut dialog) = self.selection_refine.take() else { return; };
        if self.workspace.active() != Some(dialog.owner)
            || self.workspace.session().is_none_or(|session| session.generation != dialog.generation)
        {
            return;
        }
        match dialog.show(ctx) {
            Action::KeepOpen => self.selection_refine = Some(dialog),
            Action::Cancel => {}
            Action::Apply(command) => match self.workspace.run(dtool_core::Scope::of(dialog.owner), command).1 {
                Ok(result) => {
                    self.reconcile_active();
                    if result["changes"].as_array().is_some_and(Vec::is_empty) {
                        self.status_bar.set_hint_key("refine.no_change");
                    }
                }
                Err(error) => {
                    dialog.error = Some(error.message_key);
                    self.selection_refine = Some(dialog);
                }
            },
        }
    }

    fn open_selection_modify(&mut self, kind: crate::selection_modify_dialog::Kind) -> Result<(), dtool_core::CommandError> {
        let owner = self.workspace.active().ok_or_else(dtool_core::CommandError::no_document)?;
        let session = self.workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
        self.selection_modify = Some(crate::selection_modify_dialog::ModifyDialog::new(owner, session, kind)?);
        self.app_canvas.input.cancel();
        Ok(())
    }

    fn show_selection_modify(&mut self, ctx: &egui::Context) {
        use crate::selection_modify_dialog::Action;
        let Some(mut dialog) = self.selection_modify.take() else { return; };
        if self.workspace.active() != Some(dialog.owner)
            || self.workspace.session().is_none_or(|session| session.generation != dialog.generation)
        {
            return;
        }
        match dialog.show(ctx) {
            Action::KeepOpen => self.selection_modify = Some(dialog),
            Action::Cancel => {}
            Action::Apply(command) => match self.workspace.run(dtool_core::Scope::of(dialog.owner), command).1 {
                Ok(result) => {
                    self.reconcile_active();
                    if result["changes"].as_array().is_some_and(Vec::is_empty) {
                        self.status_bar.set_hint_key("refine.no_change");
                    }
                }
                Err(error) => {
                    dialog.error = Some(error.message_key);
                    self.selection_modify = Some(dialog);
                }
            },
        }
    }

    fn open_selection_sampled_colour(&mut self) -> Result<(), dtool_core::CommandError> {
        let owner = self.workspace.active().ok_or_else(dtool_core::CommandError::no_document)?;
        let session = self.workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
        let page_id = self.workspace.get(owner).and_then(|context| context.session.view_canvas_size)
            .and_then(|viewport| dtool_core::selection::PixelSurface::at_view_center(&session.document, &session.view, viewport))
            .map(|surface| surface.page_id);
        self.selection_sampled_colour = Some(crate::selection_sampled_colour::SampledColourDialog::new(owner, session, page_id)?);
        self.app_canvas.input.cancel();
        Ok(())
    }

    fn show_selection_sampled_colour(&mut self, ctx: &egui::Context) {
        use crate::selection_sampled_colour::Action;

        let Some(mut dialog) = self.selection_sampled_colour.take() else { return; };
        if self.workspace.active() != Some(dialog.owner)
            || self.workspace.session().is_none_or(|session| session.generation != dialog.generation)
        {
            return;
        }
        match dialog.show(ctx) {
            Action::KeepOpen => self.selection_sampled_colour = Some(dialog),
            Action::Cancel => {}
            Action::Apply(command) => match self.workspace.run(dtool_core::Scope::of(dialog.owner), command).1 {
                Ok(_) => self.reconcile_active(),
                Err(error) => {
                    dialog.error = Some(error.message_key);
                    self.selection_sampled_colour = Some(dialog);
                }
            },
        }
    }

    fn show_resize_canvas(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.resize_canvas.take() else { return; };
        if self.workspace.get(dialog.owner).is_none() { return; }
        match dialog.show(ctx) {
            crate::resize_canvas_dialog::Action::Open => self.resize_canvas = Some(dialog),
            crate::resize_canvas_dialog::Action::Cancel => {}
            crate::resize_canvas_dialog::Action::Resize(command) => match self.workspace.run(dtool_core::Scope::of(dialog.owner), command).1 {
                Ok(_) => self.reconcile_active(),
                Err(error) => {
                    dialog.error = Some(error.message_key);
                    self.resize_canvas = Some(dialog);
                }
            },
        }
    }

    fn open_resize_pixel_art(&mut self) {
        let Some(id) = self.workspace.active() else { return; };
        let Some(context) = self.workspace.get(id) else { return; };
        self.resize_pixel_art = crate::resize_pixel_art_dialog::ResizePixelArtDialog::new(id, &context.session.document);
    }

    fn show_resize_pixel_art(&mut self, ctx: &egui::Context) {
        use crate::resize_canvas_dialog::Action;
        let Some(mut dialog) = self.resize_pixel_art.take() else { return; };
        if self.workspace.get(dialog.owner).is_none() { return; }
        match dialog.show(ctx) {
            Action::Open => self.resize_pixel_art = Some(dialog),
            Action::Cancel => {}
            Action::Resize(command) => match self.workspace.run(dtool_core::Scope::of(dialog.owner), command).1 {
                Ok(_) => self.reconcile_active(),
                Err(error) => {
                    dialog.error = Some(error.message_key);
                    self.resize_pixel_art = Some(dialog);
                }
            },
        }
    }

    pub fn show_resize_document(&mut self, ctx: &egui::Context) {
        if !self.resize_document.open {
            self.resize_document_owner = None;
            return;
        }
        let Some(context) = self.resize_document_owner.and_then(|id| self.workspace.get_mut(id)) else {
            self.resize_document.open = false;
            self.resize_document_owner = None;
            return;
        };
        if self.resize_document.show(ctx, &mut context.session) {
            self.reconcile_active();
        }
    }

    pub fn last_snapshot(&self) -> &Value {
        &self.last_snapshot
    }

    pub fn refresh_count(&self) -> u64 {
        self.refresh_count
    }

    pub fn reconcile_active(&mut self) {
        let Some(id) = self.workspace.active() else {
            self.last_drawn = None;
            self.last_snapshot = Value::Null;
            return;
        };
        let Some(generation) = self.workspace.session().map(|session| session.generation) else { return };
        if Some((id, generation)) == self.last_drawn {
            return;
        }
        if self.last_drawn.is_none_or(|(previous, _)| previous != id) {
            self.status_bar.clear_file_open_error();
        }
        let snapshot = self.workspace.session().and_then(|session| session.query(None).ok()).unwrap_or(Value::Null);
        self.last_drawn = Some((id, generation));
        self.render(snapshot);
    }

    pub fn open_paste_special(&mut self) -> bool {
        let Some(session) = self.workspace.session_mut() else { return false };
        self.paste_special_dialog = Some(crate::dialogs::PasteSpecialDialogState::from_session(session));
        true
    }

    #[cfg(windows)]
    pub fn accepts_vector_paste_shortcut(&self, ctx: &egui::Context) -> bool {
        !self.input_blocked(ctx) && self.current_focus(ctx) == Focus::Canvas
            && self.workspace.session().is_some_and(|session| session.text_edit.is_none())
    }

    pub fn run_and_reconcile(&mut self, mut command: Command) -> dtool_core::CommandResult {
        if matches!(command.name.as_str(), "edit.paste" | "edit.paste_inside" | "doc.new_from_clipboard") && self.workspace.session().is_none_or(|session| session.text_edit.is_none()) {
            crate::os_clipboard::refresh_native_graphic(self.workspace.app_clipboard());
        }
        if Self::is_pref_command(&command.name) {
            return self.pref_command(&command.name, &command.params);
        }
        if (command.name == "selection.all" || (command.name == "selection.load" && command.params["path"].is_string()))
            && command.params.get("page_id").is_none()
            && let Some(context) = self.workspace.active().and_then(|id| self.workspace.get(id))
            && let Some(viewport) = context.session.view_canvas_size
            && let Some(surface) = dtool_core::selection::PixelSurface::at_view_center(
                &context.session.document, &context.session.view, viewport,
            )
        {
            if !command.params.is_object() { command.params = serde_json::json!({}); }
            command.params["page_id"] = serde_json::json!(surface.page_id);
        }
        let (_, result) = self.workspace.run(dtool_core::Scope::active(), command);
        if result.is_ok() {
            self.reconcile_active();
        }
        result
    }

    fn publish_os_clipboard(&mut self, ctx: &egui::Context, events: &crate::os_clipboard::OsClipboardEvents) {
        let Some(session) = self.workspace.session().filter(|session| session.text_edit.is_none()) else { return };
        let result = match crate::os_clipboard::image_for_copy(session) {
            Ok(Some(image)) => crate::os_clipboard::write_native_image(ctx, image),
            Ok(None) => {
                if let Some(text) = crate::os_clipboard::after_shortcuts(session, events) {
                    ctx.copy_text(text);
                }
                Ok(())
            }
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.status_bar.set_hint_key(&error.message_key);
        }
    }

    pub fn open_file_dialog(&mut self, mode: crate::dialogs::FileDialogMode) {
        let owned = mode.needs_owner();
        self.file_dialog = match (owned, self.workspace.active()) {
            (true, Some(id)) => Some(crate::dialogs::FileDialogState::for_document(mode, id)),
            (true, None) => None,
            (false, _) => Some(crate::dialogs::FileDialogState::new(mode)),
        };
    }

    fn browse_selection_file(&mut self, mode: crate::dialogs::FileDialogMode) {
        #[cfg(windows)]
        {
            let Some(owner) = self.workspace.active() else { return; };
            let extension = dtool_core::selection::AFSELECTION_EXT;
            let save = mode == crate::dialogs::FileDialogMode::SelectionSave;
            let initial = format!("{}.{extension}", text("selection.file_default_name"));
            match crate::dialogs::pick_file_path(
                text(mode.title_key()), text("dialog.file.filter.selection"), extension, save.then_some(initial.as_str()),
            ) {
                Ok(Some(path)) => {
                    if save && path.extension().is_none_or(|extension| !extension.eq_ignore_ascii_case(dtool_core::selection::AFSELECTION_EXT)) {
                        self.status_bar.set_hint_key("error.invalid_params");
                        return;
                    }
                    let params = if save { json!({"to": "file", "path": path}) } else { json!({"from": "file", "path": path}) };
                    let command = Command::with_params(mode.command_name(), params);
                    match self.workspace.run(dtool_core::Scope::of(owner), command).1 {
                        Ok(_) => self.reconcile_active(),
                        Err(error) => self.status_bar.set_hint_key(&error.message_key),
                    }
                    return;
                }
                Ok(None) => return,
                Err(_) => {}
            }
        }
        self.open_file_dialog(mode);
    }

    fn load_selection_channel(&mut self, index: usize) {
        let name = self.workspace.session().and_then(|session| session.document.spare_channels.get(index))
            .map(|(name, _)| name.clone());
        if let Some(name) = name {
            let command = Command::with_params("selection.load", json!({"from": "channel", "name": name}));
            if let Err(error) = self.run_and_reconcile(command) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
    }

    pub(crate) fn save_active_document(&mut self) {
        let Some(id) = self.workspace.active() else { return; };
        if self.workspace.get(id).is_some_and(|context| context.path.is_some()) {
            if let Err(error) = self.workspace.save(id, None) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        } else {
            self.open_file_dialog(crate::dialogs::FileDialogMode::SaveAs);
        }
    }

    pub fn sync_dialog_registry(&mut self) {
        use crate::dialogs::{DialogKind, DialogOwner};
        let export_owner = export_dialog().lock().ok().and_then(|dialog| if dialog.is_open() { dialog.owner() } else { None });
        self.dialogs.sync(DialogKind::Export, export_owner.map(DialogOwner::Document));
        self.dialogs.sync_document(DialogKind::Guides, self.guides_dialog.open, self.guides_owner);
        self.dialogs.sync_document(DialogKind::GridAxis, self.grid_and_axis.open, self.grid_and_axis.owner);
        self.dialogs.sync_document(DialogKind::BaselineGrid, self.baseline_grid.open, self.baseline_grid.owner);
        self.dialogs.sync_document(DialogKind::LockedDelete, self.locked_delete.is_open(), self.locked_delete.owner);
        self.dialogs.sync_document(DialogKind::ResourceManager, self.resource_manager.open, self.resource_manager.owner);
        self.dialogs.sync(DialogKind::File, self.file_dialog.as_ref().map(|state| state.owner.clone()));
        self.dialogs.sync(DialogKind::CloseDocument, self.close_dialog.as_ref().map(|state| DialogOwner::Document(state.context_id)));
        self.dialogs.sync(DialogKind::NewDocument, self.new_document_dialog.as_ref().map(|_| DialogOwner::App));
        self.dialogs.sync(DialogKind::McpConfirm, self.mcp_confirm.as_ref().map(|_| DialogOwner::App));
        self.dialogs.sync(DialogKind::About, self.about.open.then_some(DialogOwner::App));
    }

    pub fn drop_orphan_dialogs(&mut self) {
        use crate::dialogs::DialogKind;
        self.sync_dialog_registry();
        let gone = self.dialogs.take_gone(&self.workspace);
        for kind in &gone {
            match kind {
                DialogKind::GridAxis => { self.grid_and_axis.open = false; self.grid_and_axis.owner = None; }
                DialogKind::BaselineGrid => { self.baseline_grid.open = false; self.baseline_grid.owner = None; }
                DialogKind::LockedDelete => self.locked_delete.close(),
                DialogKind::ResourceManager => { self.resource_manager.open = false; self.resource_manager.owner = None; }
                DialogKind::File => self.file_dialog = None,
                DialogKind::Guides => { self.guides_dialog.open = false; self.guides_owner = None; }
                DialogKind::Export => { if let Ok(mut dialog) = export_dialog().lock() { dialog.drop_if_gone(&self.workspace); } }
                _ => {}
            }
        }
        if gone.iter().any(|kind| *kind != DialogKind::CloseDocument) { self.status_bar.set_hint_key("error.context_not_found"); }
    }

    pub fn select_all(&mut self) {
        let Some(session) = self.workspace.session_mut() else { return; };
        if let Some(edit) = session.text_edit.as_ref() {
            let story = edit.story;
            crate::canvas::frame_text_input::commit_pending(session);
            if let Some(end) = session.document.stories.iter().find(|item| item.id == story)
                .map(|item| item.text.chars().count())
            {
                crate::input::place_caret(session, end, 0);
            }
            return;
        }
        let nodes = &mut self.app_canvas.input.nodes;
        if crate::canvas_input::tool_kind(&session.active_tool) == crate::canvas_input::ToolKind::Node
            && (nodes.grab.is_some() || nodes.select_all(session))
        {
            return;
        }
        let command = if self.menu_persona == "pixel" {
            Command::new("selection.all")
        } else {
            let doc = &session.document;
            let isolate = session.view.isolate_target().filter(|id| doc.get_object(*id).is_some());
            let ids: Vec<_> = doc.objects.iter().filter(|object| {
                isolate.map_or(object.parent_id.is_none(), |id| object.id == id)
                    && !object.locked
                    && object.clip_role != dtool_core::object::ClipRole::Mask
                    && dtool_core::scene::is_drawn_in_view(doc, &session.view, object.id)
            }).map(|object| object.id.0).collect();
            Command::with_params("selection.set", serde_json::json!({"ids": ids}))
        };
        if let Err(error) = self.run_and_reconcile(command) {
            self.status_bar.set_hint_key(&error.message_key);
        }
    }

    pub fn delete_selection(&mut self) {
        let owner=self.workspace.active();
        let Some(session)=self.workspace.session_mut() else {return;};
        if session.selection.ids.is_empty(){return;}
        let result=self.locked_delete.request_delete_selection(session);
        if self.locked_delete.is_open(){self.locked_delete.owner=owner;}
        if let Err(error)=result{
            if !crate::dialogs::locked_delete::LockedDeleteDialog::is_locked_error(&error){self.status_bar.set_hint_key(&error.message_key);}
        }
    }

    pub fn select_commands(&mut self, ui: &mut egui::Ui, session: &Session) -> Vec<Command> {
        let mut commands = Vec::new();
        if let Some(command) = layer_select_menu(ui, session) {
            commands.push(command);
        }
        if let Some(command) = nudge_from_input(ui) {
            commands.push(command);
        }
        commands
    }

    pub fn open_export_dialog(&mut self) {
        let owner = self.workspace.active();
        let result = self.run_and_reconcile(Command::with_params(crate::dialogs::export::UI_EXPORT_OPEN, serde_json::json!({})));
        match result {
            Ok(_) => {
                if let Ok(mut dialog) = export_dialog().lock() {
                    dialog.bind_owner(owner);
                }
            }
            Err(error) => self.status_bar.set_hint_key(&error.message_key),
        }
    }

    #[cfg(windows)]
    fn open_print_dialog(&mut self) {
        self.commit_ime_before_switch();
        if let Some(context) = self.workspace.active().and_then(|id| self.workspace.get(id)) {
            self.print_dialog = Some(crate::print::dialog::PrintDialog::new(context.id, &context.session.document));
        }
    }

    pub fn show_export_dialog(&mut self, ui: &mut egui::Ui) {
        let Ok(mut dialog) = export_dialog().lock() else {
            return;
        };
        if !dialog.is_open() {
            return;
        }
        self.modal_blocking = true;
        dialog.document_viewport = self.app_canvas.document_viewport;
        if let Some(crate::dialogs::export::ExportOutcome::Exported(_)) = dialog.show_owned(ui, &self.workspace) {
            self.status_bar.set_hint_key("menu.file.export.export");
        }
        if let Some(error) = &dialog.error {
            self.status_bar.set_hint_key(error.message_key());
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.pull_dock();
        self.show_frame(ui);
        self.sync_dock();
    }

    fn show_frame(&mut self, ui: &mut egui::Ui) {
        self.adopt_core_prefs();
        self.apply_prefs_to_new_contexts();
        self.apply_theme(ui.ctx());
        self.modal_blocking = crate::dialog_chrome::begin_modal_frame(ui.ctx());
        self.modal_blocking |= export_dialog().lock().is_ok_and(|dialog| dialog.is_open());
        self.modal_blocking |= self.exit_review.is_some();
        #[cfg(windows)]
        { self.modal_blocking |= self.print_dialog.is_some(); }
        if !self.input_blocked(ui.ctx()) {
            self.handle_canvas_tab(ui.ctx());
        }
        self.recovery_offer.show(ui, &mut self.workspace);
        self.drop_orphan_dialogs();
        let prompted = self.pending_close_id;
        self.show_mcp_confirms(ui);
        let save_as = crate::dialogs::show_pending_close(ui, &mut self.pending_close_id, &mut self.close_dialog, &mut self.workspace);
        if let Some(id) = save_as {
            self.file_dialog = Some(crate::dialogs::FileDialogState::for_document(crate::dialogs::FileDialogMode::SaveAs, id).closing());
        }
        if let Some(id) = prompted {
            if self.pending_close_id.is_none() && save_as.is_none() && self.workspace.get(id).is_some() {
                self.close_queue.clear();
            }
        }
        if self.file_dialog.is_none() {
            self.advance_close_queue();
        }
        if let Some(session)=self.workspace.session_mut(){
            let seen=self.persona_revision;
            if let Err(error)=crate::persona::reconcile(&mut self.layout,session,&mut self.persona_tools,&mut self.persona_toolbar,&mut self.persona_revision){self.status_bar.set_hint_key(&error.message_key);}
            if seen!=self.persona_revision{self.layout.sync_tree_visibility();}
        }
        self.reconcile_active();
        self.snapping_dialog.show(ui.ctx(), &mut self.workspace);
        if let Some(error) = crate::table::sort_options::show(ui.ctx(), &mut self.workspace) {
            self.status_bar.set_hint_key(&error);
        }
        self.show_console(ui.ctx());
        if self.guides_dialog.open {
            match self.guides_owner.map(crate::dialogs::DialogOwner::Document) {
                Some(owner) => match crate::dialogs::dialog_session(&mut self.workspace, &owner) {
                    Ok(session) => self.guides_dialog.show(ui, session),
                    Err(error) => { self.guides_dialog.open = false; self.guides_owner = None; self.status_bar.set_hint_key(&error.message_key); }
                },
                None => self.guides_dialog.open = false,
            }
        }
        if let Some((owner, id)) = self.panel_registry.layers.effects_target.take() {
            if self.layer_effects.open {
                if let Some(context) = self.layer_effects_owner.and_then(|id| self.workspace.get_mut(id)) { self.layer_effects.cancel(&mut context.session); }
            }
            if self.workspace.get(owner).is_some_and(|c| c.session.document.get_object(dtool_core::ObjectId(id)).is_some()) {
                self.layer_effects_owner = Some(owner);
                self.layer_effects.open_for(id);
            }
        }
        if self.panel_registry.layers.blend_target.take().is_some_and(|owner| self.workspace.active() == Some(owner)) {
            self.open_blend_options();
        }
        match self.pending_context_dialog.take().as_deref() {
            Some(crate::menu_tree::LAYER_EFFECTS_MENU_ID) => {
                if !self.open_layer_effects() {
                    self.status_bar.set_hint_key("error.not_found");
                }
            }
            Some(crate::menu_tree::FIND_LAYER_MENU_ID) => { self.show_selected_layer(false); }
            Some(crate::dialogs::tone_blend::MENU_ID) => self.open_tone_blend(),
            _ => {}
        }
        if self.layer_effects.open {
            match self.layer_effects_owner.map(crate::dialogs::DialogOwner::Document) {
                Some(owner) => match crate::dialogs::dialog_session(&mut self.workspace, &owner) {
                    Ok(session) => self.layer_effects.show(ui.ctx(), session),
                    Err(error) => { self.layer_effects = Default::default(); self.layer_effects_owner = None; self.status_bar.set_hint_key(&error.message_key); }
                },
                None => self.layer_effects.open = false,
            }
        }
        if self.blend_options.open {
            match self.blend_options_owner.map(crate::dialogs::DialogOwner::Document) {
                Some(owner) => match crate::dialogs::dialog_session(&mut self.workspace, &owner) {
                    Ok(session) => self.blend_options.show(ui.ctx(), session),
                    Err(error) => { self.blend_options = Default::default(); self.blend_options_owner = None; self.status_bar.set_hint_key(&error.message_key); }
                },
                None => self.blend_options.open = false,
            }
        }
        if let Some(dialog) = self.tone_blend.as_mut() {
            let keep = self.workspace.get_mut(dialog.owner).is_some_and(|context| dialog.show(ui.ctx(), &mut context.session));
            if !keep { self.tone_blend = None; }
        }
        self.show_preferences(ui);
        self.show_about(ui.ctx());
        self.add_preset.show(ui.ctx(), &mut self.preset_store, &self.layout);
        self.manage_presets.show(ui.ctx(), &mut self.preset_store);
        self.show_floating_documents(ui.ctx());
        if self.feature_finder.open {
            if let Some(session) = self.workspace.session_mut() {
                self.feature_finder.show(ui.ctx(), session);
            }
        }

        self.sync_window_title(ui.ctx());
        self.recent_files.show(ui,&mut self.workspace);
        self.recent_files.sync(&self.workspace);
        let recent=self.workspace.recent().to_vec();
        let documents:Vec<(String,bool)>=crate::window_menu::document_items(&self.workspace,&self.settings).into_iter().map(|(_,label,active)|(label,active)).collect();
        let clipboard_document = crate::os_clipboard::can_create_document(self.workspace.app_clipboard());
        let caption = self.client_title_bar.then(|| self.window_title());
        if let Some(action)=crate::menu_tree::show_queried_menu_bar_with_toolbars(ui,self.workspace.session_mut(),&self.menu_persona,&documents,&recent,self.settings.toolbars,self.layout.columns,clipboard_document,self.last_new_document.is_some(),caption.as_deref()){
            let mode=match action.id.as_str(){
                "file.open"=>Some(crate::dialogs::FileDialogMode::Open),
                "file.template.open"=>Some(crate::dialogs::FileDialogMode::TemplateOpen),
                "file.save_as"=>Some(crate::dialogs::FileDialogMode::SaveAs),
                "file.export.as_template"=>Some(crate::dialogs::FileDialogMode::TemplateSave),
            "file.import_content.from_file_s"=>Some(crate::dialogs::FileDialogMode::ImportText),
                _=>None,
            };
            if let Some(mode)=mode {
                if mode == crate::dialogs::FileDialogMode::Open { self.browse_document(); }
                else { self.open_file_dialog(mode); }
            }
            else if action.id == crate::menu_tree::recent::CLEAR_ID || crate::menu_tree::recent::item_index(&action.id).is_some() {
                match self.recent_files.handle(&mut self.workspace, &action.id) {
                    Ok(crate::menu_tree::recent::RecentOutcome::Opened) => self.status_bar.clear_file_open_error(),
                    Err(error) => self.status_bar.set_file_open_error(&error.message_key),
                    _ => {}
                }
            }
            else if action.id==crate::home::HOME_MENU_ID{self.open_new_document_dialog();}
            else if action.id==crate::dialogs::NEW_DOCUMENT_MENU{self.open_new_document_dialog();}
            else if action.id == crate::dialogs::NEW_FROM_LAST_PRESET_MENU {
                if let Some(state) = self.last_new_document.clone() {
                    self.open_new_document_dialog();
                    self.new_document_dialog = Some(state);
                }
            }
            else if action.id==crate::dialogs::NEW_FROM_FAVOURITES_MENU{self.open_new_document_dialog(); self.new_document_dialog=Some(crate::dialogs::NewDocumentDialogState::from_favourites());}
            else if action.id=="file.place"{if let Err(error)=self.place_tool.open(&mut self.workspace){self.status_bar.set_hint_key(&error.message_key);}}
            else if action.id=="vector.image_trace" {
                if let Some((session, node)) = self.workspace.session()
                    .and_then(|session| dtool_core::commands::image_trace::selected_source(session).map(|node| (session, node)))
                {
                    match self.trace_dialog.open(session, node) {
                        Ok(_) => self.trace_owner = self.workspace.active(),
                        Err(error) => self.status_bar.set_hint_key(&error.message_key),
                    }
                }
            }
            else if action.id == dtool_core::clipboard::move_duplicate::MOVE_DUPLICATE {
                self.open_move_duplicate(ui.ctx());
            }
            else if action.id=="vector.edit_compound" {
                self.compound_edit = crate::dialogs::compound::CompoundEdit::open(&self.workspace);
            }
            else if let Some(menu) = crate::grid_dialog::GridMenu::from_id(&action.id) { self.open_grid_dialog(menu); }
            else if action.id==CUSTOMISE_TOOLBAR_MENU_ID{crate::persona_bar::request_studio(ui.ctx(),crate::studio_editor::StudioRequest::Toolbar);}
            else if action.id == CUSTOMISE_TOOLS_MENU_ID {
                if let Err(error) = self.open_tool_palette(ui.ctx()) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
            else if let Some(toolbar)=crate::settings::ToolbarArea::from_menu_id(&action.id){self.toggle_toolbar(toolbar);}
            else if action.id=="view.toggle_ui"{self.toggle_ui();}
            else if action.id=="file.export.export"{self.open_export_dialog();}
            else if cfg!(windows) && action.id=="file.print" {
                #[cfg(windows)]
                self.open_print_dialog();
            }
            else if self.batch_menu(&action.id){}
            else if let Some(page_action) = crate::menus::pages::Action::from_id(&action.id) {
                if let Err(error) = self.run_page_menu(page_action) { self.status_bar.set_hint_key(&error.message_key); }
            }
            else if action.id==FONT_MANAGER_MENU_ID{if let Err(error)=self.open_font_manager(){self.status_bar.set_hint_key(&error.message_key);}}
            else if action.id==crate::find_host::FIND_MENU_ID{self.find_host.toggle();}
            else if self.spell_host.handle_menu(&action.id,&mut self.workspace){}
            else if crate::menu_tree::TEXT_STYLES_MENU_ROWS.contains(&action.id.as_str()){if let Err(error)=self.run_text_styles_menu(&action.id){self.status_bar.set_hint_key(&error.message_key);}}
            else if crate::menus::text_insert::handles(&action.id){self.insert_special(&action.id);}
            else if crate::snapping_dialog::handles_menu(&action.id,&mut self.snapping_dialog){}
            else if action.id==CONSOLE_MENU_ID{self.console_open=!self.console_open;}
            else if matches!(action.id.as_str(), crate::guides_dialog::SHOW_GUIDES_MANAGER | crate::guides_dialog::MARGINS_AND_GUIDES){self.open_guides_manager();}
            else if action.id==crate::menu_tree::LAYER_EFFECTS_MENU_ID{self.open_layer_effects();}
            else if action.id==crate::menu_tree::BLEND_OPTIONS_MENU_ID{self.open_blend_options();}
            else if action.id==crate::dialogs::tone_blend::MENU_ID{self.open_tone_blend();}
            else if action.id==crate::menu_tree::FIND_LAYER_MENU_ID{self.show_selected_layer(false);}
            else if action.id==crate::menu_tree::RENAME_LAYER_MENU_ID{self.show_selected_layer(true);}
            else if action.id == crate::menu_tree::COLLAPSE_LAYERS_MENU_ID {
                if let Err(error) = self.panel_registry.layers.collapse_all(&mut self.workspace) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
            else if action.id==crate::feature_finder::MENU_ID{self.feature_finder.open();}
            else if action.id==PREFERENCES_MENU_ID{self.open_preferences();}
            else if action.id==RESOURCE_MANAGER_MENU_ID{
                if let Err(error) = self.open_resource_manager() { self.status_bar.set_hint_key(&error.message_key); }
            }
            else if action.id=="file.save_as_package" { self.package_dialog.open=true; }
            else if crate::float::handles(&action.id){self.float_manager.arrange(&action.id,&self.workspace);}
            else if action.id==RESIZE_DOCUMENT_MENU_ID{self.open_resize_document();}
            else if action.id==crate::resize_canvas_dialog::MENU_ID{self.open_resize_canvas();}
            else if action.id==crate::resize_pixel_art_dialog::MENU_ID{self.open_resize_pixel_art();}
            else if action.id==crate::selection_refine_dialog::MENU_ID {
                if let Err(error) = self.open_selection_refine() { self.status_bar.set_hint_key(&error.message_key); }
            }
            else if action.id == "pixel.pixel_selection.save.to_file" {
                self.browse_selection_file(crate::dialogs::FileDialogMode::SelectionSave);
            }
            else if action.id == "pixel.pixel_selection.load_from_file" {
                self.browse_selection_file(crate::dialogs::FileDialogMode::SelectionLoad);
            }
            else if let Some(index) = crate::menu_tree::selection_channel_index(&action.id) {
                self.load_selection_channel(index);
            }
            else if action.id == crate::selection_sampled_colour::MENU_ID {
                if let Err(error) = self.open_selection_sampled_colour() { self.status_bar.set_hint_key(&error.message_key); }
            }
            else if let Some(kind) = crate::selection_modify_dialog::Kind::from_menu(&action.id) {
                if let Err(error) = self.open_selection_modify(kind) { self.status_bar.set_hint_key(&error.message_key); }
            }
            else if action.id==CLOSE_MENU_ID{self.close_active();}
            else if action.id==CLOSE_ALL_MENU_ID{self.close_all_documents();}
            else if action.id==EXIT_MENU_ID{self.request_exit();}
            else if action.id==DOCUMENT_SETUP_MENU_ID{self.open_document_setup();}
            else if action.id==SAVE_MENU_ID&&self.active_untitled(){self.open_file_dialog(crate::dialogs::FileDialogMode::SaveAs);}
            else if crate::presets::open_preset_dialog(&action.id,&mut self.add_preset,&mut self.manage_presets){}
            else if let Some(id)=crate::menu_tree::panels_action(&action.id){self.run_panels_action(id);}
            else if action.id=="edit.select_all"{self.select_all();}
            else if action.id=="edit.delete"{self.delete_selection();}
            else if let Some(id)=crate::panels::window_panel(&action.id){crate::panels::toggle_panel(&mut self.layout,&id);}
            else if let Some(result)=self.longdoc.handle_menu(&action.id,&mut self.workspace){if let Err(error)=result{self.status_bar.set_hint_key(&error.message_key);}}
            else if action.id==crate::about::ABOUT_MENU_ID{self.open_about();}
            else if let Some(index)=crate::menu_tree::window_document_index(&action.id){self.activate_document_index(index);}
            else if let Some(command) = self.workspace.session().and_then(|session| crate::menus::vector_warp::menu_command(session, &action.id)) {
                if let Err(error) = self.run_and_reconcile(command) { self.status_bar.set_hint_key(&error.message_key); }
            }
            else if let Some(bridge)=crate::menu_tree::bridge_action(&action.id){
                if let Err(error)=self.run_bridge_action(bridge){self.status_bar.set_hint_key(&error.message_key);}
            }
            else if action.id == crate::table::sort_options::MENU {
                if let Some(owner) = self.workspace.active() {
                    self.commit_ime_before_switch();
                    crate::table::sort_options::open(ui.ctx(), owner);
                }
            }
            else if crate::menus::text_format::handles(&action.id){
                if let Some(command)=self.workspace.session().and_then(|session|crate::menus::text_format::command(session,&action.id)) {
                    if crate::table::menu::command_name(&action.id).is_some() {
                        if let Some(session) = self.workspace.session_mut() {
                            crate::canvas::frame_text_input::finish_text_input(session);
                        }
                    }
                    if let Err(error)=self.run_and_reconcile(command){self.status_bar.set_hint_key(&error.message_key);}
                }
            }
            else if let Some(binding)=crate::menu_tree::menu_binding(&action.id){
                if let Err(error)=self.run_menu_binding(binding){self.status_bar.set_hint_key(&error.message_key);}
            }
            else if action.id==crate::os_clipboard::PASTE_SPECIAL_MENU{self.open_paste_special();}
            else if let Some(name)=action.command{
                let fixed = action.id.starts_with("vector.geometry.destructive.")
                    || action.id.starts_with("vector.geometry.") && ui.input(|input| input.modifiers.alt);
                let params = if fixed { serde_json::json!({"fixed": true}) } else { serde_json::json!({}) };
                let result=self.run_and_reconcile(Command::with_params(&name, params));
                match result {
                    Ok(_) if matches!(name.as_str(), "edit.copy" | "edit.cut" | dtool_core::clipboard::flattened::COPY_FLATTENED) => {
                        let events = crate::os_clipboard::OsClipboardEvents {
                            copy: name != "edit.cut", cut: name == "edit.cut", ..Default::default()
                        };
                        self.publish_os_clipboard(ui.ctx(), &events);
                    }
                    Err(error) => self.status_bar.set_hint_key(&error.message_key),
                    _ => {}
                }
            }
        }
        self.show_persona_header(ui);
        self.show_studio_windows(ui.ctx());
        let delete_key=!self.input_blocked(ui.ctx())&&self.current_focus(ui.ctx())==Focus::Canvas&&ui.input(|input|input.key_pressed(egui::Key::Delete));
        if delete_key && !self.app_canvas.input.nodes.claims_delete()
            && self.workspace.session().is_some_and(|session| {
                session.text_edit.is_none() && session.active_tool != "tool.stroke_width"
            })
        {
            self.delete_selection();
        }
        if !self.input_blocked(ui.ctx()) { if let Some(session)=self.workspace.session_mut(){if let Some(Err(error))=crate::style_keys::handle(ui.ctx(),session){self.status_bar.set_hint_key(&error.message_key);}} }
        let paste_after_canvas = self.take_text_tool_paste(ui.ctx());
        let os_clip=if !self.input_blocked(ui.ctx())&&self.current_focus(ui.ctx())==Focus::Canvas{ui.input(crate::os_clipboard::OsClipboardEvents::read)}else{Default::default()};
        if !os_clip.paste_image && let Some(text) = os_clip.paste.as_deref() {
            self.begin_table_paste(text);
        }
        if !self.input_blocked(ui.ctx()) && self.current_focus(ui.ctx()) == Focus::Canvas
            && ui.input(crate::table::canvas::starts_cell_text)
        {
            self.begin_table_cell_edit();
        }
        if let Some(name)=self.workspace.session_mut().and_then(|session|crate::os_clipboard::before_shortcuts(session,&os_clip)){if let Err(error)=self.run_and_reconcile(Command::with_params(name,serde_json::json!({}))){self.status_bar.set_hint_key(&error.message_key);}}
        if let Some(context)=self.locked_delete.owner.and_then(|id|self.workspace.get_mut(id)){
            if let Some(Err(error))=self.locked_delete.show(ui,&mut context.session){self.status_bar.set_hint_key(&error.message_key);}
        }
        self.show_resize_document(ui.ctx());
        self.show_selection_refine(ui.ctx());
        self.show_selection_modify(ui.ctx());
        self.show_selection_sampled_colour(ui.ctx());
        self.show_resize_canvas(ui.ctx());
        self.show_resize_pixel_art(ui.ctx());
        self.show_document_setup(ui);
        if let Some(context)=self.grid_and_axis.owner.and_then(|id|self.workspace.get_mut(id)){
            self.grid_and_axis.show(ui.ctx(),&mut context.session);
        }
        if let Some(context)=self.baseline_grid.owner.and_then(|id|self.workspace.get_mut(id)){
            self.baseline_grid.show(ui.ctx(),&mut context.session);
        }
        if let Some(session)=self.workspace.session_mut(){
            crate::menus::file_menu::show_text_link_prompt(ui.ctx(),session);
        }
        if let Some(id)=self.resource_manager.owner && let Some(context)=self.workspace.get_mut(id) {
            let dir=context.path.as_deref().and_then(std::path::Path::parent);
            if let Some(found)=self.resource_manager.show(ui.ctx(),&mut context.session,dir) {
                let session=&mut context.session;
                if let (Some(viewport),Some(bounds))=(session.view_canvas_size,dtool_core::command::selection_bounds(&session.document,&[found])) {
                    dtool_core::view::center_region(&mut session.view,bounds,viewport);
                    context.view_state=session.view.clone();
                }
                if self.workspace.active()!=Some(id) {
                    let _=self.workspace.activate(id);
                }
                self.reconcile_active();
            }
        }
        if let Some(command) = crate::dialogs::package::show_package_dialog(ui, &mut self.package_dialog) {
            let result = self.run_and_reconcile(command);
            self.package_dialog.apply_result(result);
        }
        if !self.input_blocked(ui.ctx()) && self.workspace.session_mut().is_some() && export_shortcut_pressed(ui.ctx(), &self.shortcuts) {
            self.open_export_dialog();
        }
        self.show_export_dialog(ui);
        #[cfg(windows)]
        {
            if !self.dialog_open(ui.ctx()) && self.workspace.session().is_some()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::P))
            {
                self.open_print_dialog();
            }
            if let Some(dialog) = &mut self.print_dialog {
                self.modal_blocking = true;
                if dialog.show(ui, &self.workspace) {
                    self.print_dialog = None;
                }
            }
        }
        self.batch_host.show(ui.ctx(), self.workspace.session_mut());
        self.show_font_manager(ui);
        self.show_page_menu_dialog(ui.ctx());
        if self.find_host.show(ui.ctx(), &mut self.workspace) {
            self.reconcile_active();
        }
        if self.spell_host.show(ui.ctx(), &mut self.workspace) {
            self.reconcile_active();
        }
        self.place_tool.show_dialog(ui, &mut self.workspace);
        if let Some(preset) = self.panel_registry.pixel_brushes.edit_target.take() {
            if let Err(error) = self.open_brush_editor(Some(preset)) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
        if let Some(editor) = &mut self.brush_editor {
            self.modal_blocking = true;
            if editor.show(ui.ctx(), &mut self.workspace) {
                self.brush_editor = None;
                self.context_model = None;
            }
        }
        if self.compound_edit.as_ref().is_some_and(|edit| Some(edit.owner) != self.workspace.active()) {
            self.compound_edit = None;
        }
        if let Some(edit) = &mut self.compound_edit {
            self.modal_blocking = true;
            let (cancel, command) = edit.show(ui.ctx());
            if cancel {
                self.compound_edit = None;
            } else if let Some(command) = command {
                match self.run_and_reconcile(command) {
                    Ok(_) => self.compound_edit = None,
                    Err(error) => self.status_bar.set_hint_key(&error.message_key),
                }
            }
        }
        if self.move_duplicate.as_ref().is_some_and(|dialog| !dialog.matches(&self.workspace)) {
            self.move_duplicate = None;
        }
        if let (Some(dialog), Some(session)) = (&mut self.move_duplicate, self.workspace.session()) {
            let (cancel, command) = dialog.show(ui.ctx(), session);
            if cancel {
                self.move_duplicate = None;
            } else if let Some(command) = command {
                match self.run_and_reconcile(command) {
                    Ok(_) => self.move_duplicate = None,
                    Err(error) => self.status_bar.set_hint_key(&error.message_key),
                }
            }
        }
        if self.trace_dialog.is_open() {
            if self.workspace.active() != self.trace_owner {
                self.trace_dialog.close();
            } else if let Some(session) = self.workspace.session_mut() {
                self.modal_blocking = true;
                if self.trace_dialog.show(ui, session) { self.reconcile_active(); }
            }
        }
        let dropped: Vec<std::path::PathBuf> = ui.input(|input| input.raw.dropped_files.iter().map(|file| file.path().to_path_buf()).collect());
        if !dropped.is_empty() && !self.input_blocked(ui.ctx()) {
            if let Err(error) = self.place_tool.drop_files(&mut self.workspace, &dropped) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
        if let Some(state) = &mut self.file_dialog {
            use crate::dialog_chrome::DialogResult;
            let result = crate::dialogs::file_dialog_result(ui, state, &mut self.workspace);
            if result != DialogResult::Open {
                if state.close_after_save && result == DialogResult::Cancel {
                    self.close_queue.clear();
                }
                if matches!(state.mode, crate::dialogs::FileDialogMode::Open | crate::dialogs::FileDialogMode::TemplateOpen)
                    && result == DialogResult::Confirm
                {
                    self.new_document_dialog = None;
                    self.status_bar.clear_file_open_error();
                }
                let loaded_selection = state.mode == crate::dialogs::FileDialogMode::SelectionLoad
                    && result == DialogResult::Confirm;
                self.file_dialog = None;
                if loaded_selection { self.reconcile_active(); }
            }
        }
        if let Some(review) = &mut self.exit_review {
            use crate::dialog_chrome::DialogResult;
            let result = review.show(ui, &mut self.workspace);
            if result != DialogResult::Open {
                self.exit_review = None;
                let approved = result == DialogResult::Confirm;
                if self.update_indicator.finish_exit(approved) && approved {
                    self.exit_approved = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if self.file_dialog.is_none() && self.exit_review.is_none() && !self.update_indicator.dialog_open()
            && let Some(state) = &mut self.new_document_dialog {
            use crate::dialogs::NewDocumentOutcome;
            let mut action = None;
            let result = crate::dialogs::new_document_dialog_result(ui, state, &mut self.workspace,
                Some((&mut self.template_browser, &self.prefs)), &mut action);
            if result != NewDocumentOutcome::Open {
                if result == NewDocumentOutcome::Created {
                    let last_used = state.last_used_snapshot();
                    if last_used.save_last_used().is_err() {
                        self.status_bar.set_hint_key("error.io_error");
                    }
                    self.last_new_document = Some(last_used);
                }
                self.new_document_dialog = None;
                if matches!(result, NewDocumentOutcome::Created | NewDocumentOutcome::OpenedRecent) {
                    self.status_bar.clear_file_open_error();
                    self.reconcile_active();
                }
            }
            if let Some(action) = action { self.handle_template_action(action); }
        }
        if let Some(state) = &mut self.paste_special_dialog {
            let done = match self.workspace.session_mut() { Some(session) => crate::dialogs::show_paste_special_dialog(ui, state, session).is_some(), None => true };
            if done { self.paste_special_dialog = None; }
        }
        if self.update_indicator.show_dialog(ui.ctx()) {
            self.request_exit();
            ui.ctx().request_repaint();
        }

        let ids: Vec<_> = self.workspace.contexts().iter().map(|ctx| ctx.id).collect();
        for id in ids {
            if let Some(ctx) = self.workspace.get_mut(id) {
                let name = std::format!("{}-{}", std::process::id(), id.0);
                if let Err(error) = dtool_core::command::autosave_commands::poll(&mut ctx.session, &name, ctx.path.as_deref()) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));

        self.focus = self.current_focus(ui.ctx());
        crate::shortcuts::publish_focus(self.focus);
        crate::shortcuts::sync_shared(&mut self.shortcuts, &mut self.shortcut_seen);
        self.ingest_ime(ui.ctx());
        if !self.input_blocked(ui.ctx()) {
            self.handle_shortcuts(ui.ctx());
        }
        if os_clip.copy || os_clip.cut {
            self.publish_os_clipboard(ui.ctx(), &os_clip);
        }
        self.finish_ime_on_switch();
        tabs::tab_row(self, ui);
        self.show_context_region(ui);
        self.show_tools_column(ui);
        self.show_tool_paint_editor(ui.ctx());
        self.show_dock(ui);
        self.status_bar.stats_text = self.workspace.session().and_then(|s| s.document()).and_then(crate::status_bar_stats::document_stats_text);
        self.longdoc.show(ui.ctx(), &mut self.workspace, &mut self.status_bar);
        self.sync_status_inputs(ui.ctx());
        self.sync_status_chrome();
        self.status_bar.show(ui);
        self.navigation_canvas(ui);
        if let Some(text) = paste_after_canvas {
            self.paste_after_text_tool_click(ui.ctx(), text);
        }
        if let Some(key)=self.app_canvas.input.take_error_key(){self.status_bar.set_hint_key(&key);}
        self.refresh_table_views();
        self.finish_ime_on_switch();
        self.reconcile_active();
        self.warm_spelling_when_idle(ui.ctx());
        self.frame += 1;
    }

    pub fn warm_spelling_when_idle(&mut self, ctx: &egui::Context) {
        if self.spell_warm_requested || self.frame == 0 {
            return;
        }
        if ctx.input(|i| i.events.is_empty() && !i.pointer.any_down()) {
            dtool_tools::dtool_text::spelling::warm_korean();
            self.spell_warm_requested = true;
        }
    }
}

impl AppShell {
    fn show_studio_windows(&mut self, ctx: &egui::Context) {
        let mut run = crate::persona::host_action;
        if let Some(request) = crate::persona_bar::take_studio_request(ctx) {
            self.studio_windows.open(request, &mut run);
        }
        if let Some(command) = self.studio_windows.show_with_icons(ctx, &mut run, Some(&mut self.icon_cache)) {
            self.commit_ime_before_switch();
            let result = if self.workspace.session().is_some() {
                self.run_and_reconcile(command)
            } else {
                crate::persona::host_action(&command).inspect(|_| {
                    if let Some(layout) = crate::persona::live_dock() { self.layout = layout; }
                })
            };
            if let Err(error) = result {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
    }

    fn take_text_tool_paste(&self, ctx: &egui::Context) -> Option<String> {
        use crate::canvas_input::{tool_kind, ToolKind};
        if self.input_blocked(ctx)
            || self.widget_focused(ctx)
            || !self.workspace.session().is_some_and(|session| matches!(tool_kind(&session.active_tool), ToolKind::ArtText | ToolKind::Table))
        {
            return None;
        }
        let canvas = ctx.read_response(self.canvas_id?)?.interact_rect;
        ctx.input_mut(|input| crate::os_clipboard::take_paste_after_click(input, canvas))
    }

    fn begin_table_paste(&mut self, text: &str) {
        if !text.is_empty() && !text.contains('\t') {
            self.begin_table_cell_edit();
        }
    }

    fn begin_table_cell_edit(&mut self) {
        if let Some(session) = self.workspace.session_mut()
            && let Err(error) = crate::table::canvas::begin_selected_cell_edit(session)
        {
            self.status_bar.set_hint_key(&error.message_key);
        }
    }

    fn paste_after_text_tool_click(&mut self, ctx: &egui::Context, text: String) {
        if self.current_focus(ctx) == Focus::Canvas {
            self.begin_table_paste(&text);
        }
        if self.canvas_owns_text(ctx) {
            if let Some(session) = self.workspace.session_mut() {
                if crate::canvas::frame_text_input::feed_text_input(session, &[egui::Event::Paste(text)]) {
                    ctx.request_repaint();
                }
            }
        } else if self.current_focus(ctx) == Focus::Canvas {
            let events = crate::os_clipboard::OsClipboardEvents { paste: Some(text), ..Default::default() };
            if let Some(command) = self.workspace.session_mut().and_then(|session| crate::os_clipboard::before_shortcuts(session, &events)) {
                if let Err(error) = self.run_and_reconcile(Command::new(command)) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
        }
    }

    pub fn ui_frame(&self, ctx: &egui::Context) -> crate::ui_host::UiFrame {
        let size = ctx.content_rect().size();
        crate::ui_host::UiFrame {
            frame: self.frame,
            context_id: self.workspace.active(),
            generation: self.workspace.session().map(|session| session.generation).unwrap_or(0),
            viewport: [size.x.max(0.0).round() as u32, size.y.max(0.0).round() as u32],
        }
    }

    pub fn ui_state(&self, ctx: &egui::Context) -> Value {
        let active = self.workspace.active();
        let documents: Vec<Value> = self.workspace.contexts().iter().map(|context| serde_json::json!({
            "id": context.id.0,
            "title": context.title,
            "modified": context.is_modified(),
            "active": Some(context.id) == active,
        })).collect();
        let dock: serde_json::Map<String, Value> = DockSide::ALL.into_iter().map(|side| (format!("{side:?}").to_lowercase(), Value::Bool(!self.ui_hidden() && self.layout.columns.is_shown(side)))).collect();
        serde_json::json!({
            "active_tool": self.workspace.session().map(|session| session.active_tool.clone()),
            "text_editing": self.text_editing(),
            "focus": format!("{:?}", self.current_focus(ctx)),
            "startup_errors": self.startup_errors,
            "modal_open": self.input_blocked(ctx),
            "menu_persona": self.menu_persona,
            "ui_hidden": self.ui_hidden(),
            "toolbars": {
                "top": self.top_toolbar_visible(),
                "context": self.context_toolbar_visible(),
                "tools": self.tools_column_visible(),
            },
            "status_bar_visible": self.settings.status_bar_visible,
            "documents": documents,
            "dock": dock,
            "presets": self.preset_store.list(),
        })
    }

    pub fn run_ui_dock_command(&mut self, name: &str, params: &Value) -> Option<dtool_core::CommandResult> {
        self.pull_dock();
        let path = crate::presets::PresetStore::persisted_path();
        let constraints_state = matches!(name, "ui.panel.state" | "ui.panel.query")
            && params.get("id").or_else(|| params.get("panel")).and_then(Value::as_str) == Some("constraints");
        let mut normalized = params.clone();
        if constraints_state {
            normalized["id"] = serde_json::json!("constraints");
        }
        let mut result = crate::panels::run_ui_dock_command(&mut self.layout, &self.panel_registry, &mut self.preset_store, name, &normalized, Some(&path))?;
        if constraints_state && let Ok(value) = &mut result {
            value["state"] = self.workspace.session().map(dtool_core::constraints::status)
                .unwrap_or_else(|| serde_json::json!({"available": false, "mixed": false, "layers": []}));
        }
        self.push_dock();
        if let Err(error) = &result { self.status_bar.set_hint_key(&error.message_key); }
        Some(result)
    }

    pub fn apply_host_request(&mut self, ctx: &egui::Context, request: dtool_core::workspace::HostRequest) {
        let name = request.command.name.clone();
        let menu_id = request.command.params["id"].as_str();
        if name == crate::panel_actions::INVOKE && let Some(id) = menu_id.and_then(crate::menu_tree::panels_action) {
            let frame = self.ui_frame(ctx);
            let result = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id)
                .and_then(|()| {
                    if self.ui_hidden() || self.input_blocked(ctx) {
                        return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                    }
                    self.apply_panels_action(id)?;
                    self.layout.layout_json()
                });
            crate::ui_host::reply(request, &frame, result);
            return;
        }
        if name == crate::panel_actions::INVOKE && menu_id.is_some_and(crate::menu_tree::host_layers_action) {
            let frame = self.ui_frame(ctx);
            let result = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id)
                .and_then(|()| {
                    frame.context_id.ok_or_else(dtool_core::CommandError::no_document)?;
                    if self.input_blocked(ctx) {
                        return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                    }
                    if menu_id == Some(crate::menu_tree::COLLAPSE_LAYERS_MENU_ID) {
                        return self.panel_registry.layers.collapse_all(&mut self.workspace);
                    }
                    let rename = menu_id == Some(crate::menu_tree::RENAME_LAYER_MENU_ID);
                    let id = self.show_selected_layer(rename)
                        .ok_or_else(|| dtool_core::CommandError::new("CANNOT_EXECUTE"))?;
                    self.push_dock();
                    Ok(serde_json::json!({"id": id.0}))
                });
            crate::ui_host::reply(request, &frame, result);
            return;
        }
        if name == crate::panel_actions::INVOKE && menu_id == Some(CUSTOMISE_TOOLBAR_MENU_ID) {
            let frame = self.ui_frame(ctx);
            let result = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id)
                .and_then(|()| {
                    if !self.studio_windows.toolbar_open() {
                        if self.ui_hidden() || self.input_blocked(ctx) {
                            return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                        }
                        self.studio_windows.open(crate::studio_editor::StudioRequest::Toolbar, &mut crate::persona::host_action);
                    }
                    if !self.studio_windows.toolbar_open() {
                        return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                    }
                    Ok(serde_json::json!({ "open": true }))
                });
            crate::ui_host::reply(request, &frame, result);
            return;
        }
        if name == crate::panel_actions::INVOKE && menu_id == Some(crate::dialogs::NEW_DOCUMENT_MENU) {
            let frame = self.ui_frame(ctx);
            let result = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id)
                .and_then(|()| {
                    if self.new_document_dialog.is_none() {
                        if self.input_blocked(ctx) {
                            return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                        }
                        self.open_new_document_dialog();
                    }
                    Ok(serde_json::json!({ "open": true }))
                });
            crate::ui_host::reply(request, &frame, result);
            return;
        }
        if name == crate::panel_actions::INVOKE
            && let Some(crate::longdoc_host::MenuRoute::Panel(panel)) = menu_id.and_then(crate::longdoc_host::route)
        {
            let frame = self.ui_frame(ctx);
            let result = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id)
                .and_then(|()| {
                    frame.context_id.ok_or_else(dtool_core::CommandError::no_document)?;
                    if !self.longdoc.is_open(panel) {
                        if self.input_blocked(ctx) {
                            return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                        }
                        self.longdoc.open_panel(panel);
                    }
                    Ok(serde_json::json!({ "open": true, "panel": panel }))
                });
            crate::ui_host::reply(request, &frame, result);
            return;
        }
        let guides = matches!(menu_id, Some(crate::guides_dialog::SHOW_GUIDES_MANAGER | crate::guides_dialog::MARGINS_AND_GUIDES));
        if name == crate::panel_actions::INVOKE
            && menu_id.is_some_and(crate::menu_tree::host_document_dialog)
        {
            let frame = self.ui_frame(ctx);
            let result = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id)
                .and_then(|()| {
                    let owner = frame.context_id.ok_or_else(dtool_core::CommandError::no_document)?;
                    if menu_id == Some(CUSTOMISE_TOOLS_MENU_ID) {
                        self.open_tool_palette(ctx)?;
                        return Ok(serde_json::json!({ "open": true }));
                    }
                    if menu_id == Some(crate::resize_canvas_dialog::MENU_ID) {
                        if !self.resize_canvas.as_ref().is_some_and(|dialog| dialog.owner == owner) {
                            if self.input_blocked(ctx) {
                                return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                            }
                            self.open_resize_canvas();
                        }
                        return Ok(serde_json::json!({ "open": self.resize_canvas.is_some() }));
                    }
                    if menu_id == Some(crate::menu_tree::LAYER_EFFECTS_MENU_ID) {
                        let id = self.workspace.session().and_then(crate::mask_actions::target)
                            .ok_or_else(|| dtool_core::CommandError::new("CANNOT_EXECUTE"))?.0;
                        let already_open = self.layer_effects.open
                            && self.layer_effects_owner == Some(owner)
                            && self.layer_effects.object_id == Some(id);
                        if !already_open
                            && (self.layer_effects.open || self.input_blocked(ctx) || !self.open_layer_effects())
                        {
                            return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                        }
                        return Ok(serde_json::json!({ "open": true, "id": id }));
                    }
                    if menu_id == Some(crate::menu_tree::BLEND_OPTIONS_MENU_ID) {
                        if !self.blend_options.open || self.blend_options_owner != Some(owner) {
                            if self.input_blocked(ctx) || !self.open_blend_options() {
                                return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                            }
                        }
                        return Ok(serde_json::json!({ "open": true }));
                    }
                    let manager = match menu_id {
                        Some(FONT_MANAGER_MENU_ID) => Some((
                            self.font_manager.open && self.font_manager_owner == Some(owner),
                            Self::open_font_manager as fn(&mut Self) -> Result<(), dtool_core::CommandError>,
                        )),
                        Some(RESOURCE_MANAGER_MENU_ID) => Some((
                            self.resource_manager.open && self.resource_manager.owner == Some(owner),
                            Self::open_resource_manager as fn(&mut Self) -> Result<(), dtool_core::CommandError>,
                        )),
                        _ => None,
                    };
                    if let Some((open, open_manager)) = manager {
                        if !open {
                            if self.input_blocked(ctx) {
                                return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                            }
                            open_manager(self)?;
                        }
                        return Ok(serde_json::json!({ "open": true }));
                    }
                    if let Some(menu) = menu_id.and_then(crate::grid_dialog::GridMenu::from_id) {
                        if !self.grid_dialog_open_for(menu, owner) && self.input_blocked(ctx) {
                            return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
                        }
                        return Ok(serde_json::json!({ "open": self.open_grid_dialog(menu) }));
                    }
                    let percent = if guides {
                        request.command.params.get("percent").map(|value| value.as_bool().ok_or_else(dtool_core::CommandError::invalid_params)).transpose()?
                    } else { None };
                    let open = if guides { self.guides_dialog.open && self.guides_owner == frame.context_id } else { self.document_setup.is_open() };
                    if !open {
                        if self.input_blocked(ctx) { return Err(dtool_core::CommandError::new("CANNOT_EXECUTE")); }
                        if guides { self.open_guides_manager(); } else { self.open_document_setup(); }
                    }
                    if guides {
                        if let Some(percent) = percent { self.guides_dialog.set_percent_positions(percent); }
                        Ok(serde_json::json!({ "open": self.guides_dialog.open, "percent": self.guides_dialog.percent_positions() }))
                    } else {
                        Ok(serde_json::json!({ "open": self.document_setup.is_open() }))
                    }
                });
            crate::ui_host::reply(request, &frame, result);
            return;
        }
        if crate::ui_host::is_ui_thread_command(&name) {
            self.answer_ui_request(ctx, request);
            return;
        }
        if crate::ui_host::is_dock_command(&name) {
            if let Some(result) = self.run_ui_dock_command(&name, &request.command.params) {
                let frame = self.ui_frame(ctx);
                crate::ui_host::reply(request, &frame, result);
                return;
            }
        }
        let previous_context = self.workspace.active();
        let waiting_for_document = self.new_document_dialog.is_some()
            && (previous_context.is_none() || name == "doc.new");
        self.workspace.apply_host_request(request);
        let active = self.workspace.active();
        if waiting_for_document && active.is_some() && active != previous_context {
            self.new_document_dialog = None;
            self.reconcile_active();
        }
    }

    pub fn answer_ui_request(&mut self, ctx: &egui::Context, request: dtool_core::workspace::HostRequest) {
        let frame = self.ui_frame(ctx);
        if let Err(error) = crate::ui_host::check_context(&request.command.params, request.scope.context, frame.context_id) {
            crate::ui_host::reply(request, &frame, Err(error));
            return;
        }
        if request.command.name == crate::ui_host::UI_STATE_QUERY {
            let state = self.ui_state(ctx);
            crate::ui_host::reply(request, &frame, Ok(crate::ui_host::state_reply(&frame, state)));
            return;
        }
        let capture = match crate::ui_host::parse_capture(&request.command.params) {
            Ok(capture) => capture,
            Err(error) => { crate::ui_host::reply(request, &frame, Err(error)); return; }
        };
        if ctx.input(|input| input.viewport().minimized == Some(true)) {
            crate::ui_host::reply(request, &frame, Err(dtool_core::CommandError::no_window()));
            return;
        }
        let next = crate::ui_host::UiFrame { frame: frame.frame + 1, ..frame };
        crate::ui_host::request_capture(ctx, request, next, capture);
    }

    pub fn drain_host(&mut self, ctx: &egui::Context, mut next: impl FnMut() -> Option<dtool_core::workspace::HostRequest>, budget: usize) -> usize {
        let mut applied = 0;
        while applied < budget {
            let Some(request) = next() else { break };
            self.apply_host_request(ctx, request);
            applied += 1;
        }
        applied
    }
}

impl AppShell {
    pub fn open_new_document_dialog(&mut self) {
        self.template_browser.refresh_templates(&self.prefs.home_template_folders);
        self.template_browser.error = None;
        self.new_document_dialog = Some(Default::default());
    }

    fn handle_template_action(&mut self, action: crate::home::HomeAction) {
        use crate::home::HomeAction;
        match action {
            HomeAction::Browse => {
                if self.browse_document() { self.new_document_dialog = None; }
            }
            HomeAction::BrowseTemplate => {
                self.template_browser.error = None;
                #[cfg(windows)]
                {
                    use crate::dialogs::{FileDialogMode, pick_file_path};
                    let extension = dtool_core::commands::document_files::TEMPLATE_EXTENSION;
                    match pick_file_path(dtool_strings::text(FileDialogMode::TemplateOpen.title_key()),
                        dtool_strings::text("dialog.file.filter.template"), extension, None)
                    {
                        Ok(Some(path)) => self.open_home_template(&path),
                        Ok(None) => {}
                        Err(_) => self.open_template_dialog(),
                    }
                }
                #[cfg(not(windows))]
                self.open_template_dialog();
            }
            HomeAction::AddTemplateFolder(path) => {
                match path.canonicalize().and_then(|folder| std::fs::read_dir(&folder).map(|_| folder)) {
                    Ok(folder) => {
                        let mut folders = self.prefs.home_template_folders.clone();
                        if !folders.contains(&folder) { folders.push(folder); }
                        if self.save_home_template_folders(folders) {
                            self.template_browser.clear_template_folder_input();
                        }
                    }
                    Err(_) => self.template_browser.error = Some(dtool_strings::text("home.template.invalid_folder").into()),
                }
            }
            HomeAction::RemoveTemplateFolder(path) => {
                let mut folders = self.prefs.home_template_folders.clone();
                folders.retain(|folder| folder != &path);
                self.save_home_template_folders(folders);
            }
            HomeAction::Open { section, item } if section == "templates" => {
                self.open_home_template(std::path::Path::new(&item));
            }
            _ => {}
        }
        if let Some(state) = self.new_document_dialog.as_mut() {
            state.error_message = self.template_browser.error.take();
        }
    }

    fn save_home_template_folders(&mut self, folders: Vec<std::path::PathBuf>) -> bool {
        let mut prefs = self.prefs.clone();
        prefs.home_template_folders = folders;
        if prefs.save(self.prefs_dialog.prefs_path()).is_err() {
            self.template_browser.error = Some(dtool_strings::text("error.io_error").into());
            return false;
        }
        self.prefs = prefs;
        self.push_core_prefs();
        self.template_browser.error = None;
        self.template_browser.refresh_templates(&self.prefs.home_template_folders);
        true
    }

    fn open_home_template(&mut self, path: &std::path::Path) {
        let result = self.run_and_reconcile(Command::with_params("doc.template.open", serde_json::json!({"path": path})));
        if result.is_ok() { self.new_document_dialog = None; }
        self.template_browser.error = result.err().map(|error| dtool_strings::text(&error.message_key).to_string());
    }

    pub(crate) fn browse_document(&mut self) -> bool {
        #[cfg(windows)]
        match crate::dialogs::pick_document_path() {
            Ok(Some(path)) => {
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) {
                    let mut dialog = crate::dialogs::FileDialogState::new(crate::dialogs::FileDialogMode::Open);
                    dialog.path = path.to_string_lossy().into_owned();
                    self.file_dialog = Some(dialog);
                    return true;
                }
                match self.run_and_reconcile(Command::with_params("doc.open", json!({ "path": path }))) {
                    Ok(_) => {
                        self.status_bar.clear_file_open_error();
                        return true;
                    }
                    Err(error) => self.status_bar.set_file_open_error(&error.message_key),
                }
                return false;
            }
            Ok(None) => {
                return false;
            }
            Err(_) => self.status_bar.set_file_open_error("error.io_error"),
        }
        self.open_file_dialog(crate::dialogs::FileDialogMode::Open);
        false
    }

    pub fn dialog_open(&self, ctx: &egui::Context) -> bool {
        self.input_blocked(ctx)
            || self.update_indicator.dialog_open()
            || self.exit_review.is_some()
            || self.file_dialog.is_some()
            || self.new_document_dialog.is_some()
            || self.pending_close_id.is_some()
            || self.close_dialog.is_some()
            || self.mcp_confirm.is_some()
            || self.resource_manager.open
            || self.locked_delete.is_open()
            || self.document_setup.is_open()
            || self.resize_document.open
            || self.resize_canvas.is_some()
            || self.resize_pixel_art.is_some()
            || self.selection_refine.is_some()
            || self.selection_modify.is_some()
            || self.prefs_dialog.open
            || self.about.open
            || self.studio_windows.is_open()
            || self.add_preset.open
            || self.manage_presets.open
            || self.batch_host.batch_open()
            || self.font_manager.open
            || self.page_menu_dialog.is_some()
    }

    pub fn insert_special(&mut self, id: &str) -> bool {
        let done = self.workspace.session_mut().is_some_and(|session| crate::menus::text_insert::insert_at_caret(session, id).is_ok());
        if done {
            self.reconcile_active();
        }
        done
    }

    pub fn batch_menu(&mut self, id: &str) -> bool {
        self.batch_host.handle_menu(id)
    }

    fn run_page_menu(&mut self, action: crate::menus::pages::Action) -> Result<(), dtool_core::CommandError> {
        use crate::menus::pages::{self, Action};

        let owner = self.workspace.active().ok_or_else(dtool_core::CommandError::no_document)?;
        let session = self.workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
        if !action.enabled(session) {
            return Err(dtool_core::CommandError::invalid_params());
        }
        match action {
            Action::Add | Action::Delete | Action::GoTo => {
                self.page_menu_dialog = Some(pages::Dialog::new(owner, action, session));
                self.app_canvas.input.cancel();
            }
            Action::New => self.apply_page_menu_command(pages::new_page(session))?,
            Action::Import => {
                #[cfg(windows)]
                {
                    if let Some(path) = crate::dialogs::pick_file_path_with_extensions(
                        text("menu.document.pages.add_from_file"), text("dialog.file.filter.native"), &["dtool"], None,
                    ).map_err(|_| dtool_core::CommandError::new("IO.OTHER"))? {
                        self.page_menu_dialog = Some(pages::Dialog::from_file(owner, session, path)?);
                        self.app_canvas.input.cancel();
                    }
                }
                #[cfg(not(windows))]
                return Err(dtool_core::CommandError::not_implemented());
            }
            Action::Masters => {
                let caret = dtool_core::commands::pages::viewed_page(session);
                if pages::viewed_master(session).is_some() {
                    let id = dtool_core::commands::pages::previous_viewed_page(session)
                        .or_else(|| session.document.pages.first().map(|page| page.id))
                        .ok_or_else(dtool_core::CommandError::not_found)?;
                    self.run_and_reconcile(Command::with_params("page.navigate", json!({"id":id})))?;
                    self.panel_registry.pages.show_master_tab(false);
                } else {
                    let doc = &session.document;
                    let id = caret.and_then(|page| doc.master_pages.for_page(page).map(|assignment| assignment.master))
                        .or_else(|| doc.master_pages.roots.first().copied())
                        .ok_or_else(dtool_core::CommandError::not_found)?;
                    self.navigate_to_master(id)?;
                }
                crate::panels::open_panel(&mut self.layout, &crate::dock::PanelId::Pages);
            }
            _ => {
                if let Some(command) = action.navigation() {
                    self.apply_page_menu_command(command)?;
                }
            }
        }
        Ok(())
    }

    fn navigate_to_master(&mut self, id: dtool_core::object::ObjectId) -> Result<(), dtool_core::CommandError> {
        let session = self.workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
        let bounds = dtool_core::view::ViewTarget::Artboard(id).bounds(&session.document)
            .ok_or_else(dtool_core::CommandError::not_found)?;
        let size = session.view_canvas_size.ok_or_else(dtool_core::CommandError::invalid_params)?;
        let mut centered = session.view.clone();
        dtool_core::view::center_region(&mut centered, bounds, size);
        let delta = [centered.scroll[0] - session.view.scroll[0], centered.scroll[1] - session.view.scroll[1]];
        self.run_and_reconcile(Command::with_params("view.pan", json!({"delta": delta})))?;
        self.panel_registry.pages.show_master_tab(true);
        Ok(())
    }

    fn apply_page_menu_command(&mut self, command: Command) -> Result<(), dtool_core::CommandError> {
        let added = matches!(command.name.as_str(), "page.add" | "page.import");
        let result = self.run_and_reconcile(command)?;
        if added && let Some(id) = result["id"].as_u64() {
            self.run_and_reconcile(Command::with_params("page.navigate", json!({"id":id})))?;
        }
        if result["omitted_links"].as_u64().is_some_and(|count| count > 0) {
            self.status_bar.set_hint_key("status.import_omitted_links");
        }
        self.panel_registry.pages.show_master_tab(false);
        Ok(())
    }

    fn show_page_menu_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.page_menu_dialog.take() else { return; };
        if self.workspace.active() != Some(dialog.owner) {
            return;
        }
        let Some(session) = self.workspace.session() else { return; };
        let (open, command) = dialog.show(ctx, session);
        if open {
            self.page_menu_dialog = Some(dialog);
        } else if let Some(command) = command {
            let importing = command.name == "page.import";
            if let Err(error) = self.apply_page_menu_command(command) {
                if importing {
                    dialog.error = Some(error.message_key);
                    self.page_menu_dialog = Some(dialog);
                } else {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
        }
    }

    pub fn open_font_manager(&mut self) -> Result<(), dtool_core::CommandError> {
        let id = self.workspace.active().ok_or_else(dtool_core::CommandError::no_document)?;
        let context = self.workspace.get_mut(id).ok_or_else(dtool_core::CommandError::no_document)?;
        self.font_replace.hide();
        self.font_manager.open_for(&mut context.session)?;
        self.font_manager_owner = Some(id);
        Ok(())
    }

    fn open_resource_manager(&mut self) -> Result<(), dtool_core::CommandError> {
        let context = self.workspace.active().and_then(|id| self.workspace.get(id))
            .ok_or_else(dtool_core::CommandError::no_document)?;
        self.resource_manager.open_for(context.id, &context.session);
        Ok(())
    }

    pub fn font_manager_action(&mut self, action: crate::font_manager::FontManagerAction) -> Result<(), dtool_core::CommandError> {
        use crate::font_manager::FontManagerAction;
        match action {
            FontManagerAction::Close => {
                self.font_replace.hide();
                self.font_manager.hide();
                self.font_manager_owner = None;
                return Ok(());
            }
            FontManagerAction::CancelReplace => {
                self.font_manager.cancel_replace(&mut self.font_replace);
                return Ok(());
            }
            FontManagerAction::OpenReplace => {
                self.font_manager.begin_replace(&mut self.font_replace);
                return Ok(());
            }
            FontManagerAction::Locate | FontManagerAction::ConfirmReplace | FontManagerAction::Substitute { .. } => {}
        }
        let id = self.font_manager_owner.ok_or_else(dtool_core::CommandError::no_document)?;
        if matches!(action, FontManagerAction::Locate) && self.workspace.active() != Some(id) {
            self.workspace.activate(id)?;
        }
        let context = self.workspace.get_mut(id).ok_or_else(dtool_core::CommandError::no_document)?;
        match action {
            FontManagerAction::Locate => {
                let found = self.font_manager.locate_selected(&mut context.session)?;
                let session = &mut context.session;
                if let (Some(viewport), Some(bounds)) = (session.view_canvas_size, dtool_core::command::selection_bounds(&session.document, &found)) {
                    dtool_core::view::center_region(&mut session.view, bounds, viewport);
                    context.view_state = session.view.clone();
                }
            }
            FontManagerAction::ConfirmReplace => self.font_manager.confirm_replace(&mut context.session, &mut self.font_replace)?,
            FontManagerAction::Substitute { source, target } => {
                context.session.run(&Command::with_params("font.substitution.set", json!({
                    "source": source,
                    "substitute": target,
                })))?;
                self.font_manager.refresh_usages(&mut context.session)?;
            }
            _ => return Ok(()),
        }
        self.reconcile_active();
        Ok(())
    }

    fn show_font_manager(&mut self, ui: &mut egui::Ui) {
        if !self.font_manager.open {
            return;
        }
        let Some(owner) = self.font_manager_owner.and_then(|id| self.workspace.get_mut(id)) else {
            self.font_replace.hide();
            self.font_manager.hide();
            self.font_manager_owner = None;
            return;
        };
        if let Err(error) = self.font_manager.refresh_usages(&mut owner.session) {
            self.status_bar.set_hint_key(&error.message_key);
        }
        if let Some(action) = crate::font_manager::render_font_manager(ui, &mut self.font_manager, &mut self.font_replace) {
            if let Err(error) = self.font_manager_action(action) {
                self.status_bar.set_hint_key(&error.message_key);
            }
        }
    }

    pub fn widget_focused(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused()).is_some_and(|id| Some(id) != self.canvas_id)
    }

    pub fn current_focus(&self, ctx: &egui::Context) -> Focus {
        if self.dialog_open(ctx) {
            Focus::Dialog
        } else if ctx.any_popup_open() || egui::Popup::is_any_open(ctx) {
            Focus::Menu
        } else if self.widget_focused(ctx) || self.input.is_ime_composing() || crate::canvas::frame_text_input::is_composing() || self.text_editing() {
            Focus::TextField
        } else {
            Focus::Canvas
        }
    }

    pub fn canvas_owns_text(&self, ctx: &egui::Context) -> bool {
        self.text_editing() && !self.dialog_open(ctx) && !ctx.any_popup_open() && !self.widget_focused(ctx)
    }

    pub fn ingest_ime(&mut self, ctx: &egui::Context) -> bool {
        self.input.ingest_events(&[], self.focus);
        if self.input.is_ime_composing() && !self.text_editing() {
            self.input.handle_ime_event(crate::input::ImeEvent::CompositionCancel);
        }
        let focus_lost = ctx.input(|input| input.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false))));
        if !focus_lost {
            return false;
        }
        let committed = self.workspace.session_mut().is_some_and(crate::canvas::frame_text_input::commit_pending);
        if committed {
            self.reconcile_active();
        }
        committed
    }

    pub fn commit_ime_before_switch(&mut self) -> bool {
        if !self.input.is_ime_composing() && !crate::canvas::frame_text_input::is_composing() && !self.text_editing() {
            return false;
        }
        let mut changed = false;
        if self.input.is_ime_composing() {
            changed |= self.input.commit_pending(&mut self.workspace);
        }
        if let Some(session) = self.workspace.session_mut() {
            changed |= self.app_canvas.finish_text_input(session);
        }
        self.reconcile_active();
        changed
    }

    pub fn activate_context(&mut self, id: ContextId) -> Result<(), dtool_core::CommandError> {
        if self.workspace.active() != Some(id) {
            self.commit_ime_before_switch();
        }
        self.workspace.activate(id)
    }

    pub fn finish_ime_on_switch(&mut self) {
        let now = self.workspace.active().zip(self.workspace.session().map(|session| (session.active_tool.clone(), session.text_edit.is_some())));
        let now = now.map(|(id, (tool, editing))| (id, tool, editing));
        if let (Some((old_id, old_tool, old_editing)), Some((id, tool, _))) = (self.ime_owner.clone(), now.clone()) {
            if old_id != id {
                if self.input.is_ime_composing() {
                    self.input.handle_ime_event(crate::input::ImeEvent::CompositionCancel);
                }
                if crate::canvas::frame_text_input::is_composing() {
                    if let Some(context) = self.workspace.get_mut(old_id) {
                        crate::canvas::frame_text_input::finish_text_input(&mut context.session);
                    }
                }
            } else if old_tool != tool && old_editing {
                if let Some(session) = self.workspace.session_mut() {
                    crate::canvas::frame_text_input::commit_pending(session);
                }
                self.input.commit_on_tool_switch(&mut self.workspace);
                self.reconcile_active();
            }
        }
        self.ime_owner = self.workspace.active().zip(self.workspace.session().map(|session| (session.active_tool.clone(), session.text_edit.is_some()))).map(|(id, (tool, editing))| (id, tool, editing));
    }

    pub fn ui_hidden(&self) -> bool {
        self.hidden_ui
    }

    pub fn toggle_ui(&mut self) {
        self.hidden_ui = !self.hidden_ui;
        if self.hidden_ui {
            self.align_popup_open = false;
            self.tool_palette_open = false;
        }
    }

    fn open_tool_palette(&mut self, ctx: &egui::Context) -> Result<(), dtool_core::CommandError> {
        if self.ui_hidden() || (!self.tool_palette_open && self.input_blocked(ctx)) {
            return Err(dtool_core::CommandError::new("CANNOT_EXECUTE"));
        }
        self.tool_palette_open = true;
        Ok(())
    }

    fn toggle_toolbar(&mut self, toolbar: crate::settings::ToolbarArea) {
        let mut settings = self.settings.clone();
        let visible = toolbar.preference(&mut settings.toolbars);
        *visible = !*visible;
        if settings.save(&crate::settings::UiSettings::default_path()).is_err() {
            self.status_bar.set_hint_key("error.io.other");
            return;
        }
        self.settings = settings;
        if matches!(toolbar, crate::settings::ToolbarArea::Tools) && !self.settings.toolbars.tools {
            self.tool_palette_open = false;
        }
        if !self.context_toolbar_visible() {
            self.align_popup_open = false;
        }
    }

    pub fn handle_tab(&mut self, focus: Focus, modifiers: Modifiers) -> TabAction {
        self.handle_tab_command(focus, modifiers)
    }

    pub fn handle_canvas_tab(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input(|i| i.key_pressed(egui::Key::Tab) && i.modifiers.is_none());
        let focused = ctx.memory(|m| m.focused()).filter(|id| Some(*id) != self.canvas_id);
        let stolen = pressed && self.prev_widget_focus.is_none() && focused.is_some_and(|id| !ctx.memory(|m| m.had_focus_last_frame(id)));
        if stolen {
            if let Some(id) = focused {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        }
        if pressed && self.prev_widget_focus.is_none() && self.current_focus(ctx) == Focus::Canvas && !self.text_editing() {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            self.handle_tab(Focus::Canvas, Modifiers::default());
        }
        self.prev_widget_focus = ctx.memory(|m| m.focused()).filter(|id| Some(*id) != self.canvas_id);
    }
}

impl AppShell {
    pub fn navigation_canvas(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.workspace.active() else {
            self.zoom_drag = None;
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(rect, 0.0, dtool_design::theme::surface(ui.ctx(), "pasteboard"));
            ui.allocate_rect(rect, egui::Sense::hover());
            return;
        };
        let picked: Option<&str> = None;
        let Some(context) = self.workspace.get_mut(id) else { return; };
        match (picked, context.session.active_tool.as_str()) {
            (Some(key), _) => context.session.active_tool = key.to_string(),
            (None, "tool.view") => self.navigation_tool = dtool_tools::NavigationTool::View,
            (None, "tool.zoom") => self.navigation_tool = dtool_tools::NavigationTool::Zoom,
            _ => {}
        }
        let area = ui.available_rect_before_wrap();
        let rulers_shown = crate::rulers::ruler_state(&context.session).show;
        let inner = crate::rulers::Rulers::canvas_rect(area, rulers_shown);
        let available = [inner.width(), inner.height()];
        let usable_canvas = available.iter().all(|side| side.is_finite() && *side > 1.0);
        if usable_canvas {
            let actual_zoom = context.session.actual_zoom();
            let view = &mut context.session.view;
            if let Some(before) = context.session.view_canvas_size {
                for (axis, scroll) in view.scroll.iter_mut().enumerate() {
                    *scroll += (available[axis] - before[axis]) * 0.5;
                }
            } else if context.initial_view_pending {
                ui.ctx().request_repaint();
                use dtool_core::view::{ViewTarget, center_region};
                let document = &context.session.document;
                let target = context.reopen_view_target.take().and_then(|target| target.bounds(document))
                    .or_else(|| ViewTarget::first(document));
                if let Some(bounds) = target {
                    view.zoom = actual_zoom;
                    center_region(view, bounds, available);
                }
            }
            context.initial_view_pending = false;
            context.session.view_canvas_size = Some(available);
        }
        let ruler_view = context.session.view.clone();
        if crate::canvas_input::tool_kind(&context.session.active_tool) == crate::canvas_input::ToolKind::Node
            && let (Some(press), _) = crate::input::primary_button_positions(ui)
            && inner.contains(press)
        {
            let local = press - inner.min;
            let point = dtool_core::view_to_doc(&ruler_view, [local.x, local.y]);
            let radius = crate::canvas_input::HANDLE_PX / ruler_view.zoom.max(1e-4);
            if self.app_canvas.input.nodes.claims_alignment_handle(&context.session, point, radius) {
                crate::rulers::set_canvas_gesture(ui.ctx(), true);
            }
        }
        if rulers_shown {
            match self.rulers.show(ui, &mut context.session, &ruler_view, area) {
                crate::rulers::RulerDropResult::Refused(key) => self.status_bar.set_hint_key(&key),
                _ => {}
            }
        } else if let crate::rulers::RulerDropResult::Refused(key) = self.rulers.canvas_guides(ui, &mut context.session, &ruler_view, inner) {
            self.status_bar.set_hint_key(&key);
        }
        let response = ui.allocate_rect(inner, egui::Sense::click_and_drag());
        self.canvas_id = Some(response.id);
        let rect = response.rect;
        let size = [rect.width(), rect.height()];
        if usable_canvas {
            self.canvas_size = size;
        }
        let local = |position: egui::Pos2| [position.x - rect.min.x, position.y - rect.min.y];
        let (space, alt, escape) = ui.input(|input| (input.key_down(egui::Key::Space), input.modifiers.alt, input.key_pressed(egui::Key::Escape)));
        let space = space && !self.app_canvas.input.pen.dragging && self.focus == Focus::Canvas && context.session.text_edit.is_none();
        let mut mode = crate::canvas_input::nav_mode(&context.session.active_tool, space, ui.input(|input| input.pointer.middle_down()));
        let click_mode_out = mode == crate::canvas_input::NavMode::Zoom
            && context.session.tool_options.choice("tool.zoom", "zoom.mode").as_deref() == Some("out");
        let limits = context.session.zoom_limits();
        let view = &mut context.session.view;
        let guide_drag = crate::rulers::guide_drag_active(ui.ctx());
        let mut pan = self.pan_drag.take().filter(|(owner, _, _)| *owner == id && !guide_drag && !escape);
        if !guide_drag && !escape {
            let buttons = ui.input(|state| state.events.iter().filter_map(|event| match event {
                egui::Event::PointerButton { pos, button, pressed, .. } => Some((*button, *pressed, *pos)),
                _ => None,
            }).collect::<Vec<_>>());
            for (button, pressed, pos) in buttons {
                if pressed && pan.is_none() && (button == egui::PointerButton::Middle
                    || (button == egui::PointerButton::Primary && mode == crate::canvas_input::NavMode::Pan))
                    && response.enabled() && response.interact_rect.contains(pos)
                    && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx())
                {
                    pan = Some((id, button, pos));
                } else if !pressed && pan.is_some_and(|(_, captured, _)| captured == button) {
                    let (_, _, previous) = pan.take().unwrap();
                    let delta = pos - previous;
                    dtool_tools::view_tool_pan(view, [delta.x, delta.y]);
                    mode = crate::canvas_input::NavMode::Pan;
                }
            }
            if let Some((_, button, previous)) = pan {
                let (held, latest) = ui.input(|state| (state.pointer.button_down(button), state.pointer.latest_pos()));
                mode = crate::canvas_input::NavMode::Pan;
                if held && let Some(pos) = latest {
                    let delta = pos - previous;
                    dtool_tools::view_tool_pan(view, [delta.x, delta.y]);
                    self.pan_drag = Some((id, button, pos));
                }
            }
        }
        if !guide_drag && mode == crate::canvas_input::NavMode::Pan {
            if response.hovered() { ui.ctx().set_cursor_icon(if self.pan_drag.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab }); }
        }
        let blocked = self.modal_blocking || crate::dialog_chrome::modal_open(ui.ctx())
            || self.move_duplicate.is_some();
        let mut zoom_marquee = None;
        if !guide_drag && !blocked && mode == crate::canvas_input::NavMode::Zoom {
            let button = egui::PointerButton::Primary;
            let before = (view.zoom, view.scroll);
            if self.zoom_drag.as_ref().is_some_and(|drag| drag.owner != id) {
                self.zoom_drag = None;
            }
            if escape {
                if let Some(drag) = self.zoom_drag.take() {
                    view.zoom = drag.zoom;
                    view.scroll = drag.scroll;
                }
            } else {
                // A fast drag can send both button transitions in the same frame.
                let events = ui.input(|input| input.events.iter().filter(|event| matches!(event,
                    egui::Event::PointerMoved(_) | egui::Event::PointerButton { button: egui::PointerButton::Primary, .. }
                )).cloned().collect::<Vec<_>>());
                for event in events {
                    match event {
                        egui::Event::PointerButton { pos, pressed: true, modifiers, .. }
                            if response.enabled() && response.interact_rect.contains(pos)
                            && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
                            && !egui::Popup::is_any_open(ui.ctx()) =>
                        {
                            self.zoom_drag = Some(ZoomDrag {
                                owner: id, start: pos, end: pos, zoom: view.zoom, scroll: view.scroll,
                                marquee: modifiers.alt, moved: false,
                            });
                        }
                        egui::Event::PointerMoved(pos) => {
                            if let Some(drag) = &mut self.zoom_drag {
                                drag.update(pos);
                            }
                        }
                        egui::Event::PointerButton { pos, pressed: false, modifiers, .. } => {
                            if let Some(mut drag) = self.zoom_drag.take() {
                                drag.update(pos);
                                if drag.moved {
                                    drag.apply(view, rect, true, limits);
                                } else if response.clicked_by(button) {
                                    dtool_tools::zoom_tool_click_with_limits(view, local(pos), click_mode_out ^ modifiers.alt, limits);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !ui.input(|input| input.pointer.primary_down()) {
                    self.zoom_drag = None;
                }
                if let Some(drag) = self.zoom_drag.as_ref().filter(|drag| drag.moved) {
                    zoom_marquee = drag.apply(view, rect, false, limits);
                }
            }
            if before != (view.zoom, view.scroll) {
                self.app_canvas.preview_zoom(ui);
            }
            if response.hovered() {
                let cursor = match &self.zoom_drag {
                    Some(drag) if drag.marquee => egui::CursorIcon::Crosshair,
                    Some(drag) if drag.end.x < drag.start.x => egui::CursorIcon::ZoomOut,
                    None if click_mode_out ^ alt => egui::CursorIcon::ZoomOut,
                    _ => egui::CursorIcon::ZoomIn,
                };
                ui.ctx().set_cursor_icon(cursor);
            }
        } else {
            self.zoom_drag = None;
        }
        context.view_state = view.clone();
        let placing = self.place_tool.active_for(id);
        let input = mode == crate::canvas_input::NavMode::Input;
        let table_menu_id = response.id.with(("table-menu", id.0));
        let master_menu_id = response.id.with(("master-menu", id.0));
        if response.secondary_clicked() {
            self.context_target = Self::context_target_for(&context.session);
            let header = response.interact_pointer_pos().and_then(|pointer| {
                crate::table::canvas::menu_target(&context.session, rect, pointer)
            });
            let master_target = if context.session.document.master_pages.assignments.is_empty() {
                None
            } else {
                response.interact_pointer_pos().and_then(|pointer| {
                    let session = &context.session;
                    let point = dtool_core::view_to_doc(&session.view, local(pointer));
                    let tolerance = dtool_core::scene_hit::HIT_TOLERANCE_PX / session.view.zoom.max(1e-4);
                    dtool_core::scene_hit::hit_test_in_view(self.app_canvas.scene(), session, point, tolerance, None)
                        .filter(|object| session.document.master_pages.source_of(*object)
                            .is_some_and(|source| session.document.get_object(source).is_some()))
                })
            };
            ui.data_mut(|data| {
                data.remove::<crate::table::canvas::TableMenu>(table_menu_id);
                data.remove_temp::<dtool_core::ObjectId>(master_menu_id);
                if let Some(object) = master_target { data.insert_temp(master_menu_id, object); }
                if let Some(header) = header { data.insert_temp(table_menu_id, header); }
            });
        }
        let table_menu = ui.data(|data| data.get_temp::<crate::table::canvas::TableMenu>(table_menu_id));
        let master_target = ui.data(|data| data.get_temp::<dtool_core::ObjectId>(master_menu_id));
        let target = self.context_target;
        let persona = self.menu_persona.clone();
        let mut context_error = None;
        let mut context_dialog = None;
        if !crate::canvas_curves::node_context_menu(&context.session) {
            let object_menu = table_menu.is_none() && !target.eq_ignore_ascii_case("TextEditing");
            response.context_menu(|ui| crate::menu_tree::canvas_command_menu(ui, object_menu, |ui| {
                if let Some(id) = master_target {
                    if crate::menu_tree::command_menu_item(ui, text("panel.master.reset"), true).clicked() {
                        let command = Command::with_params("master.object.reset", serde_json::json!({"id": id}));
                        if let Err(error) = context.session.run(&command) {
                            context_error = Some(error.message_key);
                        }
                        ui.close();
                        return;
                    }
                    ui.separator();
                }
                if let Some(menu) = table_menu {
                    if let Some(Err(error)) = crate::table::canvas::show_menu(ui, &mut context.session, menu) {
                        context_error = Some(error.message_key);
                    }
                    return;
                }
                if context.session.active_tool == "tool.vector_crop" {
                    if let [id] = context.session.selection.ids.as_slice() {
                        if let Some(command) = crate::canvas_tools::clear_crop_command(&context.session, *id) {
                            if crate::menu_tree::command_menu_item(ui, text("ctx.vector_crop.clear"), true).clicked() {
                                if let Err(error) = context.session.run(&command) {
                                    context_error = Some(error.message_key);
                                }
                                ui.close();
                                return;
                            }
                            ui.separator();
                        }
                    }
                }
                match crate::context_menu::show(ui, &mut context.session, &persona, target) {
                    Some(Err(error)) => context_error = Some(error.message_key),
                    Some(Ok(result)) => context_dialog = crate::context_menu::dialog_of(&result).map(str::to_string),
                    None => {}
                }
            }));
        }
        if let Some(key) = context_error { self.status_bar.set_hint_key(&key); }
        if context_dialog.is_some() { self.pending_context_dialog = context_dialog; }
        self.app_canvas.input.bind_context(Some(id.0));
        self.app_canvas.blend_preview = (self.blend_options_owner == Some(id))
            .then(|| self.blend_options.preview(&context.session)).flatten();
        self.app_canvas.tone_preview = self.tone_blend.as_ref().and_then(|dialog| dialog.preview(id, &context.session));
        self.app_canvas.layer_opacity_preview = self.panel_registry.layers.opacity_preview(ui.ctx(), &context.session);
        self.app_canvas.stroke_width_preview = self.panel_registry.stroke.width_preview(ui.ctx(), &context.session);
        let symbol_drag = crate::panels::symbols::dragging(ui.ctx());
        let canvas_input = input && !placing && !blocked && !guide_drag && !symbol_drag;
        let compound = self.compound_canvas.update(ui, &response, &context.session, id, canvas_input);
        let original = compound.draft.as_ref().and_then(|draft| {
            let object = context.session.document.find_object_mut(draft.id.0)?;
            self.app_canvas.invalidate();
            Some(std::mem::replace(object, draft.clone()))
        });
        let move_dialog = self.move_duplicate.as_mut().filter(|dialog| dialog.owner == id);
        let preview_drawn = if let Some(dialog) = move_dialog {
            if !self.modal_blocking && !guide_drag && !symbol_drag {
                dialog.update_origin(ui, &response, &context.session);
            }
            dialog.paint_preview(ui, rect, &context.session)
        } else {
            false
        };
        if !preview_drawn {
            self.app_canvas.frame(ui, &response, &mut context.session,
                input && !placing && !blocked && !symbol_drag && !compound.active);
        }
        if let Some(marquee) = zoom_marquee {
            let painter = ui.painter_at(rect);
            let accent = dtool_design::color("accent");
            painter.rect_filled(marquee, 0.0, accent.gamma_multiply(0.15));
            painter.rect_stroke(marquee, 0.0, egui::Stroke::new(1.0, accent), egui::StrokeKind::Inside);
        }
        if let Some(dialog) = self.move_duplicate.as_ref().filter(|dialog| dialog.owner == id) {
            dialog.paint_origin(ui, rect, &context.session);
        }
        if let Some(original) = original {
            if let Some(object) = context.session.document.find_object_mut(original.id.0) { *object = original; }
            self.app_canvas.invalidate();
        }
        if let Some(command) = compound.command {
            if let Err(error) = context.session.run(&command) { self.status_bar.set_hint_key(&error.message_key); }
            ui.ctx().request_repaint();
        }
        if compound.active && !blocked {
            self.compound_canvas.paint(ui, rect, &context.session, id, compound.draft.as_ref());
        }
        dtool_core::commands::pages::remember_view_change(&mut context.session, &ruler_view);
        context.view_state = context.session.view.clone();
        if context.session.text_edit.is_some() && !blocked {
            crate::canvas::frame_text_input::publish_caret_ime(ui.ctx(), &context.session, &context.session.view, rect);
        }
        let guide_view = context.session.view.clone();
        crate::rulers::paint_column_guides(ui, &context.session, &guide_view, rect,
            &self.prefs.column_guide_fill_colour, &self.prefs.column_guide_stroke_colour);
        self.rulers.paint_visible_guides(ui, &context.session, &guide_view, rect);
        crate::rulers::paint_margins(ui, &context.session, &guide_view, rect, &self.prefs.margins_colour);
        crate::rulers::paint_baseline_grid(ui, &context.session, &guide_view, rect, &self.prefs.baseline_grid_colour);
        crate::table::canvas::paint_baseline_guides(ui, rect, &context.session, &guide_view);
        if self.guides_owner == Some(id) {
            if let Some(guide) = self.guides_dialog.preview_guide(&context.session) {
                crate::rulers::Rulers::paint_guide(ui, &context.session.document, &guide_view, rect, &guide, dtool_design::color("smart_guide"));
            }
        }
        if symbol_drag && !guide_drag && !placing && !blocked
            && let Some(error) = crate::panels::symbols::canvas_drop(ui, &response, id, &mut context.session)
        {
            self.status_bar.set_hint_key(&error.message_key);
        }
        if !guide_drag { self.place_tool.canvas(ui, &response, id, &mut context.session, input); }
        if canvas_input {
            crate::canvas_input::paint_canvas_cursor(ui, &response, Some(&context.session.active_tool), &mut self.icon_cache);
        } else if !blocked {
            crate::canvas_input::paint_canvas_cursor(ui, &response, None, &mut self.icon_cache);
        }
        if let Some(key) = &self.place_tool.error_key { self.status_bar.set_hint_key(key); }
    }
}

#[derive(Clone, Copy)]
enum StudioDrop {
    Tab(egui_tiles::TileId),
    Split(egui_tiles::TileId, bool),
    Edge,
}

const STUDIO_EDGE_DEPTH: f32 = 28.0;
const STUDIO_SEAM_DEPTH: f32 = 10.0;

fn studio_edge_rect(canvas: egui::Rect, side: DockSide) -> egui::Rect {
    let mut edge = canvas;
    match side {
        DockSide::Left => edge.max.x = (edge.min.x + STUDIO_EDGE_DEPTH).min(edge.max.x),
        DockSide::Right => edge.min.x = (edge.max.x - STUDIO_EDGE_DEPTH).max(edge.min.x),
        DockSide::Top => edge.max.y = (edge.min.y + STUDIO_EDGE_DEPTH).min(edge.max.y),
        DockSide::Bottom => edge.min.y = (edge.max.y - STUDIO_EDGE_DEPTH).max(edge.min.y),
    }
    edge
}

fn studio_group_split_rects(side: DockSide, rect: egui::Rect) -> (egui::Rect, egui::Rect) {
    let header = dtool_design::size("panel.header.height");
    match side {
        DockSide::Left | DockSide::Right => {
            let depth = ((rect.height() - header) * 0.25).clamp(0.0, STUDIO_EDGE_DEPTH);
            let mut before = rect;
            before.min.y += header;
            before.max.y = before.min.y + depth;
            let mut after = rect;
            after.min.y = (after.max.y - depth).max(rect.top() + header);
            (before, after)
        }
        DockSide::Top | DockSide::Bottom => {
            let depth = (rect.width() * 0.25).min(STUDIO_EDGE_DEPTH);
            let mut before = rect;
            before.min.y += header;
            before.max.x = before.min.x + depth;
            let mut after = rect;
            after.min.y += header;
            after.min.x = after.max.x - depth;
            (before, after)
        }
    }
}

fn studio_drop_target(
    groups: &[(DockSide, egui_tiles::TileId, egui::Rect)],
    canvas: egui::Rect,
    point: egui::Pos2,
) -> Option<(DockSide, StudioDrop, egui::Rect)> {
    if let Some(target) = studio_group_seam(groups, point) {
        return Some(target);
    }
    if let Some(&(side, group, rect)) = groups.iter().find(|(_, _, rect)| rect.contains(point)) {
        let (before, after) = studio_group_split_rects(side, rect);
        let (target, preview) = if before.is_positive() && before.contains(point) {
            (StudioDrop::Split(group, true), before)
        } else if after.is_positive() && after.contains(point) {
            (StudioDrop::Split(group, false), after)
        } else {
            (StudioDrop::Tab(group), rect)
        };
        return Some((side, target, preview));
    }
    if !canvas.is_positive() || !canvas.contains(point) {
        return None;
    }
    let (side, distance) = [
        (DockSide::Left, point.x - canvas.left()), (DockSide::Right, canvas.right() - point.x),
        (DockSide::Top, point.y - canvas.top()), (DockSide::Bottom, canvas.bottom() - point.y),
    ].into_iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    (distance <= STUDIO_EDGE_DEPTH).then_some((side, StudioDrop::Edge, studio_edge_rect(canvas, side)))
}

fn studio_group_seam(
    groups: &[(DockSide, egui_tiles::TileId, egui::Rect)],
    pointer: egui::Pos2,
) -> Option<(DockSide, StudioDrop, egui::Rect)> {
    for &(side, group, rect) in groups {
        let vertical = matches!(side, DockSide::Left | DockSide::Right);
        let end = if vertical { rect.bottom() } else { rect.right() };
        let axis = if vertical { pointer.y } else { pointer.x };
        if axis < end - STUDIO_SEAM_DEPTH || axis > end + STUDIO_SEAM_DEPTH * 2.0 {
            continue;
        }
        for &(_, _, next) in groups.iter().filter(|(next_side, _, _)| *next_side == side) {
            let start = if vertical { next.top() } else { next.left() };
            if !(0.0..=STUDIO_SEAM_DEPTH).contains(&(start - end)) {
                continue;
            }
            let rect = if vertical {
                egui::Rect::from_min_max(
                    egui::pos2(rect.left().max(next.left()), end - STUDIO_SEAM_DEPTH),
                    egui::pos2(rect.right().min(next.right()), start + STUDIO_SEAM_DEPTH),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(end - STUDIO_SEAM_DEPTH, rect.top().max(next.top())),
                    egui::pos2(start + STUDIO_SEAM_DEPTH, rect.bottom().min(next.bottom())),
                )
            };
            if rect.is_positive() && rect.contains(pointer) {
                return Some((side, StudioDrop::Split(group, false), rect));
            }
        }
    }
    None
}

fn paint_studio_drop_guides(
    ctx: &egui::Context,
    groups: &[(DockSide, egui_tiles::TileId, egui::Rect)],
    canvas: egui::Rect,
    pointer: egui::Pos2,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("studio_drop_guides")));
    let colour = dtool_design::color("smart_guide");
    let surface = dtool_design::theme::surface(ctx, "panel.bg");
    let foreground = ctx.style_of(ctx.theme()).visuals.text_color();
    let caption = |rect: egui::Rect, key: &str| {
        let galley = painter.layout_no_wrap(dtool_strings::text(key).to_owned(), egui::FontId::proportional(11.0), foreground);
        let frame_size = galley.size() + egui::vec2(4.0, 4.0);
        if frame_size.x <= rect.width() && frame_size.y <= rect.height() {
            let frame = egui::Rect::from_center_size(rect.center(), frame_size);
            painter.rect_filled(frame, 3.0, surface);
            painter.galley(frame.center() - galley.size() * 0.5, galley, foreground);
        }
    };
    let paint = |rect: egui::Rect, opacity, outline| {
        if rect.width() > 2.0 && rect.height() > 2.0 {
            let rect = rect.shrink(1.0);
            painter.rect_filled(rect, 2.0, colour.gamma_multiply(opacity));
            painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, colour.gamma_multiply(outline)), egui::StrokeKind::Inside);
        }
    };
    let seam = studio_group_seam(groups, pointer);
    let group = groups.iter().find(|(_, _, rect)| rect.contains(pointer));
    if let Some((side, _, seam)) = seam {
        paint(seam, 0.28, 1.0);
        let label_area = if matches!(side, DockSide::Top | DockSide::Bottom) {
            egui::Rect::from_center_size(seam.center(), egui::vec2(120.0, seam.height()))
        } else {
            seam
        };
        caption(label_area, "studio.drop.between");
    } else if let Some(&(side, _, rect)) = group {
        let (before, after) = studio_group_split_rects(side, rect);
        let (before_key, after_key) = match side {
            DockSide::Left | DockSide::Right => ("studio.drop.above", "studio.drop.below"),
            DockSide::Top | DockSide::Bottom => ("studio.drop.before", "studio.drop.after"),
        };
        let mut tab_area = rect;
        let header = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), before.top().min(rect.bottom())));
        match side {
            DockSide::Left | DockSide::Right => {
                tab_area.min.y = before.bottom();
                tab_area.max.y = after.top();
            }
            DockSide::Top | DockSide::Bottom => {
                tab_area.min.x = before.right();
                tab_area.max.x = after.left();
            }
        }
        let target = studio_drop_target(groups, canvas, pointer).map(|(_, target, _)| target);
        if header.contains(pointer) {
            paint(header, 0.28, 1.0);
        }
        for (area, label, active) in [
            (before, before_key, matches!(target, Some(StudioDrop::Split(_, true)))),
            (tab_area, "studio.drop.tab", matches!(target, Some(StudioDrop::Tab(_)))),
            (after, after_key, matches!(target, Some(StudioDrop::Split(_, false)))),
        ] {
            paint(area, if active { 0.28 } else { 0.10 }, if active { 1.0 } else { 0.48 });
            caption(area, label);
        }
    }
    let edge = if seam.is_none() && group.is_none() {
        studio_drop_target(groups, canvas, pointer).map(|(side, _, _)| side)
    } else {
        None
    };
    for (side, label) in [
        (DockSide::Left, "studio.drop.left"),
        (DockSide::Right, "studio.drop.right"),
        (DockSide::Top, "studio.drop.top"),
        (DockSide::Bottom, "studio.drop.bottom"),
    ] {
        let rect = studio_edge_rect(canvas, side);
        let active = edge == Some(side);
        paint(rect, if active { 0.28 } else { 0.10 }, if active { 1.0 } else { 0.48 });
        caption(rect, label);
    }
}

impl AppShell {
    pub fn show_dock(&mut self, ui: &mut egui::Ui) {
        crate::panels::symbols::clear_native_drag_frame(ui.ctx());
        if self.ui_hidden() {
            ui.data_mut(|data| data.remove::<crate::panels::DockTabDrag>(crate::panels::DockTabDrag::memory_key()));
            if self.persist_layout().is_err() { self.status_bar.set_hint_key("error.io.other"); }
            return;
        }
        self.panel_registry.canvas_size = self.canvas_size;

        let mut drop_groups = Vec::new();
        let mut native_drop = None;
        for side in DockSide::ALL {
            if !self.layout.has_visible_docked_panels(side) { continue; }
            let panel = match side {
                DockSide::Left => egui::Panel::left("dtool.shell.dock.left"),
                DockSide::Right => egui::Panel::right("dtool.shell.dock.right"),
                DockSide::Top => egui::Panel::top("dtool.shell.dock.top"),
                DockSide::Bottom => egui::Panel::bottom("dtool.shell.dock.bottom"),
            };
            let horizontal = matches!(side, DockSide::Left | DockSide::Right);
            let extent = |rect: egui::Rect| if horizontal { rect.width() } else { rect.height() };
            let available = extent(ui.available_rect_before_wrap());
            let maximum = (available - 80.0).max(1.0);
            let default_size = match side {
                DockSide::Left => dtool_design::size("panel.default_width").min(ui.max_rect().width() * 0.20),
                DockSide::Right => (dtool_design::size("panel.default_width") + dtool_design::size("panel.secondary_width"))
                    .min(ui.max_rect().width() * 0.46),
                _ => (ui.max_rect().height() * 0.22).clamp(96.0, 180.0),
            };
            let saved = self.layout.dock_sizes.get(side.key()).copied().filter(|size| size.is_finite() && *size > 0.0);
            let previous = self.presented_dock_sizes.get(side.key()).copied();
            let restore = previous.is_some_and(|(size, max)| size != saved || (max < saved.unwrap_or(0.0) && max != maximum))
                || (previous.is_none() && saved.is_some());
            let panel = if restore {
                panel.exact_size(saved.unwrap_or(default_size).min(maximum))
            } else {
                panel.default_size(default_size).max_size(maximum)
            };
            panel.resizable(true).frame(egui::Frame::new().fill(dtool_design::theme::surface(ui.ctx(), "panel.bg"))).show(ui, |ui| {
                let (groups, dropped) = crate::panels::show_side_with_slices(ui, &mut self.layout, side, &mut self.panel_registry, crate::panels::SessionPanels { slices: &mut self.slices_panel, history: &mut self.history_panel, snapshots: &mut self.snapshots_panel }, &mut self.workspace);
                drop_groups.extend(groups.into_iter().map(|(group, rect)| (side, group, rect)));
                if dropped { native_drop = Some(side); }
            });
            let size = available - extent(ui.available_rect_before_wrap());
            // A temporarily narrow window can clip the dock without changing its saved width.
            let preserve = saved.is_some_and(|saved| saved > maximum && size >= maximum - 1.0);
            if size.is_finite() && size > 0.0 && !preserve {
                self.layout.dock_sizes.insert(side.key().into(), size);
            }
            let saved = self.layout.dock_sizes.get(side.key()).copied();
            self.presented_dock_sizes.insert(side.key(), (saved, maximum));
        }
        let mut canvas = ui.available_rect_before_wrap();
        if self.settings.status_bar_visible {
            canvas.max.y = (canvas.max.y - dtool_design::size("bar.status.height")).max(canvas.min.y);
        }
        self.finish_dock_tab_drag(ui.ctx(), &drop_groups, canvas, native_drop);
        let floating_drag = crate::panels::show_floating_with_slices(ui.ctx(), &mut self.layout, &mut self.panel_registry, crate::panels::SessionPanels { slices: &mut self.slices_panel, history: &mut self.history_panel, snapshots: &mut self.snapshots_panel }, &mut self.workspace);
        if std::mem::take(&mut self.panel_registry.navigator.navigation_changed) {
            self.app_canvas.preview_zoom(ui);
        }
        if let Some((panel, pointer, released)) = floating_drag {
            if !released {
                paint_studio_drop_guides(ui.ctx(), &drop_groups, canvas, pointer);
            } else if let Some((side, target, _)) = studio_drop_target(&drop_groups, canvas, pointer) {
                let was_os_window = self.layout.panel_state(&panel).is_some_and(|panel| panel.os_window);
                self.apply_studio_drop(&panel, side, target);
                if was_os_window {
                    ui.ctx().send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                }
            }
        }
        if self.persist_layout().is_err() { self.status_bar.set_hint_key("error.io.other"); }
    }

    fn apply_studio_drop(&mut self, panel: &crate::dock::PanelId, side: DockSide, target: StudioDrop) {
        match target {
            StudioDrop::Tab(group) => self.layout.redock_into_tabs(panel, side, group),
            StudioDrop::Split(group, before) => self.layout.redock_as_group(panel, side, Some((group, before))),
            StudioDrop::Edge => self.layout.redock_as_group(panel, side, None),
        }
    }

    fn finish_dock_tab_drag(&mut self, ctx: &egui::Context, groups: &[(DockSide, egui_tiles::TileId, egui::Rect)], canvas: egui::Rect, native_drop: Option<DockSide>) {
        let key = crate::panels::DockTabDrag::memory_key();
        let Some(drag) = ctx.data(|data| data.get_temp::<crate::panels::DockTabDrag>(key)) else { return };
        let (released, down, pointer, cancelled) = ctx.input(|input| {
            (input.pointer.primary_released(), input.pointer.primary_down(), input.pointer.interact_pos(),
                !input.focused || input.key_pressed(egui::Key::Escape))
        });
        let cancelled = cancelled || self.input_blocked(ctx);
        if released || !down || cancelled {
            ctx.data_mut(|data| data.remove::<crate::panels::DockTabDrag>(key));
        }
        if cancelled || (!down && !released) {
            return;
        }
        let Some(pointer) = pointer.filter(|pos| pos.distance(drag.origin) > 6.0) else { return };
        if !released {
            paint_studio_drop_guides(ctx, groups, canvas, pointer);
        } else if let Some((side, target, _)) = studio_drop_target(groups, canvas, pointer) {
            let native_handled = side == drag.source && native_drop == Some(side) && matches!(target, StudioDrop::Tab(_));
            if !native_handled {
                self.apply_studio_drop(&drag.panel, side, target);
            }
        } else if ctx.content_rect().contains(pointer) {
            self.layout.float_panel_at(&drag.panel, pointer);
        }
    }

    pub fn persist_layout(&mut self) -> std::io::Result<()> {
        let snapshot = serde_json::to_string(&self.layout).map_err(std::io::Error::other)?;
        if snapshot != self.layout_persisted {
            crate::persona::save_live_dock(&self.layout).map_err(|error| std::io::Error::other(error.message_key))?;
            self.layout.save_file(&Layout::last_layout_path())?;
            self.layout_persisted = snapshot;
        }
        if !Layout::workspace_default_path().exists() {
            self.workspace_default_layout.save_file(&Layout::workspace_default_path())?;
        }
        Ok(())
    }

    pub fn set_workspace_default_layout(&mut self, layout: Layout) -> std::io::Result<()> {
        layout.save_file(&Layout::workspace_default_path())?;
        self.workspace_default_layout = layout;
        Ok(())
    }

    pub fn reset_layout(&mut self) -> std::io::Result<()> {
        self.layout.set(self.workspace_default_layout.clone());
        crate::panels::register_named_panels(&mut self.layout, &self.panel_registry);
        self.persist_layout()
    }
}

impl AppShell {
    pub fn set_menu_persona(&mut self,persona:&str){self.menu_persona=crate::menu_tree::normalize_persona(persona).to_string();}
    pub fn show_origin_controls(&mut self,ui:&mut egui::Ui){if let Some(session)=self.workspace.session_mut(){crate::rulers::origin_controls(ui,session);}}
    pub fn open_template_dialog(&mut self){self.open_file_dialog(crate::dialogs::FileDialogMode::TemplateOpen);}
}

fn caret_style(session: &dtool_core::Session, kind: dtool_core::text_styles::StyleKind) -> Option<String> {
    let key = match kind { dtool_core::text_styles::StyleKind::Paragraph => dtool_core::text_styles::PARA_STYLE_KEY, dtool_core::text_styles::StyleKind::Character => dtool_core::text_styles::CHAR_STYLE_KEY };
    let target = crate::context::text_bar::target_of(session)?;
    let story = session.document.stories.iter().find(|story| story.id.0 == target.story)?;
    story.ranges.iter().find_map(|range| {
        let start = range.get("start").and_then(Value::as_u64)? as usize;
        let end = range.get("end").and_then(Value::as_u64)? as usize;
        (start <= target.start && target.start < end.max(start + 1)).then(|| range.get(key).and_then(Value::as_str).map(str::to_string)).flatten()
    })
}
