use dtool_core::commands::image_place::DecodedImage;
use dtool_core::{Command, CommandError, ContextId, Scope, Session, Workspace};
use dtool_strings::text;
use serde_json::json;

pub const TOOL: &str = "tool.place_image";

#[derive(Default)]
pub struct PlaceTool {
    pub dialog_open: bool,
    pub path: String,
    pub error_key: Option<String>,
    pub link: bool,
    target: Option<ContextId>,
    previous_tool: String,
    image: Option<DecodedImage>,
    press: Option<[f32; 2]>,
    current_path: Option<String>,
    queue: Vec<String>,
    queued_images: Vec<(String, DecodedImage)>,
}

/// Normalize any drag direction and preserve the image ratio unless Shift frees it.
pub fn drag_geometry(start: [f32; 2], end: [f32; 2], aspect: f32, free: bool) -> Option<([f32; 2], [f32; 2])> {
    if !start.iter().chain(&end).all(|v| v.is_finite()) || !aspect.is_finite() || aspect <= 0.0 { return None; }
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let (mut w, mut h) = (dx.abs(), dy.abs());
    if !free { w = w.max(h * aspect); h = w / aspect; }
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() { return None; }
    Some(([if dx < 0.0 { start[0] - w } else { start[0] }, if dy < 0.0 { start[1] - h } else { start[1] }], [w, h]))
}

pub const PLACE_IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];

pub fn split_paths(text: &str) -> Vec<String> {
    text.split(|c| c == ';' || c == '\n')
        .map(|part| part.trim().trim_matches('"').trim())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn image_drop_paths(paths: &[std::path::PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| PLACE_IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())))
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

pub const DROP_PLACE_EXTENSIONS: [&str; 5] = ["pdf", "svg", "svgz", "ai", "eps"];
pub const DROP_TEXT_EXTENSIONS: [&str; 3] = ["txt", "rtf", "docx"];

pub fn drop_commands(paths: &[std::path::PathBuf]) -> (Vec<Command>, Vec<Command>) {
    let mut texts = Vec::new();
    let mut opens = Vec::new();
    for path in paths {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase) else { continue };
        let params = json!({ "path": path.to_string_lossy() });
        if DROP_TEXT_EXTENSIONS.contains(&ext.as_str()) {
            texts.push(Command::with_params("text.import", params));
        } else if DROP_PLACE_EXTENSIONS.contains(&ext.as_str()) {
            opens.push(Command::with_params(dtool_core::import_hook::IMPORT_PLACE, params));
        }
    }
    (texts, opens)
}

fn place_document(workspace: &mut Workspace, command: Command) -> Result<(), CommandError> {
    if workspace.active().is_none() {
        let open = Command::with_params("doc.open", command.params);
        return workspace.run(Scope::active(), open).1.map(|_| ());
    }
    let path = command.params.get("path").and_then(|value| value.as_str()).ok_or_else(CommandError::invalid_params)?;
    dtool_io::import_into_current(workspace, std::path::Path::new(path)).map(|_| ())
}

impl PlaceTool {
    pub fn drop_files(&mut self, workspace: &mut Workspace, paths: &[std::path::PathBuf]) -> Result<bool, CommandError> {
        let (texts, opens) = drop_commands(paths);
        let images = image_drop_paths(paths);
        if texts.is_empty() && opens.is_empty() && images.is_empty() {
            return Ok(false);
        }
        let mut first_error = None;
        for command in texts {
            if let Err(error) = workspace.run(Scope::active(), command).1 {
                first_error.get_or_insert(error);
            }
        }
        if !images.is_empty() {
            if let Err(error) = self.drop_images(workspace, images) {
                first_error.get_or_insert(error);
            }
        }
        for command in opens {
            if let Err(error) = place_document(workspace, command) {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(true),
        }
    }

    fn drop_images(&mut self, workspace: &mut Workspace, paths: Vec<String>) -> Result<bool, CommandError> {
        self.cancel(workspace);
        let id = workspace.active().ok_or_else(CommandError::no_document)?;
        self.target = Some(id);
        self.previous_tool = workspace.get(id).ok_or_else(CommandError::context_not_found)?.session.active_tool.clone();
        self.queue_set(workspace, paths).map(|_| true)
    }

    pub fn open(&mut self, workspace: &mut Workspace) -> Result<(), CommandError> {
        self.cancel(workspace);
        let id = workspace.active().ok_or_else(CommandError::no_document)?;
        let session = &workspace.get(id).ok_or_else(CommandError::context_not_found)?.session;
        self.target = Some(id);
        self.previous_tool = session.active_tool.clone();
        self.error_key = None;
        self.path.clear();
        self.dialog_open = true;
        Ok(())
    }

    pub fn arm(&mut self, workspace: &mut Workspace, image: DecodedImage) -> Result<(), CommandError> {
        let id = self.target.or_else(|| workspace.active()).ok_or_else(CommandError::no_document)?;
        let context = workspace.get_mut(id).ok_or_else(CommandError::context_not_found)?;
        let [w,h] = image.resource.pixel_size;
        if w == 0 || h == 0 || (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) != Some(image.rgba.len()) {
            return Err(CommandError::invalid_params());
        }
        if self.target.is_none() { self.previous_tool = context.session.active_tool.clone(); }
        self.target = Some(id);
        self.image = Some(image);
        self.press = None;
        self.dialog_open = false;
        self.error_key = None;
        context.session.active_tool = TOOL.into();
        Ok(())
    }

    pub fn active_for(&self, id: ContextId) -> bool { self.target == Some(id) && self.image.is_some() }

    pub fn cancel(&mut self, workspace: &mut Workspace) {
        if let Some(path) = self.current_path.take() {
            self.queue.insert(0, path);
        }
        if let Some(context) = self.target.and_then(|id| workspace.get_mut(id)) {
            self.restore_tool(&mut context.session);
        }
        self.image = None;
        self.target = None;
        self.press = None;
        self.dialog_open = false;
    }

    pub fn queue_set(&mut self, workspace: &mut Workspace, paths: Vec<String>) -> Result<(), CommandError> {
        if paths.is_empty() {
            return Err(CommandError::invalid_params());
        }
        self.queued_images.clear();
        self.current_path = None;
        self.image = None;
        self.queue = paths;
        self.arm_next(workspace)
    }

    pub fn queue_set_decoded(&mut self, workspace: &mut Workspace, images: Vec<(String, DecodedImage)>) -> Result<(), CommandError> {
        if images.is_empty() {
            return Err(CommandError::invalid_params());
        }
        self.queue = images.iter().map(|(path, _)| path.clone()).collect();
        self.queued_images = images;
        self.current_path = None;
        self.image = None;
        self.arm_next(workspace)
    }

    pub fn queue_next(&self) -> Option<&str> {
        self.current_path.as_deref().or_else(|| self.queue.first().map(String::as_str))
    }

    pub fn queue_remaining(&self) -> &[String] {
        &self.queue
    }

    pub fn queue_clear(&mut self) {
        self.queue.clear();
        self.queued_images.clear();
        self.current_path = None;
    }

    fn take_queued_image(&mut self, path: &str) -> Option<DecodedImage> {
        if let Some(index) = self.queued_images.iter().position(|(item, _)| item == path) {
            return Some(self.queued_images.remove(index).1);
        }
        None
    }

    fn arm_next(&mut self, workspace: &mut Workspace) -> Result<(), CommandError> {
        let Some(path) = self.queue.first().cloned() else {
            return Ok(());
        };
        self.queue.remove(0);
        let image = match self.take_queued_image(&path) {
            Some(image) => image,
            None => dtool_io::image_import::decode_image(std::path::Path::new(&path))?,
        };
        self.current_path = Some(path);
        self.arm(workspace, image)
    }

    fn arm_next_on_session(&mut self, id: ContextId, session: &mut Session) -> Result<(), CommandError> {
        let Some(path) = self.queue.first().cloned() else {
            return Ok(());
        };
        self.queue.remove(0);
        let image = match self.take_queued_image(&path) {
            Some(image) => image,
            None => dtool_io::image_import::decode_image(std::path::Path::new(&path))?,
        };
        let [w, h] = image.resource.pixel_size;
        if w == 0 || h == 0 || (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) != Some(image.rgba.len()) {
            return Err(CommandError::invalid_params());
        }
        if self.target.is_none() { self.previous_tool = session.active_tool.clone(); }
        self.target = Some(id);
        self.current_path = Some(path);
        self.image = Some(image);
        self.press = None;
        self.dialog_open = false;
        self.error_key = None;
        session.active_tool = TOOL.into();
        Ok(())
    }

    fn restore_tool(&self, session: &mut Session) {
        if session.active_tool == TOOL { session.active_tool = self.previous_tool.clone(); }
    }

    pub fn place(&mut self, id: ContextId, session: &mut Session, position: [f32; 2], size: Option<[f32; 2]>) -> Result<(), CommandError> {
        if self.target != Some(id) { return Err(CommandError::context_not_found()); }
        let image = self.image.as_ref().ok_or_else(CommandError::invalid_params)?;
        let mut params = json!({"source":image.resource,"pixel_size":image.resource.pixel_size,"rgba":image.rgba,"pos":position});
        if let Some(size) = size {
            params["size"] = json!(size);
        } else if let Some(frame) = session.document.objects().iter().rev().find(|object| {
            !object.locked && dtool_core::scene::is_drawn_in_view(&session.document, &session.view, object.id)
                && dtool_core::commands::picture_frame::contains_point(&session.document, object, position)
        }) {
            params["picture_frame"] = json!(frame.id);
        }
        if self.link { params["link"] = json!(true); }
        session.run_named("image.place", params)?;
        self.image = None;
        self.current_path = None;
        self.press = None;
        if !self.queue.is_empty() {
            let id = self.target.ok_or_else(CommandError::no_document)?;
            self.arm_next_on_session(id, session)?;
        } else {
            self.restore_tool(session);
            self.target = None;
        }
        Ok(())
    }

    pub fn show_dialog(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) -> Vec<std::path::PathBuf> {
        if !self.dialog_open { return Vec::new(); }
        let (mut confirm, mut cancel) = (false, false);
        egui::Window::new(text("menu.file.place")).collapsible(false).show(ui.ctx(), |ui| {
            dtool_design::dialog_frame(ui, |ui| {
                ui.label(text("dialog.file.path"));
                ui.text_edit_singleline(&mut self.path);
                if let Some(key) = &self.error_key { ui.label(text(key)); }
                ui.horizontal(|ui| {
                    confirm = dtool_design::button(ui, text("button.ok")).clicked();
                    cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                });
                cancel |= ui.input(|input| input.key_pressed(egui::Key::Escape));
            });
        });
        if cancel { self.cancel(workspace); }
        else if confirm {
            let paths = split_paths(&self.path);
            let (pdfs, files): (Vec<_>, Vec<_>) = paths.iter().map(std::path::PathBuf::from)
                .partition(|path| crate::pdf_import_dialog::is_pdf(path));
            let (texts, opens) = drop_commands(&files);
            if !pdfs.is_empty() || !texts.is_empty() || !opens.is_empty() {
                self.cancel(workspace);
                self.dialog_open = false;
                if let Err(error) = self.drop_files(workspace, &files) {
                    self.dialog_open = true;
                    self.error_key = Some(error.message_key);
                }
                return pdfs;
            }
            let result = if paths.len() > 1 {
                self.queue_set(workspace, paths)
            } else {
                dtool_io::image_import::decode_image(std::path::Path::new(paths.first().map(String::as_str).unwrap_or_default()))
                    .and_then(|image| self.arm(workspace, image))
            };
            if let Err(error) = result { self.error_key = Some(error.message_key); }
        }
        Vec::new()
    }

    pub fn canvas(&mut self, ui: &mut egui::Ui, response: &egui::Response, id: ContextId, session: &mut Session, input: bool) {
        if !self.active_for(id) { return; }
        if session.active_tool != TOOL || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(path) = self.current_path.take() {
                self.queue.insert(0, path);
            }
            self.restore_tool(session);
            self.image = None;
            self.target = None;
            self.press = None;
            return;
        }
        if !input { return; }
        if response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
        let point = |p: egui::Pos2| dtool_core::view_to_doc(&session.view, [p.x - response.rect.min.x, p.y - response.rect.min.y]);
        if response.drag_started() { self.press = ui.input(|i| i.pointer.press_origin()).map(point); }
        let end = response.interact_pointer_pos().map(point);
        let free = ui.input(|i| i.modifiers.shift);
        let aspect = self.image.as_ref().map(|image| image.resource.pixel_size[0] as f32 / image.resource.pixel_size[1] as f32).unwrap_or(1.0);
        let geometry = self.press.zip(end).and_then(|(start,end)| drag_geometry(start,end,aspect,free));
        if let Some((pos,size)) = geometry {
            let corners = [pos,[pos[0]+size[0],pos[1]],[pos[0]+size[0],pos[1]+size[1]],[pos[0],pos[1]+size[1]]]
                .map(|p| { let p=dtool_core::doc_to_view(&session.view,p); egui::pos2(response.rect.min.x+p[0],response.rect.min.y+p[1]) });
            ui.painter_at(response.rect).add(egui::Shape::closed_line(corners.to_vec(),ui.visuals().selection.stroke));
        }
        let placement = if response.drag_stopped() { self.press = None; geometry.map(|(p,s)|(p,Some(s))) }
            else if response.clicked() { end.map(|p|(p,None)) } else { None };
        if let Some((pos,size)) = placement {
            if let Err(error) = self.place(id,session,pos,size) { self.error_key = Some(error.message_key); }
            ui.ctx().request_repaint();
        }
    }
}
