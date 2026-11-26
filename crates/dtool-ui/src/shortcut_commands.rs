use crate::shell::AppShell;
use crate::shortcuts::{Focus, Modifiers, ShortcutMap, TabAction};
use dtool_core::Session;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAction {
    ZoomIn,
    ZoomOut,
    OpenDocument,
    SaveDocumentAs,
    NewDocument,
    NewFromClipboard,
    SaveDocument,
    CloseDocument,
    OpenFind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutTarget {
    Run { command: &'static str, params: &'static str },
    Tool(&'static str),
    Shell(ShellAction),
    NotImplemented,
}

const P: &str = "Serif.Interop.Persona.Commands.";

const fn run(command: &'static str) -> ShortcutTarget {
    ShortcutTarget::Run { command, params: "{}" }
}

const fn run_with(command: &'static str, params: &'static str) -> ShortcutTarget {
    ShortcutTarget::Run { command, params }
}

pub const PERSONA_TABLE: &[(&str, ShortcutTarget)] = &[
    ("UndoCommand", run("edit.undo")),
    ("RedoCommand", run("edit.redo")),
    ("CopyCommand", run("edit.copy")),
    ("CutCommand", run("edit.cut")),
    ("DeleteObjectsCommand", run("edit.delete")),
    ("PasteCommand", run("edit.paste")),
    ("PasteFXCommand", run("edit.paste_fx")),
    ("PasteStyleCommand", run("edit.paste_style")),
    ("PasteWithoutFormatCommand", run("edit.paste_without_format")),
    ("DuplicateObjectsCommand", run("edit.duplicate")),
    ("SelectAllCommand", run("selection.all")),
    ("DeselectPixelsCommand", run("selection.none")),
    ("GroupNodesCommand", run("node.group")),
    ("UngroupNodesCommand", run("node.ungroup")),
    ("MoveToFrontCommand", run_with("node.arrange", r#"{"step":"to_front"}"#)),
    ("MoveToBackCommand", run_with("node.arrange", r#"{"step":"to_back"}"#)),
    ("ForwardOneCommand", run_with("node.arrange", r#"{"step":"forward"}"#)),
    ("BackOneCommand", run_with("node.arrange", r#"{"step":"backward"}"#)),
    ("LockCommand", run_with("node.lock_state", r#"{"state":"locked"}"#)),
    ("UnlockCommand", run_with("node.lock_state", r#"{"state":"unlocked"}"#)),
    ("QuickMaskPopupCommand", run("selection.quickmask.toggle")),
    ("ResetRotationCommand", run("view.rotate.reset")),
    ("ZoomTo100Command", run("view.zoom.actual")),
    ("ZoomToActualSizeCommand", run("view.zoom.actual")),
    ("ZoomTo200Command", run_with("view.zoom.set", r#"{"zoom":2.0}"#)),
    ("ZoomTo400Command", run_with("view.zoom.set", r#"{"zoom":4.0}"#)),
    ("ZoomTo800Command", run_with("view.zoom.set", r#"{"zoom":8.0}"#)),
    ("ZoomInCommand", ShortcutTarget::Shell(ShellAction::ZoomIn)),
    ("ZoomOutCommand", ShortcutTarget::Shell(ShellAction::ZoomOut)),
    ("AlignLeftCommand", run("layer.arrange.align.left")),
    ("AlignCentreCommand", run("layer.arrange.align.centre")),
    ("AlignRightCommand", run("layer.arrange.align.right")),
    ("AlignTopCommand", run("layer.arrange.align.top")),
    ("AlignMiddleCommand", run("layer.arrange.align.middle")),
    ("AlignBottomCommand", run("layer.arrange.align.bottom")),
    ("TextAlignLeftCommand", run_with("text.align", r#"{"align":"left"}"#)),
    ("TextAlignCentreCommand", run_with("text.align", r#"{"align":"centre"}"#)),
    ("TextAlignRightCommand", run_with("text.align", r#"{"align":"right"}"#)),
    ("TextAlignJustifyLeftCommand", run_with("text.align", r#"{"align":"justify_left"}"#)),
    ("TextAlignJustifyCentreCommand", run_with("text.align", r#"{"align":"justify_centre"}"#)),
    ("TextAlignJustifyRightCommand", run_with("text.align", r#"{"align":"justify_right"}"#)),
    ("TextAlignJustifyAllCommand", run_with("text.align", r#"{"align":"justify_all"}"#)),
    ("ToggleRulersCommand", run("ruler.toggle")),
    ("ToggleGuidesCommand", run("guides.toggle")),
    ("ToggleSnappingCommand", run("snap.toggle")),
    ("InsertIndentToHereCommand", run_with("text.special.insert", r#"{"what":"indent_to_here"}"#)),
];

pub const APP_TABLE: &[(&str, ShortcutTarget)] = &[
    ("Serif.Affinity.Commands.OpenDocumentCommand", ShortcutTarget::Shell(ShellAction::OpenDocument)),
    ("Serif.Interop.Persona.Commands.SaveDocumentAsCommand", ShortcutTarget::Shell(ShellAction::SaveDocumentAs)),
    ("Serif.Affinity.Commands.NewDocumentCommand", ShortcutTarget::Shell(ShellAction::NewDocument)),
    ("Serif.Interop.Persona.Commands.PasteToNewDocumentCommand", ShortcutTarget::Shell(ShellAction::NewFromClipboard)),
    ("Serif.Interop.Persona.Commands.SaveDocumentCommand", ShortcutTarget::Shell(ShellAction::SaveDocument)),
    ("Serif.Interop.Persona.Commands.CloseViewCommand", ShortcutTarget::Shell(ShellAction::CloseDocument)),
    (crate::shortcuts::FIND_OPEN_COMMAND, ShortcutTarget::Shell(ShellAction::OpenFind)),
];

pub const TOOL_TABLE: &[(&str, &str)] = &[
    ("tool.Select", "tool.move"),
    ("tool.Node", "tool.node"),
    ("tool.Pen", "tool.pen"),
    ("tool.Pencil", "tool.pen.pencil"),
    ("tool.PictureFrameRectangle", "tool.picture_frame.rect"),
    ("tool.ShapeRectangle", "tool.shape.rectangle"),
    ("tool.ShapeEllipse", "tool.shape.ellipse"),
    ("tool.ArtText", "tool.text.artistic"),
    ("tool.FrameText", "tool.text.frame"),
    ("tool.TableText", "tool.table"),
    ("tool.Knife", "tool.knife"),
    ("tool.VectorFill", "tool.gradient"),
    ("tool.RasterFloodFill", "tool.flood_fill"),
    ("tool.VectorTransparency", "tool.transparency"),
    ("tool.RasterPaintBrush", "tool.paint.brush"),
    ("tool.ColourPicker", "tool.eyedropper"),
    ("tool.FormatDropper", "tool.style_picker"),
];

pub fn entries() -> Vec<(String, ShortcutTarget)> {
    let mut all: Vec<(String, ShortcutTarget)> = Vec::new();
    all.extend(PERSONA_TABLE.iter().map(|(id, target)| (format!("{P}{id}"), *target)));
    all.extend(APP_TABLE.iter().map(|(id, target)| ((*id).to_string(), *target)));
    all.extend(TOOL_TABLE.iter().map(|(id, tool)| ((*id).to_string(), ShortcutTarget::Tool(tool))));
    all
}

pub fn target_for(id: &str) -> Option<ShortcutTarget> {
    entries().into_iter().find(|(known, _)| known == id).map(|(_, target)| target)
}

pub fn is_reserved(key: &str, modifiers: Modifiers) -> bool {
    match key {
        "Delete" | "Tab" | "Up" | "Down" | "Left" | "Right" => true,
        "C" | "V" => modifiers.ctrl && modifiers.shift && !modifiers.alt,
        _ => false,
    }
}

pub fn key_name(key: egui::Key) -> Option<String> {
    use egui::Key;
    let fixed = match key {
        Key::Plus | Key::Equals => "OemPlus",
        Key::Minus => "OemMinus",
        Key::Comma => "OemComma",
        Key::Semicolon => "Oem1",
        Key::Backtick => "Oem3",
        Key::Backslash => "Oem5",
        Key::Quote => "OemQuotes",
        Key::PageUp => "PageUp",
        Key::PageDown => "Next",
        Key::Tab => "Tab",
        Key::Delete => "Delete",
        Key::Backspace => "Back",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_string());
    }
    let name = key.name();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_digit() => Some(format!("D{c}")),
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase().to_string()),
        (Some('F'), Some(_)) if name[1..].chars().all(|c| c.is_ascii_digit()) => Some(name.to_string()),
        _ => None,
    }
}

pub fn modifiers_from(m: egui::Modifiers) -> Modifiers {
    Modifiers { ctrl: m.ctrl || m.command, shift: m.shift, alt: m.alt }
}

pub fn resolve(map: &ShortcutMap, focus: Focus, key: &str, modifiers: Modifiers) -> Option<ShortcutTarget> {
    if focus != Focus::Canvas || is_reserved(key, modifiers) {
        return None;
    }
    let bound = map.dispatch_all(focus, key, modifiers);
    if let Some(target) = entries().into_iter().find(|(id, _)| bound.iter().any(|b| b == id)).map(|(_, target)| target) {
        return Some(target);
    }
    bound.iter().any(|id| is_affinity_id(id)).then_some(ShortcutTarget::NotImplemented)
}

fn is_affinity_id(id: &str) -> bool {
    (id.starts_with("Serif.") || id.starts_with("tool.")) && id != crate::shortcuts::AFFINITY_EXPORT_COMMAND && id != crate::shortcuts::TOGGLE_UI_COMMAND
}

pub fn bound_tools(map: &ShortcutMap, focus: Focus, key: &str, modifiers: Modifiers) -> Vec<&'static str> {
    let mut tools: Vec<&'static str> = Vec::new();
    for id in map.dispatch_all(focus, key, modifiers) {
        if let Some((_, tool)) = TOOL_TABLE.iter().find(|(known, _)| *known == id) {
            if !tools.contains(tool) {
                tools.push(tool);
            }
        }
    }
    tools
}

pub fn resolve_cycling(map: &ShortcutMap, focus: Focus, key: &str, modifiers: Modifiers, current: &str, shift_cycles: bool) -> Option<ShortcutTarget> {
    if focus != Focus::Canvas || is_reserved(key, modifiers) {
        return None;
    }
    let cycle = shift_cycles && modifiers.shift && map.dispatch_all(focus, key, modifiers).is_empty();
    let lookup = if cycle { Modifiers { shift: false, ..modifiers } } else { modifiers };
    let tools = bound_tools(map, focus, key, lookup);
    if tools.len() > 1 {
        let current = dtool_core::context_toolbar::active_id(current);
        let at = tools.iter().position(|tool| dtool_core::context_toolbar::active_id(tool) == current);
        let next = match at {
            Some(index) if cycle || !shift_cycles => (index + 1) % tools.len(),
            Some(index) => index,
            None => 0,
        };
        return Some(ShortcutTarget::Tool(tools[next]));
    }
    if cycle && tools.len() == 1 {
        return Some(ShortcutTarget::Tool(tools[0]));
    }
    resolve(map, focus, key, modifiers)
}

pub fn run_target(session: &mut Session, target: ShortcutTarget) -> Option<Result<Value, dtool_protocol::CommandError>> {
    match target {
        ShortcutTarget::Run { command, params } => {
            let mut params = serde_json::from_str(params).unwrap_or_else(|_| json!({}));
            if command == "view.zoom.set" {
                if let Some(zoom) = params["zoom"].as_f64() {
                    params["zoom"] = json!(zoom * f64::from(session.actual_zoom()));
                }
            }
            Some(session.run_named(command, params))
        }
        ShortcutTarget::Tool(id) => Some(session.run_named("ui.tool.set", json!({ "id": id }))),
        ShortcutTarget::Shell(ShellAction::ZoomIn) => {
            let zoom = session.view.zoom * dtool_core::command::ZOOM_STEP;
            Some(session.run_named("view.zoom.set", json!({ "zoom": zoom })))
        }
        ShortcutTarget::Shell(ShellAction::ZoomOut) => {
            let zoom = session.view.zoom / dtool_core::command::ZOOM_STEP;
            Some(session.run_named("view.zoom.set", json!({ "zoom": zoom })))
        }
        ShortcutTarget::NotImplemented => Some(Err(dtool_protocol::CommandError::not_implemented())),
        ShortcutTarget::Shell(_) => None,
    }
}

fn pressed_keys(ctx: &egui::Context) -> Vec<(egui::Key, egui::Modifiers)> {
    ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    })
}

impl AppShell {
    pub fn handle_shortcut_key(&mut self, focus: Focus, key: &str, modifiers: Modifiers) -> Option<ShortcutTarget> {
        let current = self.workspace.session().map(|session| session.active_tool.clone()).unwrap_or_default();
        let target = resolve_cycling(&self.shortcuts, focus, key, modifiers, &current, self.prefs.shift_cycles_tools)?;
        match target {
            ShortcutTarget::Run { command: dtool_core::commands::quickmask::TOGGLE, .. }
                if self.menu_persona != "pixel" && !self.workspace.session()?.quick_mask.active => return None,
            ShortcutTarget::Run { command: "selection.all", .. } => self.select_all(),
            ShortcutTarget::Run { command: "edit.delete", .. } => {
                let session = self.workspace.session()?;
                if session.text_edit.is_some() || session.active_tool == "tool.stroke_width" {
                    return None;
                }
                let nodes = &mut self.app_canvas.input.nodes;
                let node_tool = crate::canvas_input::tool_kind(&session.active_tool) == crate::canvas_input::ToolKind::Node;
                if node_tool { nodes.sync(session); }
                if node_tool && nodes.claims_delete() {
                    if nodes.grab.is_some() { return Some(target); }
                    for command in nodes.delete_commands(session) {
                        if let Err(error) = self.run_and_reconcile(command) {
                            self.status_bar.set_hint_key(&error.message_key);
                            break;
                        }
                    }
                } else {
                    self.delete_selection();
                }
            }
            ShortcutTarget::Run { command, .. }
                if command.starts_with("layer.arrange.align.")
                    && self.workspace.session().is_some_and(|session| {
                        crate::canvas_input::tool_kind(&session.active_tool) == crate::canvas_input::ToolKind::Node
                    }) =>
            {
                let session = self.workspace.session()?;
                if session.text_edit.is_some() { return None; }
                let nodes = &mut self.app_canvas.input.nodes;
                nodes.sync(session);
                let (axis, fraction) = match command {
                    "layer.arrange.align.left" => (0, 0.0),
                    "layer.arrange.align.centre" => (0, 0.5),
                    "layer.arrange.align.right" => (0, 1.0),
                    "layer.arrange.align.top" => (1, 0.0),
                    "layer.arrange.align.middle" => (1, 0.5),
                    "layer.arrange.align.bottom" => (1, 1.0),
                    _ => return None,
                };
                if nodes.grab.is_none() && nodes.selected_count() >= 2
                    && let Some(edit) = nodes.align_command(session, axis, fraction)
                    && let Err(error) = self.run_and_reconcile(edit)
                {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
            ShortcutTarget::Run { command: command @ ("edit.undo" | "edit.redo"), .. } => {
                if self.workspace.session()?.text_edit.is_some() { return None; }
                self.run_history_shortcut(command);
            }
            ShortcutTarget::NotImplemented => {
                self.status_bar.set_hint_key(&dtool_protocol::CommandError::not_implemented().message_key);
            }
            ShortcutTarget::Shell(ShellAction::OpenDocument) => {
                self.browse_document();
            }
            ShortcutTarget::Shell(ShellAction::SaveDocumentAs) => {
                self.open_file_dialog(crate::dialogs::FileDialogMode::SaveAs);
            }
            ShortcutTarget::Shell(ShellAction::NewDocument) => {
                self.open_new_document_dialog();
            }
            ShortcutTarget::Shell(ShellAction::NewFromClipboard) => {
                let command = dtool_core::Command::new(dtool_core::clipboard::DOC_NEW_FROM_CLIPBOARD);
                if let Err(error) = self.run_and_reconcile(command) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
            ShortcutTarget::Shell(ShellAction::SaveDocument) => {
                self.workspace.active()?;
                self.save_active_document();
            }
            ShortcutTarget::Shell(ShellAction::OpenFind) => {
                self.find_host.open = true;
            }
            ShortcutTarget::Shell(ShellAction::CloseDocument) => {
                let id = self.workspace.active()?;
                self.request_close(id);
            }
            ShortcutTarget::Run { command: "view.zoom.actual", .. } => {
                use dtool_core::view::{ViewTarget, center_region};
                let id = self.workspace.active()?;
                let context = self.workspace.get_mut(id)?;
                let session = &mut context.session;
                if session.text_edit.is_some() {
                    return None;
                }
                let size = session.view_canvas_size.unwrap_or(self.canvas_size);
                let bounds = ViewTarget::at_view_center_with_hint(&session.document, &session.view, size, session.view_target_hint)
                    .and_then(|target| target.bounds(&session.document));
                match run_target(session, target)? {
                    Ok(_) => {
                        if let Some(bounds) = bounds {
                            center_region(&mut session.view, bounds, size);
                        }
                        context.view_state = session.view.clone();
                    }
                    Err(error) => self.status_bar.set_hint_key(&error.message_key),
                }
            }
            ShortcutTarget::Run { command: "text.special.insert", params } if params.contains("indent_to_here") => {
                if !self.insert_special("text.insert.spaces_and_tabs.indent_to_here") {
                    return None;
                }
            }
            _ => {
                let session = self.workspace.session_mut()?;
                if session.text_edit.is_some() {
                    return None;
                }
                if let Some(Err(error)) = run_target(session, target) {
                    self.status_bar.set_hint_key(&error.message_key);
                }
            }
        }
        Some(target)
    }


    pub fn handle_tab_command(&mut self, focus: Focus, modifiers: Modifiers) -> TabAction {
        let action = self.shortcuts.tab_action_for_edit(focus, modifiers, self.text_editing());
        if action == TabAction::ToggleUi {
            self.toggle_ui();
        }
        action
    }

    fn run_history_shortcut(&mut self, command: &str) {
        if let Err(error) = self.run_and_reconcile(dtool_core::Command::new(command)) {
            self.status_bar.set_hint_key(&error.message_key);
        }
    }

    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        self.handle_shortcuts_after_ui(ctx, self.widget_focused(ctx));
    }

    pub(crate) fn handle_shortcuts_after_ui(&mut self, ctx: &egui::Context, widget_had_focus: bool) {
        let focus = self.current_focus(ctx);
        let canvas_text = focus == Focus::TextField
            && self.canvas_owns_text(ctx)
            && !self.input.is_ime_composing()
            && !crate::canvas::frame_text_input::is_composing();
        if focus != Focus::Canvas && !canvas_text {
            return;
        }
        for (key, egui_modifiers) in pressed_keys(ctx) {
            // A text field can release focus when Enter commits its value.
            if key == egui::Key::Enter && widget_had_focus && !canvas_text {
                continue;
            }
            if key == egui::Key::Enter && egui_modifiers == egui::Modifiers::NONE && !canvas_text
                && self.workspace.session().is_some_and(|session| {
                    crate::canvas_input::tool_kind(&session.active_tool) == crate::canvas_input::ToolKind::Move
                }) && self.open_move_duplicate(ctx)
            {
                ctx.input_mut(|input| input.consume_key(egui_modifiers, key));
                continue;
            }
            let Some(name) = key_name(key) else { continue };
            let modifiers = modifiers_from(egui_modifiers);
            if canvas_text {
                let Some(ShortcutTarget::Run { command: command @ ("edit.undo" | "edit.redo" | "selection.all"), .. }) =
                    resolve(&self.shortcuts, Focus::Canvas, &name, modifiers)
                else {
                    continue;
                };
                if command == "selection.all" {
                    self.select_all();
                } else {
                    self.run_history_shortcut(command);
                }
            } else if self.handle_shortcut_key(focus, &name, modifiers).is_none() {
                continue;
            }
            ctx.input_mut(|input| input.consume_key(egui_modifiers, key));
        }
    }
}
