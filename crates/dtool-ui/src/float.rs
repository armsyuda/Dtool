use std::collections::HashSet;
use dtool_core::{ContextId, Workspace};
use egui::{ViewportId, ViewportBuilder};

pub const FLOAT_MENU_ID: &str = "window.arrange.float";
pub const FLOAT_ALL_MENU_ID: &str = "window.arrange.float_all";
pub const DOCK_MENU_ID: &str = "window.arrange.dock";
pub const DOCK_ALL_MENU_ID: &str = "window.arrange.dock_all";

pub fn handles(action: &str) -> bool {
    matches!(action, FLOAT_MENU_ID | FLOAT_ALL_MENU_ID | DOCK_MENU_ID | DOCK_ALL_MENU_ID)
}

pub fn should_dock(close_requested: bool, escape: bool, dock_clicked: bool) -> bool {
    close_requested || escape || dock_clicked
}

pub struct FloatManager {
    pub floating: HashSet<ContextId>,
}

impl Default for FloatManager {
    fn default() -> Self {
        Self {
            floating: HashSet::new(),
        }
    }
}

impl FloatManager {
    pub fn is_floating(&self, id: ContextId) -> bool {
        self.floating.contains(&id)
    }

    pub fn float(&mut self, id: ContextId) {
        self.floating.insert(id);
    }

    pub fn dock(&mut self, id: ContextId) {
        self.floating.remove(&id);
    }

    pub fn float_all(&mut self, ids: &[ContextId]) {
        for &id in ids {
            self.floating.insert(id);
        }
    }

    pub fn dock_all(&mut self) {
        self.floating.clear();
    }

    pub fn toggle_float(&mut self, id: ContextId) {
        if !self.floating.remove(&id) {
            self.floating.insert(id);
        }
    }

    pub fn arrange(&mut self, action: &str, workspace: &Workspace) -> bool {
        let known = |id: ContextId| workspace.get(id).is_some();
        match action {
            FLOAT_MENU_ID => match workspace.active().filter(|id| known(*id)) {
                Some(id) => {
                    self.float(id);
                    true
                }
                None => false,
            },
            FLOAT_ALL_MENU_ID => {
                let ids: Vec<ContextId> = workspace.contexts().iter().map(|context| context.id).collect();
                self.float_all(&ids);
                !ids.is_empty()
            }
            DOCK_MENU_ID => {
                let target = workspace.active().filter(|id| self.is_floating(*id)).or_else(|| (self.floating.len() == 1).then(|| self.floating.iter().copied().next()).flatten());
                match target {
                    Some(id) => {
                        self.dock(id);
                        true
                    }
                    None => false,
                }
            }
            DOCK_ALL_MENU_ID => {
                let any = !self.floating.is_empty();
                self.dock_all();
                any
            }
            _ => false,
        }
    }

    pub fn show_viewport(
        &self,
        ctx: &egui::Context,
        id: ContextId,
        title: &str,
        size: [f32; 2],
        content_fn: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        let viewport_id = ViewportId::from_hash_of(id);
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([size[0], size[1] + 30.0]);

        let mut content_fn = Some(content_fn);
        ctx.show_viewport_immediate(viewport_id, builder, move |ui, _class| {
            let (close_requested, escape) = ui.input(|input| (input.viewport().close_requested(), input.key_pressed(egui::Key::Escape)));
            let mut dock_clicked = false;
            if let Some(content_fn) = content_fn.take() {
                egui::CentralPanel::default().show(ui, |ui| {
                    dock_clicked = dtool_design::button(ui, dtool_strings::text("tab.menu.dock")).clicked();
                    content_fn(ui);
                });
            }
            !should_dock(close_requested, escape, dock_clicked)
        })
    }
}