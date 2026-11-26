use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::brush::{self, BrushDynamics, BrushOp, BrushSettings, BrushTexture, BrushTextureMode, BrushTip, Controller, ControllerKind, MAX_SUB_BRUSHES, ScatterAxis, StrokePoint, SubBrush, SubBrushBlend, SubBrushColorBlend, SubBrushPlacement, TipKind, WetEdgeCurve, WetEdgeScope};
use dtool_core::commands::pixel_presets::{self, EDITOR_QUERY, EDITOR_SET};
use dtool_core::raster_view::RasterViewMut;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::{Command, CommandError};
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::{Value, json};

const CONTROLLERS: [(ControllerKind, &str); 5] = [
    (ControllerKind::None, "brush_editor.controller.none"),
    (ControllerKind::Pressure, "brush_editor.controller.pressure"),
    (ControllerKind::Tilt, "brush_editor.controller.tilt"),
    (ControllerKind::Velocity, "brush_editor.controller.velocity"),
    (ControllerKind::Random, "brush_editor.controller.random"),
];

const CHANNELS: [&str; 5] = [
    "context.brush.width",
    "context.brush.opacity",
    "context.brush.flow",
    "context.brush.hardness",
    "brush_editor.rotation",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Dynamics,
    Texture,
    SubBrushes,
    WetEdges,
}

struct CurveDrag {
    index: usize,
    origin: Pos2,
    moved: bool,
}

pub(crate) struct Editor {
    owner: ContextId,
    tool: String,
    preset: Option<String>,
    activate_tool: bool,
    tip: BrushTip,
    tab: Tab,
    channel: usize,
    sub_index: usize,
    sub_search: String,
    dragged: Option<CurveDrag>,
    inherited_wet_edges: bool,
    inactive_wet_curve: Option<WetEdgeCurve>,
    preview: Option<(BrushTip, egui::TextureHandle)>,
    error: Option<String>,
}

impl Editor {
    pub fn open(workspace: &mut Workspace, preset: Option<String>) -> Result<Self, CommandError> {
        let owner = workspace.active().ok_or_else(CommandError::no_document)?;
        let session = workspace.session().ok_or_else(CommandError::no_document)?;
        let active_tool = pixel_presets::editor_tool(&session.active_tool);
        let tool = active_tool.unwrap_or("tool.paint.brush").to_owned();
        let activate_tool = preset.is_some() && active_tool.is_none();
        let inherited_wet_edges = session.tool_options.flag(&tool, "brush.wet_edges").unwrap_or(false);
        let mut params = json!({"tool": tool});
        if let Some(id) = &preset { params["id"] = json!(id); }
        let reply = workspace.run(Scope::active(), Command::with_params(EDITOR_QUERY, params)).1?;
        let tip = serde_json::from_value(reply["tip"].clone()).map_err(|_| CommandError::invalid_params())?;
        Ok(Self {
            owner, tool, tip, activate_tool, preset: preset.filter(|id| !pixel_presets::builtin(id)),
            tab: Tab::General, channel: 0, dragged: None, inherited_wet_edges, inactive_wet_curve: None,
            sub_index: 0, sub_search: String::new(), preview: None, error: None,
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, workspace: &mut Workspace) -> bool {
        if workspace.active() != Some(self.owner) {
            return true;
        }
        let (close, buttons) = show_modal(ctx, "dtool.pixel_brush_editor", text("brush_editor.title"), ClosePolicy::CancelOnly, false, |ui| {
            let width = 420.0_f32.min((ui.ctx().content_rect().width() - 44.0).max(180.0));
            ui.set_width(width);
            ui.label(crate::panels::pixel_brushes::tip_label(&self.tip));
            self.show_preview(ui);
            let tab_before = self.tab;
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.tab, Tab::General, text("brush_editor.general"));
                ui.selectable_value(&mut self.tab, Tab::Dynamics, text("brush_editor.dynamics"));
                if self.tip.texture.is_some() {
                    ui.selectable_value(&mut self.tab, Tab::Texture, text("brush_editor.texture"));
                }
                if matches!(dtool_core::commands::pixel_brush::operation(&self.tool), Some(BrushOp::Paint | BrushOp::Erase)) {
                    ui.selectable_value(&mut self.tab, Tab::SubBrushes, text("brush_editor.sub_brushes"));
                }
                if dtool_core::commands::pixel_brush::operation(&self.tool) == Some(BrushOp::Paint) {
                    ui.selectable_value(&mut self.tab, Tab::WetEdges, text("brush.wet_edges"));
                }
            });
            if self.tab != tab_before { self.dragged = None; }
            ui.separator();
            let height = (ui.ctx().content_rect().height() - 230.0).clamp(120.0, 250.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
                match self.tab {
                    Tab::General => self.show_general(ui),
                    Tab::Dynamics => self.show_dynamics(ui),
                    Tab::Texture => self.show_texture(ui),
                    Tab::SubBrushes => self.show_sub_brushes(ui),
                    Tab::WetEdges => self.show_wet_edges(ui),
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, text(error));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let apply = ui.button(text("button.apply")).clicked();
                let cancel = ui.button(text("dialog.cancel")).clicked();
                (apply, cancel)
            }).inner
        });
        if buttons.1 || close == DialogResult::Cancel {
            return true;
        }
        if !buttons.0 {
            return false;
        }
        let mut params = json!({"tool": self.tool, "tip": self.tip});
        if let Some(id) = &self.preset { params["id"] = Value::String(id.clone()); }
        match workspace.run(Scope::active(), Command::with_params(EDITOR_SET, params)).1 {
            Ok(_) => {
                if self.activate_tool && let Some(session) = workspace.session_mut() {
                    session.active_tool.clone_from(&self.tool);
                }
                true
            }
            Err(error) => { self.error = Some(error.message_key); false }
        }
    }

    fn show_general(&mut self, ui: &mut Ui) {
        ui.spacing_mut().slider_width = (ui.available_width() - 160.0).max(70.0);
        let textured = self.tip.image.is_some();
        egui::Grid::new("brush_editor.general").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            let label = ui.label(text("brush_editor.accumulation"));
            let mut accumulation = self.tip.accumulation.unwrap_or(1.0) * 100.0;
            if ui.add(egui::Slider::new(&mut accumulation, 0.0..=100.0).suffix("%").max_decimals(1))
                .labelled_by(label.id).changed()
            {
                self.tip.accumulation = Some(accumulation / 100.0);
            }
            ui.end_row();
            let round_tip = self.tip.kind == TipKind::Round;
            let controls = [
                ("context.brush.width", &mut self.tip.size_px, 1.0..=2048.0, 1.0, " px"),
                ("context.brush.opacity", &mut self.tip.opacity, 0.0..=100.0, 100.0, "%"),
                ("context.brush.flow", &mut self.tip.flow, 0.0..=100.0, 100.0, "%"),
                ("context.brush.hardness", &mut self.tip.hardness, 0.0..=100.0, 100.0, "%"),
                ("brush_editor.spacing", &mut self.tip.spacing, 1.0..=999.0, 100.0, "%"),
                ("brush_editor.roundness", &mut self.tip.roundness, 1.0..=100.0, 100.0, "%"),
                ("brush_editor.rotation", &mut self.tip.rotation, -180.0..=180.0, 1.0, "°"),
            ];
            for (key, value, range, scale, suffix) in controls {
                let label = ui.label(text(key));
                let mut display = *value * scale;
                let enabled = (key != "context.brush.hardness" || !textured)
                    && (key != "brush_editor.roundness" || round_tip);
                let changed = ui.add_enabled(enabled,
                    egui::Slider::new(&mut display, range).suffix(suffix).max_decimals(1)
                        .logarithmic(matches!(key, "context.brush.width" | "brush_editor.spacing")))
                    .labelled_by(label.id).changed();
                if changed { *value = display / scale; }
                ui.end_row();
            }
            let shape = ui.label(text("brush_editor.shape"));
            ui.horizontal(|ui| {
                let choices = if textured {
                    [(TipKind::Image, "brush_editor.image"), (TipKind::Intensity, "brush_editor.intensity")]
                } else {
                    [(TipKind::Round, "brush_editor.round"), (TipKind::Square, "brush_editor.square")]
                };
                for (kind, key) in choices {
                    ui.selectable_value(&mut self.tip.kind, kind, text(key)).labelled_by(shape.id);
                }
            });
            ui.end_row();
        });
    }

    fn show_wet_edges(&mut self, ui: &mut Ui) {
        let label = ui.label(text("brush.wet_edges"));
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (value, key) in [
                (None, "brush_editor.wet_edges.inherit"),
                (Some(true), "brush_editor.wet_edges.on"),
                (Some(false), "brush_editor.wet_edges.off"),
            ] {
                changed |= ui.selectable_value(&mut self.tip.wet_edges.enabled, value, text(key))
                    .labelled_by(label.id).changed();
            }
        });
        if !self.tip.sub_brushes.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let label = ui.label(text("brush_editor.wet_edges.scope"));
                for (scope, key) in [
                    (WetEdgeScope::PerTip, "brush_editor.wet_edges.per_tip"),
                    (WetEdgeScope::WholeStroke, "brush_editor.wet_edges.whole_stroke"),
                ] {
                    changed |= ui.selectable_value(&mut self.tip.wet_edges.scope, scope, text(key))
                        .labelled_by(label.id).changed();
                }
            });
        }
        let mut custom = self.tip.wet_edges.curve.is_some();
        if dtool_design::toggle(ui, &mut custom, text("brush_editor.wet_edges.custom")).changed() {
            self.dragged = None;
            self.tip.wet_edges.curve = if custom {
                Some(self.inactive_wet_curve.take().unwrap_or_default())
            } else {
                self.inactive_wet_curve = self.tip.wet_edges.curve.take();
                None
            };
            changed = true;
        }

        if let Some(curve) = &mut self.tip.wet_edges.curve {
            let mut smooth = !curve.second_derivatives.is_empty();
            let mut rebuild = false;
            ui.horizontal(|ui| {
                rebuild |= ui.selectable_value(&mut smooth, false, text("brush_editor.wet_edges.linear")).changed();
                rebuild |= ui.selectable_value(&mut smooth, true, text("brush_editor.wet_edges.smooth")).changed();
                if ui.small_button(text("brush_editor.reset_curve")).clicked() {
                    *curve = WetEdgeCurve::default();
                    self.dragged = None;
                    rebuild = true;
                }
            });
            let (plot, edited) = edit_curve(ui, &mut curve.points, &mut self.dragged, 64, false);
            if rebuild || edited {
                curve.set_smooth(smooth);
                changed = true;
            }
            draw_curve(ui, plot, &curve.points, (0..=128).map(|step| {
                let x = step as f32 / 128.0;
                [x, curve.sample(x)]
            }));
            ui.weak(text("brush_editor.curve_hint"));
        }
        if changed {
            ui.ctx().request_repaint();
        }
    }

    fn show_texture(&mut self, ui: &mut Ui) {
        let Some(texture) = &mut self.tip.texture else { return; };
        show_texture_controls(ui, texture);
    }

    fn show_sub_brushes(&mut self, ui: &mut Ui) {
        let count = self.tip.sub_brushes.len();
        let mut added = None;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(count < MAX_SUB_BRUSHES, |ui| {
                ui.menu_button(text("brush_editor.sub_brush.add"), |ui| {
                    let Ok(library) = pixel_presets::library_snapshot() else {
                        ui.weak(text("error.io_error"));
                        return;
                    };
                    ui.add(egui::TextEdit::singleline(&mut self.sub_search)
                        .hint_text(text("path_brushes.search")).desired_width(206.0));
                    let search = self.sub_search.trim().to_lowercase();
                    egui::ScrollArea::vertical().id_salt("sub-brush-presets").max_height(196.0).show(ui, |ui| {
                        for tip in &library.tips {
                            let label = crate::panels::pixel_brushes::tip_label(tip);
                            let item = format!("{label} · {}", crate::panels::pixel_brushes::category_label(&tip.category));
                            if !search.is_empty() && !item.to_lowercase().contains(&search) {
                                continue;
                            }
                            let supported = pixel_presets::supports_sub_brush(tip);
                            let button = ui.add_enabled(supported, egui::Button::new(item));
                            if !supported {
                                button.on_disabled_hover_text(text("brush_editor.sub_brush.unsupported"));
                            } else if button.clicked() {
                                added = Some(tip.clone());
                                ui.close();
                            }
                        }
                    });
                });
            });
            if count == MAX_SUB_BRUSHES {
                ui.weak(text("brush_editor.sub_brush.limit"));
            }
            let index = self.sub_index;
            let up = sub_brush_button(ui, index > 0 && index < count, "↑", "appearance.move_up");
            let down = sub_brush_button(ui, index + 1 < count, "↓", "appearance.move_down");
            let remove = sub_brush_button(ui, index < count, "−", "brush_editor.sub_brush.remove");
            if up || down {
                let destination = if up { index - 1 } else { index + 1 };
                self.tip.sub_brushes.swap(index, destination);
                self.sub_index = destination;
            } else if remove {
                self.tip.sub_brushes.remove(index);
                self.sub_index = index.min(self.tip.sub_brushes.len().saturating_sub(1));
            }
        });
        if let Some(tip) = added {
            self.sub_index = self.tip.sub_brushes.len();
            self.tip.sub_brushes.push(SubBrush {
                tip,
                enabled: true,
                sync_size: true,
                sync_spacing: true,
                placement: SubBrushPlacement::AtNozzle,
                blending: SubBrushBlend::Blend,
                color_blend: SubBrushColorBlend::Normal,
            });
            self.sub_search.clear();
        }
        if self.tip.sub_brushes.is_empty() {
            ui.weak(text("brush_editor.sub_brush.empty"));
            return;
        }

        egui::ScrollArea::vertical().id_salt("sub-brush-order").max_height(69.0).show(ui, |ui| {
            for (index, sub) in self.tip.sub_brushes.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let toggle = text("brush_editor.sub_brush.enabled").replace("{number}", &(index + 1).to_string());
                    dtool_design::toggle(ui, &mut sub.enabled, "").on_hover_text(&toggle)
                        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), sub.enabled, &toggle));
                    let name = crate::panels::pixel_brushes::tip_label(&sub.tip);
                    ui.selectable_value(&mut self.sub_index, index, format!("{}. {name}", index + 1));
                });
            }
        });
        ui.separator();
        let Some(sub) = self.tip.sub_brushes.get_mut(self.sub_index) else { return; };
        ui.add_enabled_ui(sub.enabled, |ui| {
            ui.horizontal_wrapped(|ui| {
                let label = ui.label(text("brush_editor.sub_brush.placement"));
                for (placement, key) in [
                    (SubBrushPlacement::BehindNozzle, "brush_editor.sub_brush.behind"),
                    (SubBrushPlacement::AtNozzle, "brush_editor.sub_brush.at_nozzle"),
                    (SubBrushPlacement::StrokeEnd, "brush_editor.sub_brush.stroke_end"),
                ] {
                    ui.selectable_value(&mut sub.placement, placement, text(key)).labelled_by(label.id);
                }
            });
            ui.horizontal_wrapped(|ui| {
                let label = ui.label(text("brush_editor.sub_brush.blending"));
                ui.selectable_value(&mut sub.blending, SubBrushBlend::Blend,
                    text("brush_editor.sub_brush.blend")).labelled_by(label.id);
                ui.selectable_value(&mut sub.blending, SubBrushBlend::Erase,
                    text("brush_editor.sub_brush.erase")).labelled_by(label.id);
            });
            ui.horizontal_wrapped(|ui| {
                dtool_design::toggle(ui, &mut sub.sync_size, text("brush_editor.sub_brush.sync_size"));
                dtool_design::toggle(ui, &mut sub.sync_spacing, text("brush_editor.sub_brush.sync_spacing"));
            });
            let tip = &mut sub.tip;
            ui.add_enabled(!sub.sync_size, egui::Slider::new(&mut tip.size_px, 1.0..=2048.0)
                .logarithmic(true).suffix(" px").text(text("context.brush.width")));
            ui.add_enabled_ui(!sub.sync_spacing, |ui| {
                sub_brush_percent(ui, &mut tip.spacing, 1.0..=999.0, "brush_editor.spacing");
            });
            sub_brush_percent(ui, &mut tip.opacity, 0.0..=100.0, "context.brush.opacity");
            let mut accumulation = tip.accumulation.unwrap_or(1.0);
            sub_brush_percent(ui, &mut accumulation, 0.0..=100.0, "brush_editor.accumulation");
            if accumulation != tip.accumulation.unwrap_or(1.0) {
                tip.accumulation = Some(accumulation);
            }
            sub_brush_percent(ui, &mut tip.flow, 0.0..=100.0, "context.brush.flow");
            ui.add_enabled_ui(tip.image.is_none(), |ui| {
                sub_brush_percent(ui, &mut tip.hardness, 0.0..=100.0, "context.brush.hardness");
            });
            if tip.kind == TipKind::Round {
                sub_brush_percent(ui, &mut tip.roundness, 1.0..=100.0, "brush_editor.roundness");
            }
            ui.add(egui::Slider::new(&mut tip.rotation, -180.0..=180.0).suffix("°").text(text("brush_editor.rotation")));
            ui.collapsing(text("brush_editor.tip_motion"), |ui| {
                show_tip_motion(ui, &mut tip.dynamics);
            });
            if let Some(texture) = &mut tip.texture {
                ui.collapsing(text("brush_editor.texture"), |ui| {
                    show_texture_controls(ui, texture);
                });
            }
            if sub.blending == SubBrushBlend::Blend {
                ui.horizontal_wrapped(|ui| {
                    let label = ui.label(text("brush_editor.sub_brush.color_blend"));
                    ui.selectable_value(&mut sub.color_blend, SubBrushColorBlend::Normal,
                        text("context.fill.blend.normal")).labelled_by(label.id);
                    ui.selectable_value(&mut sub.color_blend, SubBrushColorBlend::Average,
                        text("menu.pixel.filters.blur.average")).labelled_by(label.id);
                });
                ui.horizontal_wrapped(|ui| {
                    let label = ui.label(text("brush.wet_edges"));
                    for (enabled, key) in [
                        (None, "brush_editor.wet_edges.inherit"),
                        (Some(true), "brush_editor.wet_edges.on"),
                        (Some(false), "brush_editor.wet_edges.off"),
                    ] {
                        ui.selectable_value(&mut tip.wet_edges.enabled, enabled, text(key)).labelled_by(label.id);
                    }
                });
            }
        });
    }

    fn show_dynamics(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for (index, key) in CHANNELS.into_iter().enumerate() {
                if ui.selectable_value(&mut self.channel, index, text(key)).changed() { self.dragged = None; }
            }
        });
        let dynamics = &mut self.tip.dynamics;
        let controller = match self.channel {
            0 => &mut dynamics.size,
            1 => &mut dynamics.opacity,
            2 => &mut dynamics.flow,
            3 => &mut dynamics.hardness,
            _ => &mut dynamics.rotation,
        };
        ui.horizontal(|ui| {
            let label = ui.label(text("brush_editor.controller"));
            let key = CONTROLLERS.iter().find(|(kind, _)| *kind == controller.kind).unwrap().1;
            egui::ComboBox::from_id_salt("brush_editor.controller")
                .selected_text(text(key)).show_ui(ui, |ui| {
                    for (kind, key) in CONTROLLERS {
                        ui.selectable_value(&mut controller.kind, kind, text(key));
                    }
                }).response.labelled_by(label.id);
        });
        ui.add_enabled_ui(controller.kind != ControllerKind::None, |ui| {
            let mut percent = controller.jitter * 100.0;
            if ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%").text(text("brush_editor.jitter"))).changed() {
                controller.jitter = percent / 100.0;
            }
            let (plot, _) = edit_curve(ui, &mut controller.curve, &mut self.dragged, 16, true);
            draw_curve(ui, plot, &controller.curve, controller.curve.iter().copied());
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(text("brush_editor.reset_curve")).clicked() {
                    controller.curve = Controller::default().curve;
                    self.dragged = None;
                }
                ui.weak(text("brush_editor.curve_hint"));
            });
        });
        ui.separator();
        show_tip_motion(ui, dynamics);
    }

    fn show_preview(&mut self, ui: &mut Ui) {
        let enabled = self.tip.wet_edges.enabled.unwrap_or(self.inherited_wet_edges);
        if self.preview.as_ref().is_none_or(|(tip, _)| tip != &self.tip) {
            let image = preview_with_wet_edges(&self.tip, enabled);
            if let Some((tip, texture)) = &mut self.preview {
                *tip = self.tip.clone();
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                let texture = ui.ctx().load_texture("pixel_brush_editor.preview", image, egui::TextureOptions::LINEAR);
                self.preview = Some((self.tip.clone(), texture));
            }
        }
        if let Some((_, texture)) = &self.preview {
            let size = Vec2::new(ui.available_width(), 60.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
            ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
    }
}

fn show_tip_motion(ui: &mut Ui, dynamics: &mut BrushDynamics) {
    sub_brush_percent(ui, &mut dynamics.direction_rotation, 0.0..=100.0, "brush_editor.stroke_direction");
    sub_brush_percent(ui, &mut dynamics.rotation_jitter, 0.0..=100.0, "brush_editor.rotation_jitter");
    if dynamics.size.kind == ControllerKind::Tilt {
        let previous = dynamics.tilt_scale.unwrap_or(1.0);
        let mut scale = previous;
        sub_brush_percent(ui, &mut scale, 0.0..=200.0, "brush_editor.tilt_scale");
        if scale != previous {
            dynamics.tilt_scale = Some(scale);
        }
    }
    for (jitter, amount, minimum) in [
        (&mut dynamics.size_jitter, "brush_editor.size_jitter", "brush_editor.minimum_diameter"),
        (&mut dynamics.roundness_jitter, "brush_editor.roundness_jitter", "brush_editor.minimum_roundness"),
    ] {
        sub_brush_percent(ui, &mut jitter.amount, 0.0..=100.0, amount);
        ui.add_enabled_ui(jitter.amount > 0.0, |ui| {
            sub_brush_percent(ui, &mut jitter.minimum, 0.0..=100.0, minimum);
        });
    }
    for (index, axis) in dynamics.scatter.iter_mut().enumerate() {
        let key = ["brush_editor.scatter_x", "brush_editor.scatter_y"][index];
        ui.horizontal_wrapped(|ui| {
            sub_brush_percent(ui, &mut axis.amount, 0.0..=100.0 * ScatterAxis::MAX_AMOUNT, key);
            let mode = if axis.pressure { "brush_editor.controller.pressure" } else { "brush_editor.controller.random" };
            egui::ComboBox::from_id_salt(key).selected_text(text(mode)).show_ui(ui, |ui| {
                ui.selectable_value(&mut axis.pressure, false, text("brush_editor.controller.random"));
                ui.selectable_value(&mut axis.pressure, true, text("brush_editor.controller.pressure"));
            });
        });
    }
    ui.collapsing(text("brush_editor.path_scatter"), |ui| {
        let scatter = &mut dynamics.path_scatter;
        sub_brush_percent(ui, &mut scatter.amount, 0.0..=100.0 * ScatterAxis::MAX_AMOUNT, "brush_editor.path_scatter_amount");
        dtool_design::toggle(ui, &mut scatter.both_axes, text("brush_editor.path_scatter_both_axes"));
        dtool_design::toggle(ui, &mut scatter.pressure, text("brush_editor.path_scatter_pressure"));
        ui.add(egui::Slider::new(&mut scatter.count, 1..=dtool_core::brush::PathScatter::MAX_COUNT)
            .text(text("brush_editor.path_scatter_count")));
        sub_brush_percent(ui, &mut scatter.count_jitter, 0.0..=100.0, "brush_editor.path_scatter_count_jitter");
        dtool_design::toggle(ui, &mut scatter.count_pressure, text("brush_editor.path_scatter_count_pressure"));
    });
}

fn show_texture_controls(ui: &mut Ui, texture: &mut BrushTexture) {
    dtool_design::toggle(ui, &mut texture.enabled, text("brush_editor.texture.enabled"));
    ui.add_enabled_ui(texture.enabled, |ui| {
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(text("brush_editor.texture.mode"));
            ui.selectable_value(&mut texture.mode, BrushTextureMode::Nozzle,
                text("brush_editor.texture.nozzle")).labelled_by(label.id);
            ui.selectable_value(&mut texture.mode, BrushTextureMode::Final,
                text("brush_editor.texture.final")).labelled_by(label.id);
        });
        dtool_design::toggle(ui, &mut texture.inverted, text("brush_editor.texture.invert"));
        let mut percent = texture.scale * 100.0;
        if ui.add(egui::Slider::new(&mut percent, 1.0..=1000.0)
            .logarithmic(true).suffix("%").text(text("brush_editor.texture.scale"))).changed()
        {
            texture.scale = percent / 100.0;
        }
    });
}

fn sub_brush_button(ui: &mut Ui, enabled: bool, icon: &str, key: &str) -> bool {
    let label = text(key);
    let response = ui.add_enabled(enabled, egui::Button::new(icon).small()).on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    response.clicked()
}

fn sub_brush_percent(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, key: &str) {
    let mut percent = *value * 100.0;
    if ui.add(egui::Slider::new(&mut percent, range).suffix("%").text(text(key))).changed() {
        *value = percent / 100.0;
    }
}

fn edit_curve(
    ui: &mut Ui, points: &mut Vec<[f32; 2]>, dragged: &mut Option<CurveDrag>,
    capacity: usize, fixed_endpoints: bool,
) -> (Rect, bool) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 110.0), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, ui.is_enabled(), text("brush_editor.curve")));
    let plot = rect.shrink(6.0);
    let from_plot = |pos: Pos2| [(pos.x - plot.left()) / plot.width(), (plot.bottom() - pos.y) / plot.height()].map(|v| v.clamp(0.0, 1.0));
    let nearest = |pos: Pos2, curve: &[[f32; 2]]| {
        curve.iter().enumerate().filter_map(|(index, &point)| {
            let distance = pos.distance(plot_point(plot, point));
            (distance <= 8.0).then_some((index, distance))
        }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(index, _)| index)
    };
    let mut changed = false;
    if ui.is_enabled() {
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => {
                    *dragged = (response.interact_rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id()))
                        .then(|| nearest(pos, points)).flatten()
                        .map(|index| CurveDrag { index, origin: pos, moved: false });
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some(drag) = dragged {
                        changed |= move_curve_point(points, drag, pos, from_plot, fixed_endpoints);
                    }
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => {
                    if let Some(mut drag) = dragged.take() {
                        changed |= move_curve_point(points, &mut drag, pos, from_plot, fixed_endpoints);
                    }
                }
                egui::Event::PointerGone => *dragged = None,
                _ => {}
            }
        }
        if let Some(pos) = response.interact_pointer_pos() {
            let [x, y] = from_plot(pos);
            if response.double_clicked() && points.len() < capacity {
                let index = points.partition_point(|point| point[0] < x);
                if (!fixed_endpoints || index > 0 && index < points.len())
                    && (index == 0 || x - points[index - 1][0] > 0.006)
                    && (index == points.len() || points[index][0] - x > 0.006)
                {
                    points.insert(index, [x, y]);
                    changed = true;
                }
            } else if response.secondary_clicked() && let Some(index) = nearest(pos, points)
                && points.len() > 2 && (!fixed_endpoints || index > 0 && index + 1 < points.len())
            {
                points.remove(index);
                *dragged = None;
                changed = true;
            }
        }
    }
    (plot, changed)
}

fn move_curve_point(
    points: &mut [[f32; 2]], drag: &mut CurveDrag, pos: Pos2,
    from_plot: impl Fn(Pos2) -> [f32; 2], fixed_endpoints: bool,
) -> bool {
    drag.moved |= pos.distance_sq(drag.origin) > 4.0;
    if !drag.moved {
        return false;
    }
    let index = drag.index;
    let [x, y] = from_plot(pos);
    let first = index == 0;
    let last = index + 1 == points.len();
    let x = if fixed_endpoints && (first || last) {
        points[index][0]
    } else {
        let left = if first { 0.0 } else { points[index - 1][0] };
        let right = if last { 1.0 } else { points[index + 1][0] };
        let gap = ((right - left) * 0.25).min(0.002);
        x.clamp(if first { 0.0 } else { left + gap }, if last { 1.0 } else { right - gap })
    };
    let changed = points[index] != [x, y];
    points[index] = [x, y];
    changed
}

fn plot_point(plot: Rect, [x, y]: [f32; 2]) -> Pos2 {
    Pos2::new(plot.left() + x * plot.width(), plot.bottom() - y * plot.height())
}

fn draw_curve(ui: &Ui, plot: Rect, points: &[[f32; 2]], curve: impl IntoIterator<Item = [f32; 2]>) {
    let painter = ui.painter();
    painter.rect_filled(plot.expand(6.0), 4.0, ui.visuals().extreme_bg_color);
    let grid = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    let accent = ui.visuals().selection.bg_fill;
    for step in 0..=4 {
        let t = step as f32 * 0.25;
        painter.line_segment([plot_point(plot, [t, 0.0]), plot_point(plot, [t, 1.0])], grid);
        painter.line_segment([plot_point(plot, [0.0, t]), plot_point(plot, [1.0, t])], grid);
    }
    painter.add(egui::Shape::line(curve.into_iter().map(|point| plot_point(plot, point)).collect(), Stroke::new(1.5, accent)));
    for &point in points {
        painter.circle_filled(plot_point(plot, point), 3.5, accent);
    }
}

pub(crate) fn preview(tip: &BrushTip) -> egui::ColorImage {
    preview_with_wet_edges(tip, tip.wet_edges.enabled.unwrap_or(false))
}

fn preview_with_wet_edges(tip: &BrushTip, wet_edges: bool) -> egui::ColorImage {
    const WIDTH: u32 = 420;
    const HEIGHT: u32 = 60;
    let colour = Color32::from_gray(220);
    let size = tip.size_px.clamp(3.0, 35.0);
    let mut sub_brushes = tip.sub_brushes.clone();
    for sub in &mut sub_brushes {
        sub.tip.size_px *= size / tip.size_px;
    }
    let settings = BrushSettings {
        size_px: size, opacity: tip.opacity, flow: tip.flow,
        accumulation: tip.accumulation,
        hardness: tip.hardness, roundness: tip.roundness, spacing: tip.spacing, rotation: tip.rotation, dynamics: tip.dynamics.clone(),
        tip_kind: tip.kind, stamp: tip.image.clone(), additional_stamps: tip.additional_images.clone(),
        texture: tip.texture.clone(), sub_brushes, colour: colour.to_array(),
        wet_edges, wet_edge_curve: tip.wet_edges.curve.clone(),
        wet_edge_scope: tip.wet_edges.scope,
        ..BrushSettings::default()
    };
    let samples = (20..400).step_by(3).map(|x| {
        let t = (x - 20) as f32 / 380.0;
        StrokePoint { x: x as f32, y: 30.0 - (t * std::f32::consts::TAU).sin() * 7.0,
            pressure: t, has_pressure: true, tilt: [t, 0.0], velocity: 1.0 - t }
    }).collect::<Vec<_>>();
    let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
    brush::stroke(&mut RasterViewMut::new(WIDTH, HEIGHT, &mut pixels), &samples, &settings, BrushOp::Paint, &brush::OpParams::default());
    egui::ColorImage::from_rgba_unmultiplied([WIDTH as usize, HEIGHT as usize], &pixels)
}
