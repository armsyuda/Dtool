use dtool_protocol::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn next_position() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct Change {
    pub name: String,
    pub redo: Command,
    pub undo: Command,
}

#[derive(Debug, Clone)]
struct Step {
    id: u64,
    change: Change,
    before: u64,
    after: u64,
}

#[derive(Debug, Clone)]
pub struct UndoStack {
    undo_stack: Vec<Step>,
    redo_stack: Vec<Step>,
    limit: usize,
    position: u64,
    revision: u64,
}

impl Default for UndoStack {
    fn default() -> Self {
        let position = next_position();
        Self { undo_stack: Vec::new(), redo_stack: Vec::new(), limit: usize::MAX, position, revision: position }
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(limit: usize) -> Self {
        Self { limit, ..Self::default() }
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        if self.undo_stack.len() > limit {
            let excess = self.undo_stack.len() - limit;
            self.undo_stack.drain(0..excess);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub(crate) fn next_step(&self, redo: bool) -> Option<(&str, u64)> {
        let step = if redo { self.redo_stack.last()? } else { self.undo_stack.last()? };
        Some((&step.change.name, step.after))
    }

    pub(crate) fn next_action_is(&self, redo: bool, command: &str) -> bool {
        let step = if redo { self.redo_stack.last() } else { self.undo_stack.last() };
        step.is_some_and(|step| {
            let action = if redo { &step.change.redo } else { &step.change.undo };
            action.name == command
        })
    }

    pub fn push(&mut self, change: Change) {
        let before = self.position;
        let selection_only = change.undo.name == "selection.set"
            && matches!(change.redo.name.as_str(), "selection.set" | "selection.marquee"
                | "selection.hit" | "selection.enter" | "selection.exit" | "selection.cycle");
        if !selection_only {
            self.position = next_position();
            self.revision = self.position;
        }
        let id = if selection_only { next_position() } else { self.position };
        self.undo_stack.push(Step { id, change, before, after: self.position });
        if self.undo_stack.len() > self.limit {
            let excess = self.undo_stack.len() - self.limit;
            self.undo_stack.drain(0..excess);
        }
        self.redo_stack.clear();
    }

    pub(crate) fn replace_last_commands(&mut self, undo: Command, redo: Command) {
        if let Some(step) = self.undo_stack.last_mut() {
            step.change.undo = undo;
            step.change.redo = redo;
        }
    }

    pub fn pop_last(&mut self) -> Option<Change> {
        let step = self.undo_stack.pop()?;
        self.move_to(step.before);
        Some(step.change)
    }

    pub fn undo(&mut self) -> Option<Command> {
        if let Some(step) = self.undo_stack.pop() {
            self.move_to(step.before);
            let command = step.change.undo.clone();
            self.redo_stack.push(step);
            return Some(command);
        }
        None
    }

    pub fn redo(&mut self) -> Option<Command> {
        if let Some(step) = self.redo_stack.pop() {
            self.move_to(step.after);
            let command = step.change.redo.clone();
            self.undo_stack.push(step);
            if self.undo_stack.len() > self.limit {
                let excess = self.undo_stack.len() - self.limit;
                self.undo_stack.drain(0..excess);
            }
            return Some(command);
        }
        None
    }

    fn move_to(&mut self, position: u64) {
        if self.position != position {
            self.position = position;
            self.revision = next_position();
        }
    }

    pub fn is_delta_kept(&self, since: u64) -> bool {
        let _ = since;
        true
    }

    pub fn get_changes_since(&self, since: u64) -> Vec<dtool_protocol::ChangeName> {
        let _ = since;
        Vec::new()
    }
}

impl UndoStack {
    pub fn chronological_changes(&self) -> impl Iterator<Item = &Change> {
        self.undo_stack.iter().chain(self.redo_stack.iter().rev()).map(|step| &step.change)
    }

    pub(crate) fn commands_to_position(&self, target: usize) -> impl Iterator<Item = &Command> {
        let applied = self.undo_stack.len();
        self.undo_stack.iter().rev().take(applied.saturating_sub(target)).map(|step| &step.change.undo)
            .chain(self.redo_stack.iter().rev().take(target.saturating_sub(applied)).map(|step| &step.change.redo))
    }

    pub fn history_step_id(&self, index: u64) -> Option<u64> {
        let index = usize::try_from(index).ok()?;
        if index < self.undo_stack.len() {
            return Some(self.undo_stack[index].id);
        }
        let redo_offset = index - self.undo_stack.len();
        let redo_index = self.redo_stack.len().checked_sub(redo_offset.checked_add(1)?)?;
        Some(self.redo_stack[redo_index].id)
    }

    pub(crate) fn position(&self) -> u64 { self.position }
    pub(crate) fn revision(&self) -> u64 { self.revision }

    pub fn applied_steps(&self) -> usize { self.undo_stack.len() }

    pub fn limit(&self) -> usize { self.limit }

    pub fn total_steps(&self) -> usize { self.undo_stack.len() + self.redo_stack.len() }
}
