use super::{AiImagePanel, ImageAction};
use dtool_core::{
    ObjectId, Session,
    ai_image::{APPLY, CANCEL, STATUS},
    workspace::ContextId,
};
use dtool_design::{ControlState, NumericSpec, numeric_value};
use egui::{Color32, Pos2, Rect, TextureHandle, Vec2};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) const ACCENT: Color32 = Color32::from_rgb(138, 221, 207);
const SELECTED: Color32 = Color32::from_rgb(38, 71, 69);
const HEADER: Color32 = Color32::from_rgb(33, 37, 39);
const TOOLS: Color32 = Color32::from_rgb(40, 44, 46);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Compare {
    #[default]
    Vertical,
    Horizontal,
    Hold,
}

#[derive(PartialEq, Eq)]
enum PreviewSource {
    Job(ContextId, u64),
    Layer(ContextId, ObjectId, u64),
}

pub(super) struct WorkspaceView {
    compare: Compare,
    divider: f32,
    job: Option<(ContextId, u64)>,
    original: Option<(PreviewSource, TextureHandle)>,
    results: Vec<TextureHandle>,
    selected: BTreeSet<usize>,
    transparency: Option<TextureHandle>,
    view_source: Option<(ContextId, ImageAction, Option<ObjectId>)>,
    zoom: Option<f32>,
    pan: Vec2,
    pan_tool: bool,
    image_size: Vec2,
}

impl Default for WorkspaceView {
    fn default() -> Self {
        Self {
            compare: Compare::Vertical,
            divider: 0.5,
            job: None,
            original: None,
            results: vec![],
            selected: BTreeSet::new(),
            transparency: None,
            view_source: None,
            zoom: None,
            pan: Vec2::ZERO,
            pan_tool: false,
            image_size: Vec2::ZERO,
        }
    }
}

pub(super) fn workspace_button(
    ui: &mut egui::Ui, icons: &mut crate::icon_cache::IconCache, icon: &str, label: &str, selected: bool,
) -> egui::Response {
    ui.scope(|ui| {
        dtool_design::button_visuals(ui);
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        widgets.hovered.weak_bg_fill = Color32::from_rgb(55, 63, 65);
        widgets.active.weak_bg_fill = Color32::from_rgb(65, 78, 79);
        let ink = if selected { ACCENT } else { ui.visuals().text_color() };
        let title = egui::RichText::new(label).color(ink);
        let mut button = if let Some(texture) = icons.texture(ui.ctx(), icon, ink, dtool_design::icon_texture_px(ui)) {
            egui::Button::image_and_text(egui::Image::new(&texture).fit_to_exact_size(Vec2::splat(15.0)), title)
        } else {
            egui::Button::new(title)
        };
        button = button.corner_radius(6.0).min_size(egui::vec2(28.0, 29.0));
        if selected {
            button = button.fill(SELECTED).stroke(egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.45)));
        }
        let response = ui.add(button);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
        response
    }).inner
}

impl AiImagePanel {
    pub(crate) fn setup_frame(&mut self, ctx: &egui::Context) {
        self.setup.poll(ctx, &mut self.port);
    }

    pub(crate) fn header(&mut self, ui: &mut egui::Ui, session: &mut Session) -> bool {
        let status = session.run_named(STATUS, json!({})).unwrap_or_default();
        let running = status["state"] == "running";
        let mut back = false;
        egui::Panel::top("dtool.ai.header")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(HEADER)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                if ui.available_width() < 820.0 {
                    ui.horizontal_wrapped(|ui| back = self.navigation_controls(ui, running));
                    self.render_controls_aligned(ui, session, &status, running);
                } else {
                    ui.horizontal(|ui| {
                        back = self.navigation_controls(ui, running);
                        self.render_controls_aligned(ui, session, &status, running);
                    });
                }
                if running {
                    let (rail, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 2.0),
                        egui::Sense::hover(),
                    );
                    let t = ui.input(|input| (input.time % 1.5) as f32 / 1.5);
                    let (width, x) = if let Some(progress) = status["progress"].as_u64() {
                        (rail.width() * progress as f32 / 100.0, rail.left())
                    } else {
                        let width = rail.width() * 0.2;
                        (width, rail.left() + (rail.width() - width) * t)
                    };
                    ui.painter().rect_filled(
                        rail,
                        0.0,
                        dtool_design::theme::surface(ui.ctx(), "pasteboard"),
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            egui::pos2(x, rail.top()),
                            egui::vec2(width, rail.height()),
                        ),
                        0.0,
                        ACCENT,
                    );
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(40));
                }
            });
        self.setup
            .options_window(ui.ctx(), &mut self.port, self.action == ImageAction::Edit);
        back
    }

    fn navigation_controls(&mut self, ui: &mut egui::Ui, running: bool) -> bool {
        let back = workspace_button(ui, &mut self.icons, "persona-home", "돌아가기", false).clicked();
        ui.separator();
        ui.label(egui::RichText::new("AI 이미지").strong().color(ACCENT));
        ui.separator();
        self.setup.toolbar(ui, &mut self.icons, running);
        back
    }

    fn render_controls_aligned(
        &mut self, ui: &mut egui::Ui, session: &mut Session, status: &serde_json::Value, running: bool,
    ) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if running {
                if workspace_button(ui, &mut self.icons, "ai-stop", "렌더 취소", false).clicked() {
                    let _ = session.run_named(CANCEL, json!({}));
                }
                if let Some(progress) = status["progress"].as_u64() {
                    ui.strong(format!("{progress}%"));
                }
                if let Some(operation) = status["operation"].as_str() {
                    ui.label(dtool_strings::text(operation));
                }
                return;
            }
            self.render_control(ui, session);
            if self.action.has_candidate_count() {
                let spec = NumericSpec::new("").range(1.0..=4.0).width(46.0).suffix("장").decimals(0).reset_to(1.0);
                let count = numeric_value(ui, ui.make_persistent_id("ai.render.count"), &mut self.count, &spec);
                let label = ui.weak("결과 수");
                count.labelled_by(label.id);
                ui.separator();
            }
            if self.action.uses_prompt() {
                ui.horizontal(|ui| super::quality_tabs(ui, &mut self.detail));
                ui.weak("품질");
            }
        });
    }

    pub(crate) fn tools(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("dtool.ai.tools")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(TOOLS)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (index, (action, key)) in ImageAction::TOOLS.iter().copied().enumerate() {
                        if matches!(index, 2 | 4) {
                            ui.separator();
                        }
                        let icon = match action {
                            ImageAction::Generate => "cmd-assistant",
                            ImageAction::Edit => "tool-inpaint",
                            ImageAction::RemoveBackground => "layer-mask-pixel",
                            ImageAction::SelectSubject => "tool-object-selection",
                            ImageAction::Upscale => "ctx-zoom-in",
                            ImageAction::DepthMap => "cmd-gradient-map",
                            ImageAction::Outpaint => "cmd-transform-box",
                            ImageAction::RegenerateDetail => "cmd-tone",
                        };
                        if workspace_button(ui, &mut self.icons, icon, dtool_strings::text(key), self.action == action).clicked() {
                            self.set_action(action);
                        }
                    }
                });
            });
    }

    fn comparison_modes(&mut self, ui: &mut egui::Ui) {
        for (mode, icon, label) in [
            (Compare::Vertical, "ai-compare-vertical", "좌우 비교"),
            (Compare::Horizontal, "ai-compare-horizontal", "상하 비교"),
            (Compare::Hold, "cmd-preview-mode", "누르는 동안 원본"),
        ] {
            let selected = self.workspace_view.compare == mode;
            if workspace_button(ui, &mut self.icons, icon, label, selected).clicked() {
                self.workspace_view.compare = mode;
                if mode == Compare::Hold {
                    self.workspace_view.pan_tool = false;
                }
            }
        }
    }

    pub(crate) fn dock_options(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        self.poll_models();
        dtool_design::panel_header(
            ui,
            match self.action {
                ImageAction::Generate => "생성 옵션",
                ImageAction::Edit => "편집 옵션",
                ImageAction::RemoveBackground => dtool_strings::text("ai.image.remove_background"),
                ImageAction::SelectSubject => dtool_strings::text("ai.image.select_subject"),
                ImageAction::Upscale => dtool_strings::text("ai.image.upscale"),
                ImageAction::DepthMap => dtool_strings::text("ai.image.depth_map"),
                ImageAction::Outpaint => dtool_strings::text("ai.image.outpaint"),
                ImageAction::RegenerateDetail => dtool_strings::text("ai.image.regenerate_detail"),
            },
        );
        let status = session.run_named(STATUS, json!({})).unwrap_or_default();
        egui::ScrollArea::vertical()
            .id_salt(self.action.key())
            .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add_enabled_ui(status["state"] != "running", |ui| {
                self.options(ui, session, true)
            });
            if let Some(key) = (status["operation"] == self.action.key())
                .then(|| status["error"]["message_key"].as_str()).flatten()
                .or(self.feedback.as_deref())
            {
                ui.label(dtool_strings::text(key));
            }
        });
    }

    pub(crate) fn render_workspace(&mut self, ui: &mut egui::Ui, session: &mut Session) -> bool {
        let status = session.run_named(STATUS, json!({})).unwrap_or_default();
        let matching_operation = status["operation"] == self.action.key();
        let completed = session.context.zip(status["job_id"].as_u64())
            .filter(|_| status["state"] == "done");
        let job = completed.filter(|_| matching_operation);
        if completed != self.workspace_view.job {
            self.workspace_view.job = completed;
            self.workspace_view.results.clear();
            self.workspace_view.selected.clear();
            self.index = 0;
            if let Some((_, job_id)) = completed {
                let count = status["count"].as_u64().unwrap_or(0) as usize;
                for index in 0..count {
                    let Some(image) = session.ai_image.preview_image(index) else {
                        break;
                    };
                    let rgba = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.rgba,
                    );
                    self.workspace_view.results.push(ui.ctx().load_texture(
                        format!("dtool.ai.{job_id}.{index}"),
                        rgba,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                if !self.workspace_view.results.is_empty() {
                    self.workspace_view.selected.insert(0);
                }
            }
        }
        let view_source = session.context.map(|context| {
            let source = if matching_operation && matches!(status["state"].as_str(), Some("running" | "done")) {
                session.ai_image.source()
            } else if let [id] = session.selection.ids.as_slice() {
                Some(*id)
            } else {
                None
            };
            (context, self.action, source)
        });
        if self.workspace_view.view_source != view_source {
            self.workspace_view.view_source = view_source;
            self.workspace_view.zoom = None;
            self.workspace_view.pan = Vec2::ZERO;
            self.workspace_view.image_size = Vec2::ZERO;
        }
        let preview = session.context.and_then(|context| {
            if matching_operation && let Some(id) = status["job_id"].as_u64()
                && matches!(status["state"].as_str(), Some("running" | "done"))
            {
                return session.ai_image.original_image().map(|_| PreviewSource::Job(context, id));
            }
            if self.action != ImageAction::Generate && let [id] = session.selection.ids.as_slice() {
                return Some(PreviewSource::Layer(context, *id, session.artwork_generation()));
            }
            None
        });
        if self.workspace_view.original.as_ref().map(|(source, _)| source) != preview.as_ref() {
            self.workspace_view.original = preview.and_then(|source| {
                let layer;
                let image = match source {
                    PreviewSource::Job(..) => session.ai_image.original_image()?,
                    PreviewSource::Layer(_, id, _) => {
                        layer = dtool_core::ai_image::source_image(session, id).ok()?;
                        &layer
                    }
                };
                let rgba = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width as usize, image.height as usize],
                    &image.rgba,
                );
                let texture =
                    ui.ctx()
                        .load_texture("dtool.ai.source", rgba, egui::TextureOptions::LINEAR);
                Some((source, texture))
            });
        }
        if let Some((_, id)) = job
            && !self.workspace_view.results.is_empty()
        {
            egui::Panel::bottom("dtool.ai.results")
                .default_size(124.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(HEADER)
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                )
                .show(ui, |ui| self.result_strip(ui, session, id));
        }
        if job.is_some() || self.workspace_view.original.is_some() {
            self.comparison_view_controls(ui, job.is_some() && self.workspace_view.original.is_some());
        }
        let current = job.and_then(|_| self.workspace_view.results.get(self.index)).cloned();
        let original = self
            .workspace_view
            .original
            .as_ref()
            .map(|(_, texture)| texture.clone());
        let Some(texture) = current.as_ref().or(original.as_ref()) else {
            return false;
        };
        let canvas = ui.available_rect_before_wrap();
        let space = (canvas.size() - Vec2::splat(34.0)).max(Vec2::splat(1.0));
        let image_size = texture.size_vec2();
        let fit = (space.x / image_size.x).min(space.y / image_size.y);
        let view = &mut self.workspace_view;
        if view.image_size != Vec2::ZERO && view.image_size != image_size {
            if let Some(zoom) = &mut view.zoom {
                *zoom *= view.image_size.x / image_size.x;
            }
        }
        view.image_size = image_size;
        let mut scale = view.zoom.unwrap_or(fit);
        let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
        if response.hovered() && let Some(pointer) = response.hover_pos() {
            let wheel = ui.input(|input| input.smooth_scroll_delta.y);
            if wheel.abs() > f32::EPSILON {
                let next = (scale * (wheel / 360.0).exp()).clamp(fit.min(0.05), 16.0);
                let anchor = pointer - canvas.center();
                view.pan = anchor - (anchor - view.pan) * (next / scale);
                view.zoom = Some(next);
                scale = next;
            }
        }
        if response.dragged_by(egui::PointerButton::Middle)
            || (view.pan_tool && response.dragged_by(egui::PointerButton::Primary))
        {
            view.pan += response.drag_delta();
        }
        if view.pan_tool && view.zoom.is_some() && response.hovered() {
            ui.ctx().set_cursor_icon(if response.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
        if view.zoom.is_none() {
            view.pan = Vec2::ZERO;
        } else {
            let span = ((canvas.size() + image_size * scale) * 0.5 - Vec2::splat(32.0)).max(Vec2::ZERO);
            view.pan = view.pan.clamp(-span, span);
        }
        let mut rect = Rect::from_center_size(canvas.center() + view.pan, image_size * scale);
        let pixels_per_point = ui.ctx().pixels_per_point();
        if (scale * pixels_per_point - 1.0).abs() < 0.001 {
            let aligned = (rect.min.to_vec2() * pixels_per_point).round() / pixels_per_point;
            rect = rect.translate(aligned - rect.min.to_vec2());
        }
        let painter = ui.painter_at(canvas);
        let background = dtool_design::theme::surface(ui.ctx(), "pasteboard");
        painter.rect_filled(canvas, 0.0, background);
        let transparency = self.workspace_view.transparency.get_or_insert_with(|| {
            let light = Color32::from_gray(118);
            let dark = Color32::from_gray(90);
            let pixels = vec![light, dark, dark, light];
            ui.ctx().load_texture(
                "dtool.ai.transparency",
                egui::ColorImage::new([2, 2], pixels),
                egui::TextureOptions {
                    wrap_mode: egui::TextureWrapMode::Repeat,
                    ..egui::TextureOptions::NEAREST
                },
            )
        });
        let tile_uv = Rect::from_min_size(Pos2::ZERO, rect.size() / 24.0);
        let paint_transparency = |painter: &egui::Painter| {
            painter.image(transparency.id(), rect, tile_uv, Color32::WHITE);
        };
        paint_transparency(&painter);
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(texture.id(), rect, uv, Color32::WHITE);
        if let (Some(original), Some(_)) = (original, current) {
            let original_rect = if let Some([left, top, _, _]) = session.ai_image.borders() {
                Rect::from_min_size(rect.min + egui::vec2(left as f32, top as f32) * scale, original.size_vec2() * scale)
            } else {
                rect
            };
            if self.workspace_view.compare == Compare::Hold {
                if !self.workspace_view.pan_tool && response.is_pointer_button_down_on() {
                    paint_transparency(&painter);
                    painter.image(original.id(), original_rect, uv, Color32::WHITE);
                }
            } else {
                let horizontal = self.workspace_view.compare == Compare::Horizontal;
                let divider = self.workspace_view.divider;
                let handle = if horizontal {
                    Rect::from_center_size(
                        egui::pos2(rect.center().x, rect.top() + rect.height() * divider),
                        egui::vec2(rect.width(), 12.0),
                    )
                } else {
                    Rect::from_center_size(
                        egui::pos2(rect.left() + rect.width() * divider, rect.center().y),
                        egui::vec2(12.0, rect.height()),
                    )
                };
                let drag = ui
                    .interact(
                        handle,
                        ui.make_persistent_id("dtool.ai.divider"),
                        egui::Sense::drag(),
                    )
                    .on_hover_cursor(if horizontal {
                        egui::CursorIcon::ResizeVertical
                    } else {
                        egui::CursorIcon::ResizeHorizontal
                    });
                if drag.dragged()
                    && let Some(pointer) = drag.interact_pointer_pos()
                {
                    let offset = pointer - rect.min;
                    let relative = egui::vec2(offset.x / rect.width(), offset.y / rect.height());
                    self.workspace_view.divider =
                        if horizontal { relative.y } else { relative.x }.clamp(0.02, 0.98);
                }
                let divider = self.workspace_view.divider;
                let segment = if horizontal {
                    let y = rect.top() + rect.height() * divider;
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)]
                } else {
                    let x = rect.left() + rect.width() * divider;
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())]
                };
                let clip = Rect::from_min_max(
                    rect.min,
                    if horizontal {
                        egui::pos2(rect.right(), segment[1].y)
                    } else {
                        egui::pos2(segment[1].x, rect.bottom())
                    },
                );
                let original_side = painter.with_clip_rect(clip);
                paint_transparency(&original_side);
                original_side.image(original.id(), original_rect, uv, Color32::WHITE);
                painter.line_segment(segment, egui::Stroke::new(1.5, Color32::WHITE));
                painter.circle_filled(segment[0].lerp(segment[1], 0.5), 5.0, Color32::WHITE);
            }
        }
        true
    }

    fn comparison_view_controls(&mut self, ui: &mut egui::Ui, can_compare: bool) {
        ui.horizontal_wrapped(|ui| {
            let view = &mut self.workspace_view;
            if workspace_button(ui, &mut self.icons, "ctx-zoom-fit", dtool_strings::text("ai.image.view_fit"), view.zoom.is_none()).clicked() {
                view.zoom = None;
            }
            let physical = ui.ctx().pixels_per_point().recip();
            let pixel_size = view.zoom.is_some_and(|zoom| (zoom - physical).abs() < 0.001);
            if workspace_button(ui, &mut self.icons, "ctx-zoom-actual", "100%", pixel_size)
                .on_hover_text(dtool_strings::text("ai.image.view_pixel_hint"))
                .clicked()
            {
                view.zoom = Some(physical);
            }
            if workspace_button(ui, &mut self.icons, "tool-view", dtool_strings::text("ai.image.view_pan"), view.pan_tool).clicked() {
                view.pan_tool = !view.pan_tool;
            }
            if let Some(zoom) = view.zoom {
                ui.label(format!("{:.0}%", 100.0 * zoom / physical));
            }
            if can_compare {
                ui.separator();
                self.comparison_modes(ui);
            }
        });
    }

    fn result_strip(&mut self, ui: &mut egui::Ui, session: &mut Session, job: u64) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("결과").strong().color(ACCENT));
            ui.weak(format!("{}장 중 {}장 선택", self.workspace_view.results.len(), self.workspace_view.selected.len()));
        });
        let thumbnail_height = (ui.ctx().content_rect().height() * 0.13).clamp(28.0, 70.0);
        egui::ScrollArea::horizontal()
            .id_salt("dtool.ai.candidates")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, texture) in self.workspace_view.results.iter().enumerate() {
                        let size = texture.size_vec2();
                        let image = egui::Image::new(texture)
                            .fit_to_exact_size(size * (thumbnail_height / size.y).min(132.0 / size.x));
                        let selected = self.workspace_view.selected.contains(&index);
                        let response = ui.add(egui::Button::image(image).selected(selected));
                        let label = format!("결과 {}", index + 1);
                        response.widget_info(|| egui::WidgetInfo::selected(
                            egui::Role::Button, ui.is_enabled(), selected, &label,
                        ));
                        if response.on_hover_text(label).clicked() {
                            self.index = index;
                            if ui.input(|input| input.modifiers.command) {
                                if !self.workspace_view.selected.remove(&index) {
                                    self.workspace_view.selected.insert(index);
                                }
                            } else {
                                self.workspace_view.selected.clear();
                                self.workspace_view.selected.insert(index);
                            }
                        }
                    }
                });
            });
        if self.action == ImageAction::SelectSubject {
            let label = dtool_strings::text("ai.image.subject_apply");
            if super::render_button(ui, label, true).clicked() {
                self.feedback = Some(match session.run_named(APPLY, json!({"job_id": job})) {
                    Ok(_) => "ai.image.subject_applied".into(),
                    Err(error) => error.message_key,
                });
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let selected = &self.workspace_view.selected;
            if ui.add_enabled_ui(!selected.is_empty() && cfg!(windows), |ui| {
                workspace_button(ui, &mut self.icons, "file-save", "저장", false)
            }).inner.clicked()
            {
                #[cfg(windows)]
                if let Err(error) = save_candidates(session, job, selected) {
                    self.feedback = Some(error.to_string());
                }
            }
            let source = session
                .ai_image
                .source()
                .and_then(|id| session.document.get_object(id));
            let replace = !session.ai_image.is_depth_map() && session.ai_image.borders().is_none() && selected.len() == 1
                && source
                    .is_some_and(|obj| {
                        let image = obj.kind == dtool_core::ObjectKind::Image
                            || (obj.kind == dtool_core::ObjectKind::PictureFrame && session.ai_image.supports_frame_replacement());
                        image && !session.document.is_object_locked(obj)
                    });
            if ControlState::enabled(replace)
                .reason("대체할 수 있는 원본과 결과 한 장을 선택하세요.")
                .decorate(ui.add_enabled_ui(replace, |ui| {
                    workspace_button(ui, &mut self.icons, "colour-swap", "원본 레이어 대체", false)
                }).inner)
                .clicked()
                && let Some(&index) = selected.first()
            {
                self.feedback = session
                    .run_named(
                        APPLY,
                        json!({"job_id": job, "index": index, "replace": true}),
                    )
                    .err()
                    .map(|error| error.message_key);
            }
            if super::render_button(ui, "새 레이어 추가", !selected.is_empty()).clicked() {
                let mut selection: Vec<_> = selected.iter().copied().collect();
                if session.ai_image.source().is_some() {
                    selection.reverse();
                }
                for index in selection {
                    if let Err(error) =
                        session.run_named(APPLY, json!({"job_id": job, "index": index}))
                    {
                        self.feedback = Some(error.message_key);
                        break;
                    }
                }
            }
        });
        if let Some(feedback) = &self.feedback {
            ui.label(dtool_strings::text(feedback));
        }
    }
}

#[cfg(windows)]
fn save_candidates(session: &Session, job: u64, selected: &BTreeSet<usize>) -> std::io::Result<()> {
    use std::io::Write;
    let Some(folder) = crate::dialogs::pick_folder_path("AI 결과를 저장할 폴더")? else {
        return Ok(());
    };
    for &index in selected {
        let Some(bytes) = session.ai_image.preview_png(index) else {
            continue;
        };
        let basename = format!("DTool-AI-{job}-{}", index + 1);
        for suffix in 0..1000 {
            let name = if suffix == 0 {
                format!("{basename}.png")
            } else {
                format!("{basename}-{suffix}.png")
            };
            let path = folder.join(name);
            match std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(bytes) {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return Err(error);
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && suffix < 999 => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}
