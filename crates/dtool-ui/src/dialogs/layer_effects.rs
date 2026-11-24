use dtool_core::command::Session;
use dtool_core::layer_style::{default_effect_for, EffectKind, LayerEffect};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

pub const OUTER_SHADOW: &str = "OuterShadow";

/// Effect kinds the dialog offers, in panel order, with their label keys (B50-T002).
pub const EFFECT_KINDS: [(&str, &str); 10] = [
    (OUTER_SHADOW, "layer.effects.outer_shadow"),
    ("InnerShadow", "layer.effects.inner_shadow"),
    ("OuterGlow", "layer.effects.outer_glow"),
    ("InnerGlow", "layer.effects.inner_glow"),
    ("Outline", "layer.effects.outline"),
    ("ThreeD", "layer.effects.three_d"),
    ("BevelEmboss", "layer.effects.bevel_emboss"),
    ("ColourOverlay", "layer.effects.colour_overlay"),
    ("GradientOverlay", "layer.effects.gradient_overlay"),
    ("GaussianBlur", "layer.effects.gaussian_blur"),
];

mod editor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EffectRow {
    Instance(usize),
    Available(EffectKind),
}

fn effect_kind(name: &str) -> Option<EffectKind> {
    serde_json::from_value(json!(name)).ok()
}

fn effect_label(kind: EffectKind) -> &'static str {
    EFFECT_KINDS.iter().find(|(name, _)| effect_kind(name) == Some(kind))
        .map(|(_, key)| text(key)).unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
pub struct LayerEffectsDialog {
    pub open: bool,
    pub object_id: Option<u64>,
    pub outer_shadow: bool,
    pub enabled: Vec<&'static str>,
    pub last_error: Option<String>,
    selected: Option<EffectRow>,
    original: Vec<LayerEffect>,
    draft: Vec<LayerEffect>,
    generation: Option<u64>,
    initialized: bool,
}

pub fn preview_command(id: u64, kind: &str) -> Command {
    Command::with_params("effect.preview", json!({ "id": id, "kind": kind }))
}

pub fn cancel_command(id: u64) -> Command {
    Command::with_params("effect.cancel", json!({ "id": id }))
}

pub fn add_command(id: u64, kind: &str) -> Command {
    Command::with_params("effect.add", json!({ "id": id, "kind": kind, "params": {} }))
}

impl LayerEffectsDialog {
    pub fn open_for(&mut self, id: u64) {
        *self = Self::default(); self.open = true; self.object_id = Some(id);
    }

    pub fn is_enabled(&self, kind: &str) -> bool { self.enabled.contains(&kind) }

    fn load(&mut self, session: &Session) -> bool {
        if self.initialized { return true; }
        let Some(object) = self.object_id.and_then(|id| session.document.get_object(dtool_core::ObjectId(id))) else { self.last_error = Some("error.not_found".into()); return false };
        self.original = object.effects.clone(); self.generation = Some(session.generation);
        self.draft = self.original.clone();
        self.sync_enabled();
        self.initialized = true;
        true
    }

    fn clear_preview(&self, session: &mut Session) {
        if let Some(id) = self.object_id {
            let _ = session.run(&cancel_command(id));
            let keep = dtool_core::commands::layer_style::snapshot_staged(session).into_iter().filter(|item| item.object.0 != id).collect();
            dtool_core::commands::layer_style::restore_staged(session, keep);
        }
    }

    fn sync_enabled(&mut self) {
        self.enabled = EFFECT_KINDS.iter().filter_map(|(name, _)| {
            let kind = effect_kind(name)?;
            self.draft.iter().any(|effect| effect.kind == kind && effect.enabled).then_some(*name)
        }).collect();
        self.outer_shadow = self.is_enabled(OUTER_SHADOW);
    }

    fn preview(&mut self, session: &mut Session) -> bool {
        self.sync_enabled();
        if self.generation != Some(session.generation) { self.last_error = Some("error.stale".into()); return false; }
        let Some(id) = self.object_id else { return false };
        let result = session.run(&Command::with_params("effect.preview", json!({"id": id, "effects": self.draft})));
        self.last_error = result.as_ref().err().map(|e| e.message_key.clone()); result.is_ok()
    }

    pub fn set_enabled(&mut self, session: &mut Session, kind: &str, enabled: bool) {
        let Some(kind) = effect_kind(kind) else { return };
        if !self.load(session) { return; }
        if enabled && !self.draft.iter().any(|effect| effect.kind == kind) {
            let size = self.object_id.and_then(|id| session.document.get_object(dtool_core::ObjectId(id))).map_or([1.0, 1.0], |object| object.size);
            self.draft.push(default_effect_for(kind, size));
        } else if !enabled && !self.original.iter().any(|effect| effect.kind == kind) {
            self.draft.retain(|effect| effect.kind != kind);
        } else {
            for effect in self.draft.iter_mut().filter(|effect| effect.kind == kind) {
                effect.enabled = enabled;
            }
        }
        self.preview(session);
    }

    pub fn set_effect(&mut self, session: &mut Session, effect: dtool_core::layer_style::LayerEffect) -> bool {
        if !effect.is_valid() || !self.load(session) { self.last_error = Some("error.invalid_params".into()); return false; }
        if let Some(index) = self.draft.iter().position(|old| old.kind == effect.kind) {
            for old in self.draft.iter_mut().filter(|old| old.kind == effect.kind) { old.enabled = effect.enabled; }
            self.draft[index] = effect;
        } else if effect.enabled {
            self.draft.push(effect);
        }
        self.preview(session)
    }

    pub fn enable_outer_shadow(&mut self, session: &mut Session) { self.set_enabled(session, OUTER_SHADOW, true); }

    pub fn cancel(&mut self, session: &mut Session) { self.clear_preview(session); *self = Self::default(); }

    pub fn ok(&mut self, session: &mut Session) -> bool {
        if !self.load(session) { return false; }
        if self.generation != Some(session.generation) { self.clear_preview(session); self.last_error = Some("error.stale".into()); return false; }
        let Some(id) = self.object_id else { return false };
        let shared = self.original.iter().zip(&self.draft).take_while(|(old, new)| old.kind == new.kind).count();
        let mut commands = Vec::new();
        for (index, (old, effect)) in self.original.iter().zip(&self.draft).take(shared).enumerate() {
            if old != effect {
                let mut params = serde_json::to_value(effect).expect("effect serializes");
                params["id"] = json!(id);
                params["index"] = json!(index);
                commands.push(Command::with_params("effect.update", params));
            }
        }
        for index in (shared..self.original.len()).rev() {
            commands.push(Command::with_params("effect.remove", json!({"id": id, "index": index})));
        }
        for effect in &self.draft[shared..] {
            commands.push(Command::with_params("effect.add", json!({"id": id, "effect": effect})));
        }
        self.clear_preview(session);
        let result = match commands.len() {
            0 => Ok(json!({"changes": []})),
            1 => session.run(&commands[0]),
            _ => session.run(&Command::with_params("edit.batch", json!({"commands": commands}))),
        };
        self.last_error = result.as_ref().err().map(|e| e.message_key.clone());
        if result.is_ok() { *self = Self::default(); }
        result.is_ok()
    }

    fn show_list(&mut self, ui: &mut egui::Ui, selected: &mut EffectRow, size: [f32; 2], height: f32) -> bool {
        let mut changed = false;
        egui::ScrollArea::vertical().id_salt("effects-list").max_height((height - 26.0).max(60.0))
            .auto_shrink([false, false]).show(ui, |ui| {
            let mut count = std::collections::HashMap::new();
            for effect in &self.draft { *count.entry(effect.kind).or_insert(0usize) += 1; }
            let mut number = std::collections::HashMap::new();
            for (index, effect) in self.draft.iter_mut().enumerate() {
                let ordinal = number.entry(effect.kind).or_insert(0usize);
                *ordinal += 1;
                let name = effect_label(effect.kind);
                let label = if count[&effect.kind] > 1 { format!("{name} {ordinal}") } else { name.to_owned() };
                changed |= effect_row(ui, selected, EffectRow::Instance(index), &label, &mut effect.enabled);
            }
            let missing: Vec<_> = EFFECT_KINDS.iter().filter_map(|(name, _)| effect_kind(name))
                .filter(|kind| !count.contains_key(kind)).collect();
            if !self.draft.is_empty() && !missing.is_empty() { ui.separator(); }
            for kind in missing {
                if effect_row(ui, selected, EffectRow::Available(kind), effect_label(kind), &mut false) {
                    *selected = EffectRow::Instance(self.draft.len());
                    self.draft.push(default_effect_for(kind, size));
                    changed = true;
                }
            }
        });
        let (kind, index) = match *selected {
            EffectRow::Available(kind) => (kind, None),
            EffectRow::Instance(index) => (self.draft[index].kind, Some(index)),
        };
        let range = |other: usize| self.draft[other].kind == kind;
        // The compositor assigns fixed ranks across kinds. Moving a row changes its order within its kind.
        let up = index.and_then(|i| (0..i).rev().find(|&other| range(other)));
        let down = index.and_then(|i| (i + 1..self.draft.len()).find(|&other| range(other)));
        let can_add = index.is_none() || matches!(kind,
            EffectKind::OuterShadow | EffectKind::InnerShadow | EffectKind::Outline |
            EffectKind::ColourOverlay | EffectKind::GradientOverlay);
        ui.horizontal(|ui| {
            if instance_button(ui, "+", "fx.instance.add", can_add) {
                *selected = EffectRow::Instance(self.draft.len());
                self.draft.push(default_effect_for(kind, size));
                changed = true;
            }
            if instance_button(ui, "−", "fx.instance.remove", index.is_some()) {
                self.draft.remove(index.expect("selected instance"));
                *selected = self.draft.iter().position(|effect| effect.kind == kind)
                    .map_or(EffectRow::Available(kind), EffectRow::Instance);
                changed = true;
            }
            for (icon, key, target) in [("↑", "fx.instance.up", up), ("↓", "fx.instance.down", down)] {
                if instance_button(ui, icon, key, target.is_some()) {
                    let target = target.expect("enabled move");
                    self.draft.swap(index.expect("selected instance"), target);
                    *selected = EffectRow::Instance(target);
                    changed = true;
                }
            }
        });
        changed
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        use crate::dialog_chrome::{ClosePolicy, DialogResult};
        use egui::{vec2, Align, Layout, ScrollArea};

        if !self.open || !self.load(session) { return; }
        let size = self.object_id.and_then(|id| session.document.get_object(dtool_core::ObjectId(id))).map(|o| o.size).unwrap_or([1.0, 1.0]);
        let width = (ctx.content_rect().width() - 32.0).clamp(240.0, 640.0);
        let height = (ctx.content_rect().height() - 120.0).clamp(120.0, 356.0);
        let list_width = (width * 0.32).clamp(94.0, 182.0);
        let mut selected = self.selected.filter(|row| match row {
            EffectRow::Instance(index) => *index < self.draft.len(),
            EffectRow::Available(kind) => !self.draft.iter().any(|effect| effect.kind == *kind),
        }).unwrap_or_else(|| self.draft.iter().position(|effect| effect.enabled)
            .or_else(|| (!self.draft.is_empty()).then_some(0))
            .map_or(EffectRow::Available(EffectKind::OuterShadow), EffectRow::Instance));
        let editing_number = ctx.memory(|memory| memory.focused())
            .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        let mut changed = false;
        let (result, footer) = crate::dialog_chrome::show_modal(ctx, "dtool.dialog.layer_effects", text("menu.layer.layer_effects"), ClosePolicy::Free, !editing_number, |ui| {
            ui.set_width(width);
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.set_max_height(height);
                ui.allocate_ui_with_layout(vec2(list_width, height), Layout::top_down(Align::Min), |ui| {
                    changed |= self.show_list(ui, &mut selected, size, height);
                });
                ui.separator();
                let mut unused;
                let (effect, enabled) = match selected {
                    EffectRow::Instance(index) => {
                        let effect = &mut self.draft[index];
                        let enabled = effect.enabled;
                        (effect, enabled)
                    }
                    EffectRow::Available(kind) => {
                        unused = default_effect_for(kind, size);
                        (&mut unused, false)
                    }
                };
                ui.vertical(|ui| {
                    dtool_design::panel_header(ui, effect_label(effect.kind));
                    ui.push_id(selected, |ui| {
                        ScrollArea::vertical().max_height((height - 26.0).max(60.0)).auto_shrink([false, false]).show(ui, |ui| {
                            let before = effect.clone();
                            if ui.add_enabled_ui(enabled, |ui| editor::show_effect(ui, effect, size)).inner {
                                if effect.is_valid() { changed = true; } else { *effect = before; self.last_error = Some("error.invalid_params".into()); }
                            }
                        });
                    });
                });
            });
            if let Some(error) = &self.last_error { ui.colored_label(ui.visuals().error_fg_color, text(error)); }
            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if dtool_design::primary_button(ui, text("dialog.ok"), vec2(72.0, 22.0)).clicked() {
                    DialogResult::Confirm
                } else if dtool_design::button(ui, text("dialog.cancel")).clicked() {
                    DialogResult::Cancel
                } else {
                    DialogResult::Open
                }
            }).inner
        });
        self.selected = Some(selected);
        if changed { self.preview(session); }
        match (footer, result) {
            (DialogResult::Cancel, _) | (_, DialogResult::Cancel) => self.cancel(session),
            (DialogResult::Confirm, _) | (_, DialogResult::Confirm) => { self.ok(session); }
            _ => {}
        }
    }
}

fn effect_row(ui: &mut egui::Ui, selected: &mut EffectRow, id: EffectRow, label: &str, enabled: &mut bool) -> bool {
    ui.push_id(id, |ui| ui.horizontal(|ui| {
        let checkbox = dtool_design::toggle(ui, enabled, "");
        let row = dtool_design::list_row(ui, *selected == id, label);
        let toggled = checkbox.labelled_by(row.id).on_hover_text(label).changed();
        if row.clicked() || toggled { *selected = id; }
        toggled
    }).inner).inner
}

fn instance_button(ui: &mut egui::Ui, glyph: &str, key: &str, enabled: bool) -> bool {
    let label = text(key);
    let response = ui.add_enabled(enabled, egui::Button::new(glyph)).on_hover_text(label).on_disabled_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.clicked()
}
