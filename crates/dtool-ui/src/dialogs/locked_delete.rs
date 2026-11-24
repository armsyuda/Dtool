use dtool_core::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use dtool_strings::text;
use serde_json::{json, Value};

pub const NODE_DELETE: &str = "node.delete";
pub const LOCKED_NEEDS_CONFIRM: &str = "LOCKED_NEEDS_CONFIRM";
pub const TEXT_KEY: &str = "dialog.locked_delete.text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockedDeleteAnswer {
    Yes,
    No,
}

#[derive(Debug, Clone, Default)]
pub struct LockedDeleteDialog {
    pub open: bool,
    pub nodes: Vec<u64>,
    pub answer: Option<LockedDeleteAnswer>,
    pub owner: Option<dtool_core::workspace::ContextId>,
}

impl LockedDeleteDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open_for(&mut self, nodes: Vec<u64>) {
        self.nodes = nodes;
        self.answer = None;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.owner = None;
        self.nodes.clear();
    }

    pub fn needs_confirm(result: &CommandResult) -> bool {
        matches!(result, Err(err) if Self::is_locked_error(err))
    }

    pub fn is_locked_error(err: &CommandError) -> bool {
        err.code == LOCKED_NEEDS_CONFIRM
    }

    pub fn delete_command(nodes: &[u64], confirm_locked: bool) -> Command {
        let list: Vec<Value> = nodes.iter().map(|id| json!(id)).collect();
        Command::with_params(NODE_DELETE, json!({ "nodes": list, "confirm_locked": confirm_locked }))
    }

    pub fn selected_nodes(session: &Session) -> Vec<u64> {
        session.selection.ids.iter().map(|id| id.0).collect()
    }

    pub fn request_delete(&mut self, session: &mut Session, nodes: Vec<u64>) -> CommandResult {
        let result = session.run(&Self::delete_command(&nodes, false));
        if Self::needs_confirm(&result) {
            self.open_for(nodes);
        }
        result
    }

    pub fn request_delete_selection(&mut self, session: &mut Session) -> CommandResult {
        let nodes = Self::selected_nodes(session);
        self.request_delete(session, nodes)
    }

    pub fn resolve(&mut self, session: &mut Session, answer: LockedDeleteAnswer) -> Option<CommandResult> {
        if !self.open {
            return None;
        }
        self.answer = Some(answer);
        let nodes = std::mem::take(&mut self.nodes);
        self.open = false;
        match answer {
            LockedDeleteAnswer::Yes => Some(session.run(&Self::delete_command(&nodes, true))),
            LockedDeleteAnswer::No => None,
        }
    }

    pub fn message(&self) -> String {
        text(TEXT_KEY).to_string()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) -> Option<CommandResult> {
        if !self.open {
            return None;
        }
        let mut picked: Option<LockedDeleteAnswer> = None;
        let message = self.message();
        egui::Window::new(message.clone())
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                dtool_design::dialog_frame(ui, |ui| {
                    ui.label(message.as_str());
                    ui.add_space(dtool_design::space("md"));
                    ui.horizontal(|ui| {
                        if dtool_design::button(ui, text("button.yes")).clicked() {
                            picked = Some(LockedDeleteAnswer::Yes);
                        }
                        if dtool_design::button(ui, text("button.no")).clicked() {
                            picked = Some(LockedDeleteAnswer::No);
                        }
                    });
                    match dtool_design::dialog_keys(ui) {
                        Some(dtool_design::DialogKey::Cancel) => picked = Some(LockedDeleteAnswer::No),
                        Some(dtool_design::DialogKey::Confirm) => picked = Some(LockedDeleteAnswer::Yes),
                        None => {}
                    }
                });
            });
        match picked {
            Some(answer) => self.resolve(session, answer),
            None => None,
        }
    }
}
