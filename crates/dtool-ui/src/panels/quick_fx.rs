use dtool_core::workspace::{Scope, Workspace};
use dtool_core::blend::BlendMode;
use dtool_core::layer_style::{EffectKind, EffectParams, LayerEffect};
use dtool_core::paint::{Colour, Paint, paint_from_legacy_str};
use dtool_core::{Command, CommandError};
use dtool_design::{ControlState, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use serde_json::{json, Value};

#[derive(Default)]
pub struct QuickFxPanel {
    pub last_error: Option<String>,
    selected_kind: Option<&'static str>,
    icons: crate::icon_cache::IconCache,
}

fn common_number(rows: &[Option<&LayerEffect>], read: impl Fn(&LayerEffect) -> Option<f64>) -> NumericValue {
    let Some(first) = rows.first().and_then(|row| row.and_then(&read)) else {
        return NumericValue::Mixed;
    };
    if rows.iter().all(|row| row.and_then(&read) == Some(first)) {
        NumericValue::Value(first)
    } else {
        NumericValue::Mixed
    }
}

fn effect_size(effect: &LayerEffect) -> Option<(&'static str, f64)> {
    match &effect.params {
        EffectParams::Blur { radius, .. } | EffectParams::Shadow { radius, .. }
        | EffectParams::Glow { radius, .. } | EffectParams::Light { radius, .. } => Some(("radius", f64::from(*radius))),
        EffectParams::Outline { width, .. } => Some(("width", f64::from(*width))),
        EffectParams::Overlay { .. } => None,
    }
}

impl QuickFxPanel {
    fn submit(&mut self, workspace: &mut Workspace, commands: Vec<Command>) -> Result<Value, CommandError> {
        let result = if commands.is_empty() { Ok(json!({"changes": []})) } else {
            workspace.run(Scope::active(), Command::with_params("edit.batch", json!({"commands": commands}))).1
        };
        self.last_error = result.as_ref().err().map(|e| e.message_key.clone());
        result
    }

    pub fn set_enabled(&mut self, workspace: &mut Workspace, kind: &str, enabled: bool) -> Result<Value, CommandError> {
        let parsed: EffectKind = serde_json::from_value(json!(kind)).map_err(|_| CommandError::invalid_params())?;
        let session = workspace.session().ok_or_else(CommandError::no_document)?;
        if session.selection.ids.is_empty() { return Err(CommandError::new("NO_SELECTION")); }
        let mut commands = Vec::new();
        for id in &session.selection.ids {
            let object = session.document.get_object(*id).ok_or_else(CommandError::not_found)?;
            if let Some(index) = object.effects.iter().position(|effect| effect.kind == parsed) {
                if object.effects[index].enabled != enabled { commands.push(Command::with_params("effect.update", json!({"id": id.0, "index": index, "enabled": enabled}))); }
            } else if enabled {
                commands.push(Command::with_params("effect.add", json!({"id": id.0, "kind": kind})));
            }
        }
        self.submit(workspace, commands)
    }

    pub fn set_numeric(&mut self, workspace: &mut Workspace, kind: &str, field: &str, value: f64) -> Result<Value, CommandError> {
        if !value.is_finite() || !matches!(field, "opacity" | "radius" | "width" | "offset" | "angle" | "intensity") {
            return Err(CommandError::invalid_params());
        }
        self.update_effects(workspace, kind, |effect| {
            if field == "opacity" {
                return Ok(json!({ "opacity": value }));
            }
            let mut params = serde_json::to_value(&effect.params).map_err(|_| CommandError::invalid_params())?;
            let data = params.as_object_mut().and_then(|m| m.values_mut().next())
                .and_then(|body| body.get_mut(field)).ok_or_else(CommandError::invalid_params)?;
            *data = json!(value);
            Ok(json!({ "params": params }))
        })
    }

    fn update_effects(&mut self, workspace: &mut Workspace, kind: &str, patch: impl Fn(&LayerEffect) -> Result<Value, CommandError>) -> Result<Value, CommandError> {
        let parsed: EffectKind = serde_json::from_value(json!(kind)).map_err(|_| CommandError::invalid_params())?;
        let session = workspace.session().ok_or_else(CommandError::no_document)?;
        let mut commands = Vec::new();
        for id in &session.selection.ids {
            let object = session.document.get_object(*id).ok_or_else(CommandError::not_found)?;
            let Some((index, effect)) = object.effects.iter().enumerate().find(|(_, effect)| effect.kind == parsed) else { continue };
            let mut params = patch(effect)?;
            params["id"] = json!(id.0);
            params["index"] = json!(index);
            commands.push(Command::with_params("effect.update", params));
        }
        self.submit(workspace, commands)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let effects: Vec<_> = workspace.session().map(|s| s.selection.ids.iter()
            .filter_map(|id| s.document.get_object(*id).map(|o| o.effects.as_slice())).collect()).unwrap_or_default();
        let selection = workspace.session().map(|s| (s.document.instance.0, s.selection.ids.as_slice()));
        let mut toggle = None;
        let mut numeric = None;
        let mut blend = None;
        let mut colour = None;
        dtool_design::panel_frame(ui, |ui| {
            dtool_design::panel_header(ui, text("menu.window.vector.quick_fx"));
            if effects.is_empty() { ui.label(text("fx.no_selection")); return; }
            let choices: Vec<_> = crate::dialogs::layer_effects::EFFECT_KINDS.iter().map(|&(kind, key)| {
                let parsed: EffectKind = serde_json::from_value(json!(kind)).expect("known effect kind");
                let rows: Vec<_> = effects.iter().map(|list| list.iter().find(|fx| fx.kind == parsed)).collect();
                let all = rows.iter().all(|row| row.is_some_and(|fx| fx.enabled));
                let any = rows.iter().any(|row| row.is_some_and(|fx| fx.enabled));
                (kind, key, rows, all, any)
            }).collect();
            let selected = choices.iter().find(|(kind, _, _, all, _)| *all && Some(*kind) == self.selected_kind)
                .or_else(|| choices.iter().find(|(_, _, _, all, _)| *all));
            self.selected_kind = selected.map(|(kind, _, _, _, _)| *kind);
            let compact = visible_width(ui) < 190.0;
            let mut draw_choices = |ui: &mut egui::Ui| {
                for (kind, key, _, all, any) in &choices {
                    let mut enabled = *all;
                    ui.horizontal(|ui| {
                        let checkbox = dtool_design::toggle_with(ui, &mut enabled, "", &ControlState::default().mixed(*any && !*all));
                        let label = text(key);
                        let row = ui.add(egui::Button::selectable(self.selected_kind == Some(*kind), label).truncate())
                            .on_hover_text(label);
                        if checkbox.labelled_by(row.id).clicked() {
                            toggle = Some((*kind, enabled));
                            self.selected_kind = enabled.then_some(*kind);
                            if compact { ui.close(); }
                        } else if row.clicked() && *all {
                            self.selected_kind = Some(*kind);
                            if compact { ui.close(); }
                        }
                    });
                }
            };
            if compact {
                let label = text("fx.effect_list");
                ui.horizontal(|ui| {
                    let button = self.icons.studio_button(ui, "layer-effects", label, false);
                    egui::Popup::menu(&button).show(|ui| {
                        ui.set_min_width(166.0);
                        draw_choices(ui);
                    });
                    let caption = selected.map_or(label, |(_, key, _, _, _)| text(key));
                    ui.add(egui::Label::new(caption).truncate()).on_hover_text(caption);
                });
            } else {
                draw_choices(ui);
            }
            if let Some((kind, key, rows, _, _)) = selected {
                ui.separator();
                if !compact { ui.strong(text(key)); }
                let first = rows[0].expect("enabled effect on each selected object");
                if let Some(mode) = blend_row(ui, rows, first.blend) {
                    blend = Some((*kind, mode));
                }
                if let Some(first_colour) = effect_colour(&first.params) {
                    let applied = ui.push_id((kind, selection), |ui| colour_row(ui, rows, *first_colour)).inner;
                    if let Some(value) = applied {
                        colour = Some((*kind, value));
                    }
                }
                let opacity = common_number(rows, |fx| Some(f64::from(fx.opacity) * 100.0));
                let spec = NumericSpec::new("").range(0.0..=100.0).width(60.0).suffix("%").mixed_hint(text("ctx.text.mixed"));
                if let Some(value) = detail_number(ui, kind, "fx.opacity", opacity, spec) {
                    numeric = Some((*kind, "opacity", value / 100.0));
                }
                if let Some((field, _)) = effect_size(first) {
                    let value = common_number(rows, |fx| effect_size(fx).map(|(_, value)| value));
                    let caption = if field == "width" { "fx.width" } else { "fx.radius" };
                    let spec = NumericSpec::new("").range(0.0..=10000.0).width(60.0).mixed_hint(text("ctx.text.mixed"));
                    if let Some(value) = detail_number(ui, kind, caption, value, spec) {
                        numeric = Some((*kind, field, value));
                    }
                }
                if matches!(first.params, EffectParams::Shadow { .. }) {
                    for (field, caption, range, suffix, scale) in [
                        ("offset", "fx.offset", -100000.0..=100000.0, "", 1.0),
                        ("angle", "fx.light_direction", 0.0..=360.0, "°", 1.0),
                        ("intensity", "fx.intensity", 0.0..=100.0, "%", 100.0),
                    ] {
                        let value = common_number(rows, |fx| {
                            let EffectParams::Shadow { offset, angle, intensity, .. } = &fx.params else { return None };
                            let value = match field {
                                "offset" => *offset,
                                "angle" => *angle,
                                _ => *intensity,
                            };
                            Some(f64::from(value) * scale)
                        });
                        let spec = NumericSpec::new("").range(range).width(60.0).suffix(suffix).mixed_hint(text("ctx.text.mixed"));
                        if let Some(value) = detail_number(ui, kind, caption, value, spec) {
                            numeric = Some((*kind, field, value / scale));
                        }
                    }
                }
            }
            if let Some(key) = &self.last_error { ui.colored_label(ui.visuals().error_fg_color, text(key)); }
        });
        if let Some((kind, enabled)) = toggle { let _ = self.set_enabled(workspace, kind, enabled); }
        if let Some((kind, field, value)) = numeric { let _ = self.set_numeric(workspace, kind, field, value); }
        if let Some((kind, mode)) = blend {
            let _ = self.update_effects(workspace, kind, |_| Ok(json!({ "blend": mode })));
        }
        if let Some((kind, value)) = colour {
            let _ = self.update_effects(workspace, kind, |effect| {
                let mut params = effect.params.clone();
                match &mut params {
                    EffectParams::Shadow { colour, .. } | EffectParams::Glow { colour, .. } => *colour = value,
                    _ => return Err(CommandError::invalid_params()),
                }
                Ok(json!({ "params": params }))
            });
        }
    }
}

fn effect_colour(params: &EffectParams) -> Option<&Colour> {
    match params {
        EffectParams::Shadow { colour, .. } | EffectParams::Glow { colour, .. } => Some(colour),
        _ => None,
    }
}

fn colour_row(ui: &mut egui::Ui, rows: &[Option<&LayerEffect>], first: Colour) -> Option<Colour> {
    use super::colour::{hex_of, picker_hsva};
    let mixed = rows.iter().flatten().any(|fx| effect_colour(&fx.params) != Some(&first));
    let to_gui = |colour: Colour| egui::Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a);
    let value = if mixed { dtool_design::WellValue::Mixed } else { dtool_design::WellValue::Colour(to_gui(first)) };
    dtool_design::studio_row(ui, text("fx.colour"), |ui, label| {
        let tip = if mixed { text("ctx.text.mixed").to_owned() } else { hex_of(&first) };
        let well = dtool_design::colour_well_with(ui, value, "quick_fx.colour", &ControlState::default())
            .labelled_by(label).on_hover_text(tip);
        let draft_id = well.id.with("draft");
        let picker_id = well.id.with("picker");
        if well.clicked() {
            let draft = if mixed { String::new() } else { hex_of(&first) };
            ui.data_mut(|data| {
                data.insert_temp(draft_id, draft);
                data.remove::<egui::ecolor::Hsva>(picker_id);
            });
        }
        let mut applied = None;
        egui::Popup::menu(&well).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let mut draft = ui.data_mut(|data| data.get_temp::<String>(draft_id)).unwrap_or_default();
            let parse = |value: &str| {
                let digits = value.trim().trim_start_matches('#');
                if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) { return None; }
                match paint_from_legacy_str(value)? {
                    Paint::Solid { colour } => Some(colour),
                    _ => None,
                }
            };
            let input = parse(&draft).unwrap_or(first);
            let channels = [input.r, input.g, input.b, input.a];
            let mut picked = ui.data_mut(|data| data.get_temp::<egui::ecolor::Hsva>(picker_id))
                .filter(|hsva| hsva.to_srgba_unmultiplied() == channels)
                .unwrap_or_else(|| picker_hsva(input));
            if egui::color_picker::color_picker_hsva_2d(ui, &mut picked, egui::color_picker::Alpha::OnlyBlend) {
                let next = picked.to_srgba_unmultiplied();
                if next != channels {
                    let [r, g, b, a] = next;
                    draft = hex_of(&Colour::new(r, g, b, a));
                }
            }
            ui.horizontal(|ui| {
                let label = ui.label("HEX");
                ui.add(egui::TextEdit::singleline(&mut draft).desired_width(100.0)).labelled_by(label.id);
            });
            let next = parse(&draft).filter(|colour| mixed || hex_of(colour) != hex_of(&first));
            ui.horizontal(|ui| {
                if ui.add_enabled_ui(next.is_some(), |ui| dtool_design::button(ui, text("button.apply"))).inner.clicked() {
                    applied = next;
                    ui.close();
                }
                if dtool_design::button(ui, text("button.cancel")).clicked() {
                    ui.close();
                }
            });
            ui.data_mut(|data| {
                data.insert_temp(draft_id, draft);
                data.insert_temp(picker_id, picked);
            });
        });
        applied
    })
}

fn visible_width(ui: &egui::Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x).max(0.0)
}

fn blend_row(ui: &mut egui::Ui, rows: &[Option<&LayerEffect>], mode: BlendMode) -> Option<BlendMode> {
    let mixed = rows.iter().flatten().any(|fx| fx.blend != mode);
    let options: Vec<_> = BlendMode::ALL.into_iter().filter(|mode| *mode != BlendMode::PassThrough)
        .map(|mode| (mode, crate::dialogs::blend_options::mode_label(&format!("{mode:?}")))).collect();
    let label = text("layers.blend");
    let shown = if mixed { dtool_design::MIXED_TEXT.to_string() } else {
        options.iter().find(|(candidate, _)| *candidate == mode).map(|(_, label)| label.clone()).unwrap_or_default()
    };
    let combo = |ui: &mut egui::Ui, caption: egui::Id| {
        let response = egui::ComboBox::from_id_salt("quick_fx.blend")
            .selected_text(&shown).truncate().width(visible_width(ui).clamp(32.0, 152.0))
            .show_ui(ui, |ui| {
                let mut picked = None;
                for (choice, name) in &options {
                    if ui.selectable_label(!mixed && *choice == mode, name).clicked() {
                        picked = Some(*choice);
                        ui.close();
                    }
                }
                picked
            });
        response.response.labelled_by(caption).on_hover_text(&shown);
        response.inner.flatten()
    };
    if visible_width(ui) < 190.0 {
        let caption = ui.label(label);
        combo(ui, caption.id)
    } else {
        dtool_design::studio_row(ui, label, combo)
    }
    .filter(|choice| mixed || *choice != mode)
}

fn detail_number(ui: &mut egui::Ui, kind: &str, key: &'static str, value: NumericValue, mut spec: NumericSpec) -> Option<f64> {
    let width = visible_width(ui);
    if width < 190.0 {
        let label_width = (width * 0.49).min(72.0);
        spec.label = text(key);
        spec = spec.label_width(label_width).width((width - label_width - ui.spacing().item_spacing.x).clamp(30.0, 60.0));
        let id = ui.make_persistent_id(("quick_fx", kind, key));
        let (_, event) = dtool_design::numeric_field(ui, id, value, &spec);
        if let NumericEvent::Commit(value) = event { Some(value) } else { None }
    } else {
        dtool_design::studio_row(ui, text(key), |ui, label| {
            let id = ui.make_persistent_id(("quick_fx", kind, key));
            let (response, event) = dtool_design::numeric_field(ui, id, value, &spec);
            response.labelled_by(label);
            if let NumericEvent::Commit(value) = event { Some(value) } else { None }
        })
    }
}
