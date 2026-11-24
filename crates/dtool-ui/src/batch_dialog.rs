use dtool_export::batch::{
    self, e06_label_key, is_running, output_skip_reason, BatchAction, BatchHandle, BatchInputId,
    BatchManifest, BatchOutput, BatchProblem, BatchSnapshot, BatchStatus, Destination, OverwritePolicy,
    E06_OFFERED_FORMATS,
};
use dtool_export::macros::{Macro, MacroLibrary};
use dtool_export::ExportFormat;
use dtool_strings::text;
use egui;

#[derive(Default)]
pub struct BatchDialog {
    pub open: bool,
    pub manifest: BatchManifest,
    pub selected: Option<BatchInputId>,
    pub problems: Vec<BatchProblem>,
    pub library: MacroLibrary,
    add_open: bool,
    paths: String,
    handle: Option<BatchHandle>,
    snapshot: Option<BatchSnapshot>,
    run_error: Option<String>,
}

impl BatchDialog {
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = std::path::PathBuf>) {
        for path in paths {
            match self.manifest.add(path) {
                Ok(id) => self.selected = Some(id),
                Err(problem) => self.problems.push(problem),
            }
        }
    }
    pub fn set_library(&mut self, library: MacroLibrary) {
        self.library = library;
    }
    pub fn load_library(&mut self) {
        self.library = MacroLibrary::load_default();
    }
    pub fn available_macros(&self) -> Vec<(String, Vec<String>)> {
        self.library
            .categories
            .iter()
            .map(|(category, macros)| (category.clone(), macros.iter().map(|m| m.name.clone()).collect()))
            .collect()
    }
    pub fn apply_macro(&mut self, entry: &Macro) -> usize {
        let before = self.manifest.actions.len();
        for step in entry.enabled_steps() {
            if dtool_export::batch::BATCH_ACTIONS.contains(&step.command.as_str()) {
                self.manifest.actions.push(BatchAction {
                    command: step.command.clone(),
                    params: step.params.clone(),
                });
            }
        }
        self.manifest.actions.len() - before
    }
    pub fn offered_formats() -> &'static [ExportFormat] {
        E06_OFFERED_FORMATS
    }
    pub fn add_output(&mut self, format: ExportFormat, webp_lossy: bool) {
        self.manifest.outputs.push(BatchOutput {
            format,
            width: None,
            height: None,
            keep_aspect: true,
            dest: Destination::Original,
            name: "{name}.{format}".into(),
            overwrite: OverwritePolicy::Fail,
            webp_lossy,
        });
    }
    pub fn refresh_snapshot(&mut self) {
        if let Some(handle) = &self.handle {
            self.snapshot = Some(batch::query(handle));
        }
    }
    pub fn start_run(&mut self) -> Result<BatchHandle, String> {
        self.run_error = None;
        let handle = batch::prepare(self.manifest.clone()).map_err(|error| error.code.clone())?;
        self.handle = Some(handle.clone());
        let worker = handle.clone();
        std::thread::spawn(move || {
            let _ = batch::start(&worker);
        });
        self.refresh_snapshot();
        Ok(handle)
    }
    pub fn cancel_run(&mut self) {
        if let Some(handle) = &self.handle {
            self.snapshot = Some(batch::cancel(handle));
        }
    }
    pub fn is_busy(&self) -> bool {
        self.handle.as_ref().is_some_and(is_running)
    }
    pub fn can_close(&self) -> bool {
        !self.is_busy()
    }
    pub fn active_handle(&self) -> Option<&BatchHandle> {
        self.handle.as_ref()
    }
    pub fn last_snapshot(&self) -> Option<&BatchSnapshot> {
        self.snapshot.as_ref()
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        self.refresh_snapshot();
        let busy = self.is_busy();
        let mut open = self.open;
        egui::Window::new(text("batch.title")).open(&mut open).show(ctx, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                dtool_design::panel_header(ui, text("batch.sources"));
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("batch.add")).clicked() {
                        self.add_open = true;
                    }
                    if ui
                        .add_enabled_ui(self.selected.is_some(), |ui| dtool_design::button(ui, text("batch.remove")))
                        .inner
                        .clicked()
                    {
                        if let Some(id) = self.selected.take() {
                            self.manifest.remove(id);
                        }
                    }
                });
                let mut moved = None;
                for (index, input) in self.manifest.inputs.iter().enumerate() {
                    let response = ui.dnd_drag_source(egui::Id::new(("batch-source", input.id.0)), input.id, |ui| {
                        dtool_design::tab(ui, self.selected == Some(input.id), input.path.display().to_string())
                    });
                    if response.inner.clicked() {
                        self.selected = Some(input.id);
                    }
                    if let Some(id) = response.response.dnd_release_payload::<BatchInputId>() {
                        moved = Some((*id, index));
                    }
                }
                if let Some((id, index)) = moved {
                    self.manifest.reorder(id, index);
                }
                for problem in &self.problems {
                    ui.label(text(&problem.message_key));
                }

                dtool_design::panel_header(ui, text("batch.available_macros"));
                let mut chosen: Option<Macro> = None;
                ui.vertical(|ui| {
                    for (category, macros) in &self.library.categories {
                        ui.label(category.as_str());
                        ui.indent(("batch-macro-category", category.as_str()), |ui| {
                            for entry in macros {
                                if dtool_design::button(ui, entry.name.as_str()).clicked() {
                                    chosen = Some(entry.clone());
                                }
                            }
                        });
                    }
                    for command in dtool_export::batch::BATCH_ACTIONS {
                        if dtool_design::button(ui, *command).clicked() {
                            self.manifest.actions.push(BatchAction {
                                command: command.to_string(),
                                params: serde_json::json!({}),
                            });
                        }
                    }
                });

                if let Some(entry) = chosen {
                    self.apply_macro(&entry);
                }

                dtool_design::panel_header(ui, text("batch.applied_macros"));
                ui.vertical(|ui| {
                    for action in &self.manifest.actions {
                        ui.label(&action.command);
                    }
                });

                dtool_design::panel_header(ui, text("batch.output"));
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("batch.save_original")).clicked() {
                        self.manifest.outputs.iter_mut().for_each(|o| o.dest = Destination::Original);
                    }
                    if dtool_design::button(ui, text("batch.save_into")).clicked() {
                        let folder = std::path::PathBuf::from(".");
                        self.manifest.outputs.iter_mut().for_each(|o| o.dest = Destination::Folder(folder.clone()));
                    }
                });
                ui.horizontal(|ui| {
                    for format in E06_OFFERED_FORMATS {
                        if dtool_design::button(ui, text(e06_label_key(*format))).clicked() {
                            self.add_output(*format, false);
                        }
                    }
                });

                dtool_design::toggle(ui, &mut self.manifest.parallel, text("batch.parallel"));

                let mut remove_at = None;
                for (index, output) in self.manifest.outputs.iter_mut().enumerate() {
                    dtool_design::panel_frame(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(text(e06_label_key(output.format)));
                            if let Some(reason) = output_skip_reason(output) {
                                ui.label(text("error.export.option_unavailable"));
                                ui.label(reason);
                            }
                            edit_dimension(ui, &mut output.width, "dialog.export.width.label");
                            edit_dimension(ui, &mut output.height, "dialog.export.height.label");
                            dtool_design::toggle(ui, &mut output.keep_aspect, text("batch.aspect_lock"));
                            if dtool_design::button(ui, text("batch.remove")).clicked() {
                                remove_at = Some(index);
                            }
                        });
                    });
                }
                if let Some(index) = remove_at {
                    self.manifest.outputs.remove(index);
                }
            });

            ui.horizontal(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if dtool_design::button(ui, text("panel.console.run")).clicked() {
                        if let Err(code) = self.start_run() {
                            self.run_error = Some(code);
                        }
                    }
                });
                ui.add_enabled_ui(busy, |ui| {
                    if dtool_design::button(ui, text("button.cancel")).clicked() {
                        self.cancel_run();
                    }
                });
            });
            if let Some(code) = &self.run_error {
                ui.label(code);
            }
            if let Some(snapshot) = &self.snapshot {
                ui.label(format!("{}/{}", snapshot.done_units, snapshot.total_units));
                if snapshot.status == BatchStatus::Cancelled {
                    ui.label(text("error.export.cancelled"));
                }
                for file in &snapshot.files {
                    for warning in &file.warnings {
                        ui.label(text("error.export.option_unavailable"));
                        ui.label(warning.as_str());
                    }
                }
            }
        });
        self.open = open || !self.can_close();
        if self.add_open && !busy {
            let mut add_open = true;
            egui::Window::new(text("batch.add")).open(&mut add_open).show(ctx, |ui| {
                dtool_design::text_input_multiline(ui, &mut self.paths);
                if dtool_design::button(ui, text("batch.add")).clicked() {
                    let paths: Vec<_> = self
                        .paths
                        .lines()
                        .map(str::trim)
                        .filter(|path| !path.is_empty())
                        .map(std::path::PathBuf::from)
                        .collect();
                    self.add_paths(paths);
                    self.paths.clear();
                    self.add_open = false;
                }
            });
            self.add_open &= add_open;
        }
    }
}

fn edit_dimension(ui: &mut egui::Ui, dimension: &mut Option<u32>, key: &str) {
    let before = dimension.map(f64::from).unwrap_or(0.0);
    let mut value = before;
    dtool_design::number_field(ui, &mut value, &text(key));
    if value != before && value.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&value) {
        *dimension = if value == 0.0 { None } else { Some(value.round().max(1.0) as u32) };
    }
}
