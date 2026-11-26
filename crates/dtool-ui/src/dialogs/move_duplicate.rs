use dtool_core::clipboard::move_duplicate::{self, MAX_COPIES, MOVE_DUPLICATE, MoveDuplicate};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::document::Units;
use dtool_core::{Command, ObjectId, Session};
use dtool_design::{NumericSpec, numeric_cancel_edit, numeric_state, numeric_value};
use dtool_strings::text;

pub struct MoveDuplicateDialog {
    pub owner: ContextId,
    target: Vec<ObjectId>,
    tool: String,
    settings: Fields,
    previous: Option<Fields>,
    displayed_per_unit: f64,
    suffix: &'static str,
    origin: Option<[f32; 2]>,
    origin_drag_offset: Option<[f32; 2]>,
    preview: Option<Session>,
    preview_key: Option<(u64, MoveDuplicate, Option<[f32; 2]>)>,
    canvas: crate::app_canvas::AppCanvas,
    icons: crate::icon_cache::IconCache,
    error: Option<String>,
}

#[derive(Clone, Copy)]
struct Fields {
    offsets: [f64; 2],
    angle: f64,
    rotation: f64,
    scale: [f64; 2],
    duplicate: bool,
    copies: f64,
    behind: bool,
}

impl Default for Fields {
    fn default() -> Self {
        Self { offsets: [0.0; 2], angle: 0.0, rotation: 0.0, scale: [100.0; 2],
            duplicate: false, copies: 1.0, behind: false }
    }
}

const WINDOW_ID: &str = "dtool.move_duplicate";
const NUMBER_FIELDS: [&str; 8] = [
    "move_duplicate.horizontal", "move_duplicate.vertical", "move_duplicate.distance", "move_duplicate.angle",
    "move_duplicate.rotation", "move_duplicate.scale_x", "move_duplicate.scale_y", "move_duplicate.count",
];

fn clear_fields(ctx: &egui::Context) {
    for key in NUMBER_FIELDS { numeric_cancel_edit(ctx, egui::Id::unique((WINDOW_ID, key))); }
}

impl MoveDuplicateDialog {
    pub fn open(workspace: &Workspace, ctx: &egui::Context) -> Option<Self> {
        let session = workspace.session()?;
        if !move_duplicate::available(session) || session.text_edit.is_some() {
            return None;
        }
        clear_fields(ctx);
        let doc = &session.document;
        let suffix = if doc.rulers.meters { "m" } else {
            dtool_core::units::suffix(doc.rulers.effective_units(doc.units))
        };
        let displayed_per_unit = f64::from(dtool_core::commands::ruler_units::display_scale(session));
        let display_per_mm = displayed_per_unit
            * f64::from(dtool_core::doc_setup::unit_factor(Units::Mm, doc.dpi, doc.units, doc.dpi));
        let previous = ctx.data(|data| data.get_temp::<Fields>(egui::Id::unique(WINDOW_ID))).map(|mut fields| {
            fields.offsets = fields.offsets.map(|mm| mm * display_per_mm);
            fields
        });
        Some(Self {
            owner: workspace.active()?,
            target: session.selection.ids.clone(),
            tool: session.active_tool.clone(),
            settings: Fields::default(),
            previous,
            displayed_per_unit,
            suffix,
            origin: None,
            origin_drag_offset: None,
            preview: None,
            preview_key: None,
            canvas: crate::app_canvas::AppCanvas::new(),
            icons: crate::icon_cache::IconCache::new(),
            error: None,
        })
    }

    pub fn matches(&self, workspace: &Workspace) -> bool {
        workspace.active() == Some(self.owner) && workspace.session().is_some_and(|session| {
            session.active_tool == self.tool && session.selection.ids == self.target
                && move_duplicate::available(session)
        })
    }

    fn settings(&self) -> MoveDuplicate {
        MoveDuplicate {
            translate: self.settings.offsets.map(|offset| (offset / self.displayed_per_unit) as f32),
            rotation_deg: self.settings.rotation as f32,
            scale: self.settings.scale.map(|percent| (percent / 100.0) as f32),
            copies: if self.settings.duplicate { self.settings.copies as u16 } else { 0 },
            behind: self.settings.behind,
            origin: self.origin,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &Session) -> (bool, Option<Command>) {
        let anchor = ctx.content_rect().right_top() + egui::vec2(-326.0, 112.0);
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(text("move_duplicate.dialog"))
            .id(egui::Id::unique(WINDOW_ID)).open(&mut open).default_pos(anchor)
            .resizable(false).collapsible(false)
            .show(ctx, |ui| {
                ui.set_min_width(294.0);
                dtool_design::button_visuals(ui);
                if let Some(previous) = self.previous
                    && dtool_design::button(ui, text("move_duplicate.previous")).clicked()
                {
                    self.settings = previous;
                }
                let valid = self.fields(ui);
                if valid { self.refresh_preview(session); }
                if let Some(key) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(key));
                }
                crate::dialog_chrome::options_divider(ui);
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("move_duplicate.reset")).clicked() {
                        self.settings = Fields::default();
                        self.origin = None;
                        self.origin_drag_offset = None;
                        clear_fields(ctx);
                        self.refresh_preview(session);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        cancel = dtool_design::button(ui, text("dialog.cancel")).clicked();
                        let can_apply = self.settings().changes_document() || (self.target.len() == 1 && self.origin.is_some());
                        apply = ui.add_enabled_ui(valid && self.error.is_none() && can_apply, |ui| {
                            dtool_design::primary_button(ui, text("dialog.ok"), egui::vec2(68.0, 24.0)).clicked()
                        }).inner;
                    });
                });
            });
        let cancel = cancel || !open || ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let command = (apply && !cancel).then(|| {
            Command::with_params(MOVE_DUPLICATE, serde_json::to_value(self.settings()).unwrap_or_default())
        });
        if cancel || command.is_some() {
            clear_fields(ctx);
            let doc = &session.document;
            let mm_per_display = f64::from(dtool_core::doc_setup::unit_factor(doc.units, doc.dpi, Units::Mm, doc.dpi))
                / self.displayed_per_unit;
            let mut remembered = self.settings;
            remembered.offsets = remembered.offsets.map(|value| value * mm_per_display);
            ctx.data_mut(|data| data.insert_temp(egui::Id::unique(WINDOW_ID), remembered));
        }
        (cancel, command)
    }

    fn fields(&mut self, ui: &mut egui::Ui) -> bool {
        let settings = &mut self.settings;
        let mut valid = true;
        let mut length = |ui: &mut egui::Ui, key: &'static str, value: &mut f64| {
            number(ui, key, value, self.suffix, -1.0e7..=1.0e7, 3, &mut valid)
        };
        crate::dialog_chrome::options_group(ui, text("move_duplicate.offset"));
        let horizontal = length(ui, "move_duplicate.horizontal", &mut settings.offsets[0]);
        let vertical = length(ui, "move_duplicate.vertical", &mut settings.offsets[1]);
        let mut distance = settings.offsets[0].hypot(settings.offsets[1]);
        if (horizontal || vertical) && distance > 0.0 {
            settings.angle = settings.offsets[1].atan2(settings.offsets[0]).to_degrees();
        }
        let polar_distance = number(ui, "move_duplicate.distance", &mut distance, self.suffix, 0.0..=1.0e7, 3, &mut valid);
        let angle = number(ui, "move_duplicate.angle", &mut settings.angle, "°", -360.0..=360.0, 2, &mut valid);
        if polar_distance || angle {
            let (sin, cos) = settings.angle.to_radians().sin_cos();
            settings.offsets = [distance * cos, distance * sin].map(|value| if value.abs() < 1e-9 { 0.0 } else { value });
        }
        crate::dialog_chrome::options_group(ui, text("move_duplicate.transform"));
        number(ui, "move_duplicate.rotation", &mut settings.rotation, "°", -360.0..=360.0, 2, &mut valid);
        number(ui, "move_duplicate.scale_x", &mut settings.scale[0], "%", 0.01..=10_000.0, 2, &mut valid);
        number(ui, "move_duplicate.scale_y", &mut settings.scale[1], "%", 0.01..=10_000.0, 2, &mut valid);
        ui.label(egui::RichText::new(text("move_duplicate.origin_hint")).small().weak());
        crate::dialog_chrome::options_divider(ui);
        dtool_design::toggle(ui, &mut settings.duplicate, text("move_duplicate.copy"));
        ui.add_enabled_ui(settings.duplicate, |ui| {
            number(ui, "move_duplicate.count", &mut settings.copies, "", 1.0..=f64::from(MAX_COPIES), 0, &mut valid);
            settings.copies = settings.copies.round();
            ui.horizontal(|ui| {
                ui.label(text("move_duplicate.placement"));
                if self.icons.studio_button(ui, "cmd-arrange-front", text("move_duplicate.in_front"), !settings.behind).clicked() {
                    settings.behind = false;
                }
                if self.icons.studio_button(ui, "cmd-arrange-back", text("move_duplicate.behind"), settings.behind).clicked() {
                    settings.behind = true;
                }
            });
        });
        valid
    }

    fn refresh_preview(&mut self, session: &Session) {
        let settings = self.settings();
        let key = (session.generation, settings, self.origin.or_else(|| move_duplicate::default_origin(session)));
        if self.preview_key == Some(key) {
            return;
        }
        self.preview_key = Some(key);
        self.error = None;
        self.preview = None;
        if settings.changes_document() || self.origin.is_some() {
            match settings.preview(session) {
                Ok(document) => {
                    let mut draft = Session::new(document);
                    draft.context = session.context;
                    self.canvas.invalidate();
                    self.preview = Some(draft);
                }
                Err(error) => self.error = Some(error.message_key),
            }
        }
    }

    pub fn update_origin(&mut self, ui: &egui::Ui, canvas: &egui::Response, active: &Session) {
        let Some(origin) = self.origin.or_else(|| move_duplicate::default_origin(active)) else { return };
        let at = origin_screen(canvas.rect, &active.view, origin);
        let to_doc = |point: egui::Pos2| {
            let local = point - canvas.rect.min;
            dtool_core::view_to_doc(&active.view, [local.x, local.y])
        };
        let (press, release) = crate::input::primary_button_positions(ui);
        let (pointer, held) = ui.input(|input| (input.pointer.latest_pos(), input.pointer.primary_down()));
        if self.origin_drag_offset.is_none()
            && let Some(point) = press.filter(|point| point.distance(at) <= 8.0
                && canvas.interact_rect.contains(*point) && canvas.enabled()
                && ui.ctx().layer_id_at(*point) == Some(canvas.layer_id) && !egui::Popup::is_any_open(ui.ctx()))
        {
            let doc = to_doc(point);
            self.origin_drag_offset = Some([origin[0] - doc[0], origin[1] - doc[1]]);
        }
        if let Some(offset) = self.origin_drag_offset {
            if let Some(point) = release.or(pointer) {
                let doc = to_doc(point);
                let position = [doc[0] + offset[0], doc[1] + offset[1]];
                if position != origin && position.iter().all(|value| value.is_finite()) {
                    self.origin = Some(position);
                    ui.ctx().request_repaint();
                }
            }
            if release.is_some() || !held {
                self.origin_drag_offset = None;
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        } else if canvas.hovered() && pointer.is_some_and(|point| point.distance(at) <= 8.0) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    pub fn paint_origin(&self, ui: &egui::Ui, rect: egui::Rect, active: &Session) {
        let Some(origin) = self.origin.or_else(|| move_duplicate::default_origin(active)) else { return };
        let at = origin_screen(rect, &active.view, origin);
        let painter = ui.painter_at(rect);
        painter.circle_filled(at, 6.0, egui::Color32::from_black_alpha(220));
        crate::context_bar::paint_transform_origin(&painter, at, crate::canvas_input::SELECTION_BLUE, 4.0);
    }

    pub fn paint_preview(&mut self, ui: &egui::Ui, rect: egui::Rect, active: &Session) -> bool {
        let Some(draft) = &mut self.preview else { return false };
        draft.view = active.view.clone();
        self.canvas.paint(ui, rect, draft);
        true
    }
}

fn origin_screen(rect: egui::Rect, view: &dtool_core::ViewState, point: [f32; 2]) -> egui::Pos2 {
    let [x, y] = dtool_core::doc_to_view(view, point);
    rect.min + egui::vec2(x, y)
}

fn number(ui: &mut egui::Ui, key: &'static str, value: &mut f64, suffix: &str,
    range: std::ops::RangeInclusive<f64>, decimals: usize, valid: &mut bool,
) -> bool {
    let mut spec = NumericSpec::new(text(key)).range(range).decimals(decimals)
        .width(96.0).label_width(116.0).enabled(ui.is_enabled());
    if !suffix.is_empty() { spec = spec.suffix(suffix); }
    let id = egui::Id::unique((WINDOW_ID, key));
    let previous = *value;
    numeric_value(ui, id, value, &spec);
    let state = numeric_state(ui.ctx(), id);
    if ui.is_enabled() && state.edited && let Some(buffer) = state.buffer {
        if let Some(typed) = spec.parse_text(&buffer) {
            *value = typed;
        } else {
            *valid = false;
        }
    }
    *value != previous
}
