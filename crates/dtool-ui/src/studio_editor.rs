use crate::dialog_chrome::{confirm_cancel_buttons, options_divider, options_scroll};
use crate::persona::{PersonaId,PersonaEntry,CustomStudio,persona_catalog};
use crate::tools_panel::ToolId;
use crate::dock::PanelId;
use crate::icon_cache::IconCache;
use dtool_core::{Command,CommandError,CommandResult};

pub type Runner<'a> = dyn FnMut(&Command) -> CommandResult + 'a;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum StudioSection {
    #[default]
    Toolbar,
    Tools,
    Panels,
}

#[derive(Debug,Clone)]
pub struct StudioEditorState {
    pub original: Option<String>,
    pub name: String,
    pub base: PersonaId,
    pub tools: Vec<ToolId>,
    pub toolbar: Vec<String>,
    pub panels: Vec<PanelId>,
    section: StudioSection,
}

impl StudioEditorState {
    pub fn new(base: PersonaId) -> Self {
        let def = persona_catalog().into_iter().find(|d| d.entry == PersonaEntry::Builtin(base));
        let (tools, toolbar, panels) = def.map(|d| (d.tools, d.toolbar, d.panels)).unwrap_or_default();
        Self { original: None, name: String::new(), base, tools, toolbar, panels, section: StudioSection::default() }
    }

    pub fn edit(studio: &CustomStudio) -> Self {
        Self { original: Some(studio.name.clone()), name: studio.name.clone(), base: studio.base, tools: studio.tools.clone(), toolbar: studio.toolbar.clone(), panels: studio.panels.clone(), section: StudioSection::default() }
    }

    pub fn toggle_tool(&mut self, tool: &ToolId) {
        if let Some(pos) = self.tools.iter().position(|t| t.0 == tool.0) {
            self.tools.remove(pos);
        } else {
            self.tools.push(tool.clone());
        }
    }

    pub fn toggle_panel(&mut self, panel: &PanelId) {
        if let Some(pos) = self.panels.iter().position(|p| p == panel) {
            self.panels.remove(pos);
        } else {
            self.panels.push(panel.clone());
        }
    }

    pub fn toggle_toolbar(&mut self, item: &str) {
        if let Some(pos) = self.toolbar.iter().position(|t| t == item) {
            self.toolbar.remove(pos);
        } else {
            self.toolbar.push(item.into());
        }
    }

    pub fn to_studio(&self) -> CustomStudio {
        CustomStudio { name: self.name.trim().to_string(), base: self.base, tools: self.tools.clone(), toolbar: self.toolbar.clone(), panels: self.panels.clone() }
    }

    pub fn commands(&self) -> Vec<Command> {
        let name = self.name.trim();
        let layout = serde_json::json!({
            "tools": self.tools.iter().map(|tool| tool.0).collect::<Vec<_>>(),
            "toolbar": self.toolbar,
            "panels": self.panels.iter().map(|panel| panel.key().to_string()).collect::<Vec<_>>(),
        });
        let mut commands = Vec::new();
        match &self.original {
            None => {
                let mut params = layout;
                params["name"] = name.into();
                params["base"] = PersonaEntry::Builtin(self.base).key().into();
                commands.push(Command::with_params("ui.studio.create", params));
            }
            Some(original) => {
                if original != name {
                    commands.push(Command::with_params("ui.studio.rename", serde_json::json!({ "name": original, "new_name": name })));
                }
                let mut params = layout;
                params["name"] = name.into();
                commands.push(Command::with_params("ui.studio.update", params));
            }
        }
        commands
    }
}

#[derive(Debug,Clone,Default)]
pub struct StudioManagerState {
    pub studios: Vec<CustomStudio>,
    pub editing: Option<StudioEditorState>,
    pub renaming: Option<(String,String)>,
    pub skipped: Vec<String>,
    pub error: Option<String>,
    pub pending_switch: Option<Command>,
}

impl StudioManagerState {
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&mut self, result: CommandResult) -> Option<serde_json::Value> {
        match result {
            Ok(value) => {
                if let Some(items) = value.get("skipped").and_then(|v| v.as_array()) {
                    self.skipped.extend(items.iter().filter_map(|item| item.as_str()).map(String::from));
                }
                Some(value)
            }
            Err(error) => {
                self.error = Some(error.code.to_string());
                None
            }
        }
    }

    pub fn refresh(&mut self, run: &mut Runner) -> Result<(), CommandError> {
        let value = run(&Command::new("ui.studio.list"))?;
        self.studios = serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?;
        Ok(())
    }

    pub fn open_editor(&mut self, base: PersonaId) {
        self.editing = Some(StudioEditorState::new(base));
    }

    pub fn open_existing(&mut self, name: &str) -> Result<(), CommandError> {
        let studio = self.studios.iter().find(|studio| studio.name == name).ok_or_else(CommandError::not_found)?;
        self.editing = Some(StudioEditorState::edit(studio));
        Ok(())
    }

    pub fn close_editor(&mut self, run: &mut Runner) -> Result<Option<CustomStudio>, CommandError> {
        let Some(editor) = self.editing.take() else { return Ok(None) };
        if editor.name.trim().is_empty() {
            return Ok(None);
        }
        self.skipped.clear();
        self.error = None;
        for command in editor.commands() {
            let result = run(&command);
            if let Err(error) = &result {
                let error = error.clone();
                self.record(result);
                self.editing = Some(editor);
                return Err(error);
            }
            self.record(result);
        }
        self.refresh(run)?;
        Ok(self.studios.iter().find(|studio| studio.name == editor.name.trim()).cloned())
    }

    pub fn cancel_editor(&mut self) {
        self.editing = None;
    }

    pub fn rename(&mut self, run: &mut Runner, old: &str, new: &str) -> Result<(), CommandError> {
        run(&Command::with_params("ui.studio.rename", serde_json::json!({ "name": old, "new_name": new })))?;
        self.renaming = None;
        self.refresh(run)
    }

    pub fn delete(&mut self, run: &mut Runner, name: &str) -> Result<(), CommandError> {
        run(&Command::with_params("ui.studio.delete", serde_json::json!({ "name": name })))?;
        self.refresh(run)
    }

    pub fn activate(&mut self, run: &mut Runner, name: &str) -> Result<(), CommandError> {
        run(&switch_command(name))?;
        Ok(())
    }

    pub fn request_switch(&mut self, name: &str) {
        self.pending_switch = Some(switch_command(name));
    }

    pub fn list(&self) -> Vec<CustomStudio> {
        self.studios.clone()
    }
}

pub fn switch_command(name: &str) -> Command {
    Command::with_params("ui.persona.set", serde_json::json!({ "id": PersonaEntry::Custom(name.into()).key() }))
}

fn text(key: &str) -> &str {
    dtool_strings::text(key)
}

fn panel_label(panel: &PanelId) -> String {
    let key = format!("panel.{}", panel.key());
    dtool_strings::text(&key).to_string()
}

pub fn selectable_panels() -> Vec<PanelId> {
    crate::panel_ids::studio_panel_ids()
}

fn toolbar_editor(ui: &mut egui::Ui, items: &mut Vec<String>, mut icons: Option<&mut IconCache>) {
    use crate::persona::TOOLBAR_ACTIONS;

    ui.label(text("studio.toolbar.current"));
    let (mut moved, mut removed) = (None, None);
    let mut added = None;
    let mut drag_rects = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let spec = TOOLBAR_ACTIONS.iter().find(|(id, _, _)| *id == item);
        let label = spec.map_or(item.as_str(), |(_, _, key)| text(key));
        ui.horizontal(|ui| {
            let row = ui.horizontal(|ui| {
                if let Some((_, icon, _)) = spec
                    && let Some(texture) = icons.as_deref_mut().and_then(|cache| cache.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui))) {
                    ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(16.0)));
                }
                ui.set_min_width(130.0);
                ui.label(label);
            });
            drag_rects.push(row.response.rect.intersect(ui.clip_rect()));
            if row.response.dnd_hover_payload::<ToolbarCatalogItem>().is_some() {
                let rect = row.response.rect;
                let after = ui.input(|input| input.pointer.hover_pos().is_some_and(|pos| pos.y >= rect.center().y));
                let line_y = if after { rect.bottom() } else { rect.top() };
                ui.painter().hline(rect.x_range(), line_y, egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
                if let Some(command) = row.response.dnd_release_payload::<ToolbarCatalogItem>() {
                    added = Some((index + usize::from(after), command.id));
                }
            }
            row.response.on_hover_cursor(egui::CursorIcon::Grab);
            if ui.add_enabled(index > 0, egui::Button::new("↑")).on_hover_text(text("studio.toolbar.earlier")).clicked() {
                moved = Some((index, index - 1));
            }
            if ui.add_enabled(index + 1 < items.len(), egui::Button::new("↓")).on_hover_text(text("studio.toolbar.later")).clicked() {
                moved = Some((index, index + 1));
            }
            if ui.small_button("×").on_hover_text(text("pref.remove")).clicked() {
                removed = Some(index);
            }
        });
    }
    let drag_key = ui.scope_id().with("toolbar.drag_start");
    let mut drag_start = ui.data(|data| data.get_temp::<usize>(drag_key));
    let (pointer, down, button_events) = ui.input(|input| {
        let events = input.events.iter().filter_map(|event| match event {
            egui::Event::PointerButton { pos, pressed, button: egui::PointerButton::Primary, .. } => Some((*pos, *pressed)),
            _ => None,
        }).collect::<Vec<_>>();
        (input.pointer.hover_pos(), input.pointer.primary_down(), events)
    });
    let row_at = |pos| {
        if ui.ctx().layer_id_at(pos) != Some(ui.layer_id()) {
            return None;
        }
        drag_rects.iter().position(|rect| rect.contains(pos))
    };
    for (pos, pressed) in button_events {
        if pressed {
            drag_start = row_at(pos);
        } else if let Some(from) = drag_start.take()
            && let Some(to) = row_at(pos) {
            moved = Some((from, to));
        }
    }
    if let (Some(from), Some(to)) = (drag_start.filter(|_| down), pointer.and_then(row_at))
        && from != to {
        let row = drag_rects[to];
        let y = if from < to { row.bottom() } else { row.top() };
        ui.painter().hline(row.x_range(), y, egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
    }
    ui.data_mut(|data| {
        data.remove::<usize>(drag_key);
        if let Some(from) = drag_start.filter(|_| down) {
            data.insert_temp(drag_key, from);
        }
    });
    if let Some(index) = removed {
        items.remove(index);
    } else if let Some((from, to)) = moved.filter(|(from, to)| from != to && *from < items.len()) {
        let item = items.remove(from);
        items.insert(to, item);
    }

    let (drop_rect, drop) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
    let hovered = drop.dnd_hover_payload::<ToolbarCatalogItem>().is_some();
    let stroke = if hovered { ui.visuals().selection.stroke } else { ui.visuals().widgets.noninteractive.bg_stroke };
    ui.painter().rect_stroke(drop_rect, 3.0, stroke, egui::StrokeKind::Inside);
    ui.painter().text(drop_rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(14.0), ui.visuals().text_color());
    if let Some(command) = drop.dnd_release_payload::<ToolbarCatalogItem>() {
        added = Some((items.len(), command.id));
    }
    if let Some((index, action)) = added.filter(|(_, action)| !items.iter().any(|item| item == action)) {
        items.insert(index.min(items.len()), action.to_owned());
    }

    ui.separator();
    ui.label(text("studio.toolbar.available"));
    let search_id = ui.scope_id().with("toolbar.catalog.search");
    let mut search = ui.data_mut(|data| data.get_temp_mut_or_default::<String>(search_id).clone());
    ui.add(egui::TextEdit::singleline(&mut search).hint_text(text("pref.search")));
    let filter = search.trim().to_lowercase();
    ui.data_mut(|data| data.insert_temp(search_id, search));
    for &(id, icon, key) in TOOLBAR_ACTIONS {
        let label = text(key);
        if !label.to_lowercase().contains(&filter) && !id.contains(&filter) { continue; }
        let available = !items.iter().any(|item| item == id);
        let texture = icons.as_deref_mut().and_then(|cache| cache.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)));
        let button = match &texture {
            Some(texture) => egui::Button::image_and_text(
                egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(17.0)), label),
            None => egui::Button::new(label),
        };
        let response = ui.add_enabled(available, button.sense(egui::Sense::click_and_drag()));
        let press_origin = ui.input(|input| {
            input.pointer.primary_pressed().then(|| input.pointer.press_origin()).flatten()
        });
        if available && press_origin.is_some_and(|pos| {
            response.interact_rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
        }) {
            egui::DragAndDrop::set_payload(ui.ctx(), ToolbarCatalogItem { id, placed: false });
        }
        if response.clicked() {
            items.push(id.to_owned());
        }
    }
}

pub(crate) struct ToolbarCatalogItem {
    pub id: &'static str,
    pub placed: bool,
}

pub fn show_editor(ui: &mut egui::Ui, state: &mut StudioManagerState, run: &mut Runner) {
    show_editor_with_icons(ui, state, run, None);
}

fn show_editor_with_icons(ui: &mut egui::Ui, state: &mut StudioManagerState, run: &mut Runner, mut icons: Option<&mut IconCache>) {
    let Some(editor) = state.editing.as_mut() else { return };
    ui.horizontal(|ui| {
        ui.label(text("studio.editor.name"));
        ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(ui.available_width()));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(text("studio.editor.base"));
        if editor.original.is_some() {
            let key = format!("persona.{}", PersonaEntry::Builtin(editor.base).key());
            ui.label(text(&key));
        } else {
            let mut base = editor.base;
            for option in crate::persona::STUDIO_BASES {
                let key = format!("persona.{}", PersonaEntry::Builtin(option).key());
                dtool_design::choice_value(ui, &mut base, option, text(&key));
            }
            if base != editor.base {
                let defaults = StudioEditorState::new(base);
                editor.base = base;
                editor.tools = defaults.tools;
                editor.panels = defaults.panels;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (section, key) in [
            (StudioSection::Toolbar, "menu.view.top_toolbar"),
            (StudioSection::Tools, "studio.editor.tools"),
            (StudioSection::Panels, "studio.editor.panels"),
        ] {
            dtool_design::choice_value(ui, &mut editor.section, section, text(key));
        }
    });
    options_divider(ui);
    let body_height = (ui.ctx().content_rect().bottom() - ui.cursor().min.y - 52.0)
        .clamp(16.0, dtool_design::size("dialog.max_body_height"));
    let scroll_id = match editor.section {
        StudioSection::Toolbar => "studio.editor.toolbar",
        StudioSection::Tools => "studio.editor.tools",
        StudioSection::Panels => "studio.editor.panels",
    };
    options_scroll(ui, scroll_id, body_height, |ui| {
        if !state.skipped.is_empty() {
            ui.label(dtool_strings::format("studio.skipped", &[("items", &state.skipped.join(", "))]));
        }
        if let Some(error) = &state.error {
            ui.colored_label(ui.visuals().error_fg_color, text(error));
        }
        match editor.section {
            StudioSection::Toolbar => toolbar_editor(ui, &mut editor.toolbar, icons.as_deref_mut()),
            StudioSection::Tools => {
                let allowed = persona_catalog().into_iter()
                    .find(|d| d.entry == PersonaEntry::Builtin(editor.base))
                    .map(|d| d.tools).unwrap_or_default();
                for tool in &allowed {
                    let mut on = editor.tools.iter().any(|t| t.0 == tool.0);
                    let changed = ui.horizontal(|ui| {
                        if let Some(texture) = icons.as_deref_mut().and_then(|cache| {
                            cache.tool_texture(ui.ctx(), tool.0, dtool_design::icons::current_icon_colour(), dtool_design::icon_texture_px(ui))
                        }) {
                            ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(16.0)));
                        }
                        dtool_design::toggle(ui, &mut on, text(tool.0)).changed()
                    }).inner;
                    if changed {
                        editor.toggle_tool(tool);
                    }
                }
            }
            StudioSection::Panels => {
                for panel in selectable_panels() {
                    let mut on = editor.panels.contains(&panel);
                    if dtool_design::toggle(ui, &mut on, panel_label(&panel)).changed() {
                        editor.toggle_panel(&panel);
                    }
                }
            }
        }
    });
    options_divider(ui);
    let (save, cancel) = confirm_cancel_buttons(ui, text("studio.editor.save"), !editor.name.trim().is_empty());
    if save {
        let _ = state.close_editor(run);
    } else if cancel {
        state.cancel_editor();
    }
}

pub fn show_manager(ui: &mut egui::Ui, state: &mut StudioManagerState, run: &mut Runner) {
    show_manager_with_icons(ui, state, run, None);
}

fn transfer_studio(run: &mut Runner, name: Option<&str>) -> Result<bool, CommandError> {
    let (title, command) = if name.is_some() {
        ("studio.manager.export", "ui.studio.export")
    } else {
        ("studio.manager.import", "ui.studio.import")
    };
    let path = crate::dialogs::pick_file_path(
        text(title),
        text("studio.manager.file"),
        "dtoolstudio",
        name.map(|_| "workspace.dtoolstudio"),
    ).map_err(|_| CommandError::new("IO_ERROR"))?;
    let Some(path) = path else { return Ok(false) };
    let path = path.to_str().ok_or_else(CommandError::invalid_params)?;
    let params = if let Some(name) = name {
        serde_json::json!({ "name": name, "path": path })
    } else {
        serde_json::json!({ "path": path, "replace_existing": false })
    };
    run(&Command::with_params(command, params))?;
    Ok(true)
}

fn show_manager_with_icons(ui: &mut egui::Ui, state: &mut StudioManagerState, run: &mut Runner, icons: Option<&mut IconCache>) {
    const ROW_ACTIONS: [(&str, &str); 5] = [
        ("use", "studio.manager.use"),
        ("edit", "studio.manager.edit"),
        ("rename", "studio.manager.rename"),
        ("export", "studio.manager.export"),
        ("delete", "studio.manager.delete"),
    ];
    let action_width = dtool_design::size("button.width");
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if dtool_design::button(ui, text("studio.manager.new")).clicked() {
            state.open_editor(PersonaId::Vector);
        }
        if dtool_design::button(ui, text("studio.manager.import")).clicked() {
            action = Some(("import", String::new()));
        }
    });
    if state.studios.is_empty() {
        ui.label(text("studio.manager.empty"));
    }
    for studio in &state.studios {
        ui.separator();
        ui.add(egui::Label::new(&studio.name).wrap());
        let mut show_action = |ui: &mut egui::Ui, index: usize| {
            let (action_id, label) = ROW_ACTIONS[index];
            if dtool_design::button(ui, text(label)).clicked() {
                action = Some((action_id, studio.name.clone()));
            }
        };
        if dtool_design::panel_column_count(ui, ui.available_width(), ROW_ACTIONS.len(), action_width)
            == ROW_ACTIONS.len() {
            ui.horizontal(|ui| {
                for index in 0..ROW_ACTIONS.len() {
                    show_action(ui, index);
                }
            });
        } else {
            dtool_design::panel_columns(ui, ROW_ACTIONS.len(), action_width, show_action);
        }
    }
    let mut commit_rename = None;
    if let Some((old, new)) = state.renaming.as_mut() {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(new);
            if ui.button(text("studio.editor.save")).clicked() { commit_rename = Some((old.clone(), new.clone())); }
        });
    }
    let transfer = matches!(&action, Some(("import" | "export", _)));
    let result = match action {
        Some(("use", name)) => { state.request_switch(&name); Ok(()) }
        Some(("edit", name)) => state.open_existing(&name),
        Some(("rename", name)) => { state.renaming = Some((name.clone(), name)); Ok(()) }
        Some(("import", _)) => transfer_studio(run, None).and_then(|imported| {
            if imported { state.refresh(run)?; }
            Ok(())
        }),
        Some(("export", name)) => transfer_studio(run, Some(&name)).map(|_| ()),
        Some((_, name)) => state.delete(run, &name),
        None => match commit_rename {
            Some((old, new)) => state.rename(run, &old, &new),
            None => Ok(()),
        },
    };
    if transfer {
        state.error = result.err().map(|error| error.message_key);
    } else if let Err(error) = result {
        state.error = Some(error.message_key);
    }
    if state.editing.is_none()
        && let Some(error) = &state.error {
        ui.colored_label(ui.visuals().error_fg_color, text(error));
    }
    if state.editing.is_some() {
        ui.separator();
        show_editor_with_icons(ui, state, run, icons);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioRequest {
    Create,
    Manager,
    Toolbar,
}

#[derive(Debug)]
struct ToolbarDraft {
    workspace: String,
    items: Vec<String>,
}

#[derive(Debug, Default)]
pub struct StudioWindows {
    pub manager_open: bool,
    pub state: StudioManagerState,
    toolbar: Option<ToolbarDraft>,
}

impl StudioWindows {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.manager_open || self.state.editing.is_some() || self.toolbar.is_some()
    }

    pub(crate) fn toolbar_open(&self) -> bool {
        self.toolbar.is_some()
    }

    pub(crate) fn toolbar_draft(&mut self, workspace: &str) -> Option<&mut Vec<String>> {
        self.toolbar.as_mut().filter(|draft| draft.workspace == workspace).map(|draft| &mut draft.items)
    }

    pub fn open(&mut self, request: StudioRequest, run: &mut Runner) {
        self.state.error = None;
        if request == StudioRequest::Toolbar {
            match run(&Command::new("ui.persona.query")) {
                Ok(current) => {
                    if let (Some(workspace), Ok(items)) = (current["id"].as_str(), serde_json::from_value(current["layout"]["toolbar"].clone())) {
                        self.toolbar = Some(ToolbarDraft { workspace: workspace.to_owned(), items });
                    }
                }
                Err(error) => self.state.error = Some(error.message_key),
            }
            return;
        }
        if let Err(error) = self.state.refresh(run) {
            self.state.error = Some(error.code.to_string());
        }
        match request {
            StudioRequest::Create => self.state.open_editor(PersonaId::Vector),
            StudioRequest::Manager => self.manager_open = true,
            StudioRequest::Toolbar => unreachable!(),
        }
    }

    pub fn close(&mut self) {
        self.manager_open = false;
        self.toolbar = None;
        self.state.cancel_editor();
        self.state.renaming = None;
    }

    pub fn show(&mut self, ctx: &egui::Context, run: &mut Runner) -> Option<Command> {
        self.show_with_icons(ctx, run, None)
    }

    pub(crate) fn show_with_icons(&mut self, ctx: &egui::Context, run: &mut Runner, icons: Option<&mut IconCache>) -> Option<Command> {
        if let Some(draft) = self.toolbar.as_mut() {
            let mut open = true;
            let mut cancel = false;
            let viewport = ctx.content_rect();
            let width = (viewport.width() - 32.0).clamp(1.0, 560.0);
            let body_height = (viewport.height() - 180.0).clamp(64.0, 460.0);
            let save = egui::Window::new(text("studio.toolbar.customise"))
                .id(egui::Id::unique("studio.toolbar.window"))
                .default_pos(egui::pos2(160.0, 125.0))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_width(width);
                    ui.label(text("studio.toolbar.drag_hint"));
                    use egui::scroll_area::ScrollSource;
                    egui::ScrollArea::vertical()
                        .scroll_source(ScrollSource::SCROLL_BAR | ScrollSource::MOUSE_WHEEL)
                        .max_height(body_height)
                        .show(ui, |ui| toolbar_editor(ui, &mut draft.items, icons));
                    if let Some(error) = &self.state.error {
                        ui.colored_label(ui.visuals().error_fg_color, text(error));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if dtool_design::button(ui, text("pref.reset")).clicked() {
                            draft.items = crate::persona::DEFAULT_TOOLBAR.map(String::from).into();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let save = dtool_design::primary_button(ui, text("studio.editor.save"), egui::vec2(72.0, 24.0)).clicked();
                            cancel = dtool_design::button(ui, text("studio.editor.cancel")).clicked();
                            save
                        }).inner
                    }).inner
                }).and_then(|window| window.inner).unwrap_or(false);
            if save {
                match crate::persona::save_toolbar(&draft.workspace, &draft.items) {
                    Ok(()) => self.toolbar = None,
                    Err(error) => self.state.error = Some(error.message_key),
                }
            } else if cancel || !open {
                self.toolbar = None;
            }
            return None;
        }
        if self.manager_open {
            let mut open = true;
            let viewport = ctx.content_rect();
            let width = (viewport.width() - 32.0).clamp(1.0, 560.0);
            egui::Window::new(text("studio.manager.title"))
                .id(egui::Id::unique("studio.manager.window"))
                .default_width(width)
                .max_width(width)
                .max_height((viewport.height() - 56.0).max(1.0))
                .vscroll(true)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_width(width);
                    show_manager_with_icons(ui, &mut self.state, run, icons);
                });
            if !open {
                self.close();
            }
        } else if self.state.editing.is_some() {
            let mut open = true;
            let width = (ctx.content_rect().width() - 32.0).clamp(1.0, 560.0);
            egui::Window::new(text("studio.editor.title"))
                .id(egui::Id::unique("studio.editor.window"))
                .default_width(width)
                .max_width(width)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_width(width);
                    show_editor_with_icons(ui, &mut self.state, run, icons);
                });
            if !open {
                self.state.cancel_editor();
            }
        }
        let switch = self.state.pending_switch.take();
        if switch.is_some() {
            self.close();
        }
        switch
    }
}
