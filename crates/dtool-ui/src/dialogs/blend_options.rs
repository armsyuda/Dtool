use dtool_core::command::Session;
use dtool_core::layer_style::BlendRanges;
use dtool_core::{Object, ObjectId};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

pub const MODES: [&str; 17] = ["Normal", "PassThrough", "Multiply", "Screen", "Overlay", "Darken", "Lighten", "ColorDodge", "ColorBurn", "HardLight", "SoftLight", "Difference", "Exclusion", "Hue", "Saturation", "Color", "Luminosity"];

pub fn mode_label(mode: &str) -> String { text(&format!("blend.{mode}")).to_string() }

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlendPreview {
    pub id: dtool_core::ObjectId,
    pub mode: dtool_core::blend::BlendMode,
    pub ranges: Option<dtool_core::layer_style::BlendRanges>,
}

#[derive(Debug, Clone)]
pub struct BlendOptionsDialog {
    pub open: bool,
    pub object_id: Option<u64>,
    pub mode: String,
    pub ranges: Option<dtool_core::layer_style::BlendRanges>,
    pub last_error: Option<String>,
    initialized: bool,
    generation: u64,
    selected: Vec<ObjectId>,
    all_groups: bool,
    mixed_ranges: RangeFields,
    changed_ranges: RangeFields,
    replace_ranges: bool,
    disabled_ranges: Option<BlendRanges>,
    source_graph: CurveEditor,
    underlying_graph: CurveEditor,
}

#[derive(Debug, Clone, Default)]
struct RangeFields {
    enabled: bool,
    source: bool,
    underlying: bool,
    gamma: bool,
    antialias: bool,
}

impl RangeFields {
    fn any(&self) -> bool {
        self.enabled || self.source || self.underlying || self.gamma || self.antialias
    }
}

impl Default for BlendOptionsDialog {
    fn default() -> Self { Self {
        open: false, object_id: None, mode: "Normal".into(), ranges: None, last_error: None,
        initialized: false, generation: 0, selected: Vec::new(), all_groups: false,
        mixed_ranges: Default::default(), changed_ranges: Default::default(), replace_ranges: false, disabled_ranges: None,
        source_graph: Default::default(), underlying_graph: Default::default(),
    } }
}
pub fn blend_set_command(id: u64, mode: &str) -> Command { Command::with_params("layer.blend.set", json!({"id": id, "mode": mode})) }
pub(crate) fn can_edit_selection(session: &Session) -> bool {
    session.has_document && !session.selection.ids.is_empty() && session.selection.ids.iter()
        .all(|id| session.document.get_object(*id).is_some_and(|object| !session.document.is_object_locked(object)))
}

impl BlendOptionsDialog {
    pub fn open_for(&mut self, id: u64, mode: &str) { *self = Self::default(); self.open = true; self.object_id = Some(id); self.mode = mode.to_string(); }
    pub(crate) fn open_selection(&mut self, session: &Session) -> bool {
        if !can_edit_selection(session) { return false; }
        let Some(&first) = session.selection.ids.first() else { return false; };
        let Some(object) = session.document.get_object(first) else { return false; };
        self.open_for(first.0, &format!("{:?}", object.blend_mode));
        self.selected = session.selection.ids.clone();
        if !self.load(session) {
            self.cancel();
            return false;
        }
        true
    }
    pub fn set_mode(&mut self, mode: &str) { self.mode = mode.to_string(); }
    fn targets<'a>(&'a self, session: &'a Session) -> impl Iterator<Item = &'a Object> {
        self.selected.iter().filter_map(|id| session.document.get_object(*id))
    }
    fn load(&mut self, session: &Session) -> bool {
        if self.initialized { return true; }
        if self.selected.is_empty() {
            self.selected.extend(self.object_id.map(ObjectId));
        }
        let objects: Vec<_> = self.targets(session).collect();
        let Some(first) = objects.first().filter(|_| objects.len() == self.selected.len()) else {
            self.last_error = Some("error.not_found".into());
            return false;
        };
        let mode_mixed = objects.iter().any(|object| object.blend_mode != first.blend_mode);
        let all_groups = objects.iter().all(|object| matches!(object.kind, dtool_core::ObjectKind::Group));
        let ranges = objects.iter().find_map(|object| object.blend_ranges.clone());
        let defaults = default_ranges();
        let reference = ranges.as_ref().unwrap_or(&defaults);
        let mut mixed = RangeFields::default();
        for object in objects {
            let actual = object.blend_ranges.as_ref();
            mixed.enabled |= actual.is_some() != ranges.is_some();
            let actual = actual.unwrap_or(&defaults);
            mixed.source |= actual.source != reference.source;
            mixed.underlying |= actual.underlying != reference.underlying;
            mixed.gamma |= actual.gamma != reference.gamma;
            mixed.antialias |= actual.antialias != reference.antialias;
        }
        if mode_mixed { self.mode.clear(); }
        self.all_groups = all_groups;
        self.mixed_ranges = mixed;
        self.ranges = ranges;
        self.generation = session.generation;
        self.initialized = true;
        true
    }
    pub fn set_ranges(&mut self, session: &Session, ranges: Option<dtool_core::layer_style::BlendRanges>) -> bool {
        if ranges.as_ref().is_some_and(|r| !r.is_valid()) || !self.load(session) { self.last_error = Some("error.invalid_params".into()); return false; }
        self.ranges = ranges; self.replace_ranges = true; self.last_error = None; true
    }
    fn target_ranges(&self, current: &Option<BlendRanges>) -> Option<BlendRanges> {
        if self.replace_ranges {
            return self.ranges.clone();
        }
        let changed = &self.changed_ranges;
        if !changed.any() {
            return current.clone();
        }
        let draft = self.ranges.as_ref()?;
        let mut ranges = current.clone().unwrap_or_else(default_ranges);
        if changed.source { ranges.source.clone_from(&draft.source); }
        if changed.underlying { ranges.underlying.clone_from(&draft.underlying); }
        if changed.gamma { ranges.gamma = draft.gamma; }
        if changed.antialias { ranges.antialias = draft.antialias; }
        Some(ranges)
    }

    fn changes(&self, session: &Session) -> Result<Vec<BlendPreview>, &'static str> {
        if self.generation != session.generation { return Err("error.stale"); }
        let mode = if self.mode.is_empty() { None } else {
            Some(serde_json::from_value(serde_json::Value::String(self.mode.clone())).map_err(|_| "error.invalid_params")?)
        };
        let mut changes = Vec::new();
        for &id in &self.selected {
            let object = session.document.get_object(id).ok_or("error.not_found")?;
            if session.document.is_object_locked(object) { return Err("error.locked"); }
            let mode = mode.unwrap_or(object.blend_mode);
            let ranges = self.target_ranges(&object.blend_ranges);
            if object.blend_mode != mode || object.blend_ranges != ranges {
                changes.push(BlendPreview { id, mode, ranges });
            }
        }
        Ok(changes)
    }

    pub(crate) fn preview(&self, session: &Session) -> Option<Vec<BlendPreview>> {
        if !self.open || !self.initialized {
            return None;
        }
        self.changes(session).ok().filter(|changes| !changes.is_empty())
    }

    pub fn apply(&mut self, session: &mut Session) -> bool {
        if !self.load(session) { return false; }
        let changes = match self.changes(session) {
            Ok(changes) => changes,
            Err(key) => { self.last_error = Some(key.into()); return false; }
        };
        let mut commands = Vec::new();
        for change in changes {
            let object = session.document.get_object(change.id).expect("validated blend target");
            if object.blend_mode != change.mode { commands.push(blend_set_command(change.id.0, &format!("{:?}", change.mode))); }
            if object.blend_ranges != change.ranges {
                commands.push(Command::with_params("layer.blend_ranges.set", json!({"id": change.id.0, "ranges": change.ranges})));
            }
        }
        let result = match commands.len() { 0 => Ok(json!({"changes": []})), 1 => session.run(&commands[0]), _ => session.run(&Command::with_params("edit.batch", json!({"commands": commands}))) };
        self.last_error = result.as_ref().err().map(|e| e.message_key.clone()); if result.is_ok() { self.open = false; } result.is_ok()
    }
    pub fn cancel(&mut self) { self.open = false; }
    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) {
        use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};
        if !self.open || !self.load(session) {
            return;
        }
        let width = (ctx.content_rect().width() - 48.0).clamp(340.0, 560.0);
        let graph_height = (ctx.content_rect().height() * 0.32).clamp(110.0, 220.0);
        let mut editing_number = false;
        let (result, clicked) = show_modal(
            ctx, "dtool.dialog.blend_options", text("menu.layer.blend_options"), ClosePolicy::Free, true, |ui| {
                ui.set_width(width);
                ui.columns(2, |columns| {
                    let ui = &mut columns[0];
                    dtool_design::studio_row(ui, text("layers.blend"), |ui, label| {
                        let shown = if self.mode.is_empty() { dtool_design::MIXED_TEXT.into() } else { mode_label(&self.mode) };
                        let all_groups = self.all_groups;
                        egui::ComboBox::from_id_salt("layer.blend_mode").selected_text(shown).show_ui(ui, |ui| {
                            for mode in MODES.iter().copied().filter(|mode| *mode != "PassThrough" || all_groups) {
                                if ui.selectable_label(self.mode == mode, mode_label(mode)).clicked() {
                                    self.set_mode(mode);
                                }
                            }
                        }).response.labelled_by(label);
                    });
                    let mut enabled = self.ranges.is_some();
                    let mixed = self.mixed_ranges.enabled && !self.changed_ranges.enabled;
                    if dtool_design::toggle_with(ui, &mut enabled, text("fx.ranges.enabled"), &dtool_design::ControlState::default().mixed(mixed)).clicked() {
                        if enabled {
                            if self.ranges.is_none() {
                                self.ranges = Some(self.disabled_ranges.take().unwrap_or_else(default_ranges));
                            }
                        } else {
                            self.disabled_ranges = self.ranges.take();
                        }
                        self.changed_ranges.enabled = true;
                    }
                    if let Some(ranges) = &mut self.ranges {
                        let ui = &mut columns[1];
                        let gamma_mixed = self.mixed_ranges.gamma && !self.changed_ranges.gamma;
                        let (editing, changed) = number(ui, "fx.ranges.gamma", &mut ranges.gamma, 0.001..=1000.0, "", gamma_mixed);
                        editing_number |= editing;
                        self.changed_ranges.gamma |= changed;
                        let antialias_mixed = self.mixed_ranges.antialias && !self.changed_ranges.antialias;
                        if dtool_design::toggle_with(ui, &mut ranges.antialias, text("fx.ranges.antialias"), &dtool_design::ControlState::default().mixed(antialias_mixed)).clicked() {
                            self.changed_ranges.antialias = true;
                        }
                    }
                });
                ui.separator();

                let mut default = default_ranges();
                let enabled = self.ranges.is_some();
                let ranges = self.ranges.as_mut().unwrap_or(&mut default);
                ui.columns(2, |columns| {
                    for (ui, (key, editor, points, mixed, changed)) in columns.iter_mut().zip([
                        ("fx.ranges.source", &mut self.source_graph, &mut ranges.source, self.mixed_ranges.source, &mut self.changed_ranges.source),
                        ("fx.ranges.underlying", &mut self.underlying_graph, &mut ranges.underlying, self.mixed_ranges.underlying, &mut self.changed_ranges.underlying),
                    ]) {
                        let (editing, curve_changed) = ui.add_enabled_ui(enabled, |ui| {
                            editor.show(ui, key, points, graph_height, mixed && !*changed)
                        }).inner;
                        editing_number |= editing;
                        *changed |= curve_changed;
                    }
                });

                if let Some(key) = &self.last_error {
                    ui.colored_label(ui.visuals().error_fg_color, text(key));
                }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if dtool_design::primary_button(ui, text("dialog.ok"), egui::vec2(72.0, 22.0)).clicked() {
                        DialogResult::Confirm
                    } else if dtool_design::button(ui, text("dialog.cancel")).clicked() {
                        DialogResult::Cancel
                    } else {
                        DialogResult::Open
                    }
                }).inner
            },
        );
        match (clicked, result) {
            (DialogResult::Confirm, _) => { self.apply(session); }
            (DialogResult::Cancel, _) | (_, DialogResult::Cancel) => self.cancel(),
            (_, DialogResult::Confirm) if !editing_number => { self.apply(session); }
            _ => {}
        }
    }
}

fn flat_curve() -> Vec<[f32; 2]> {
    vec![[0.0, 1.0], [1.0, 1.0]]
}

fn default_ranges() -> dtool_core::layer_style::BlendRanges {
    dtool_core::layer_style::BlendRanges {
        source: flat_curve(),
        underlying: flat_curve(),
        gamma: 1.0,
        antialias: false,
    }
}

fn number(ui: &mut egui::Ui, key: &str, value: &mut f32, range: std::ops::RangeInclusive<f64>, suffix: &str, mixed: bool) -> (bool, bool) {
    dtool_design::studio_row(ui, text(key), |ui, label| {
        let spec = dtool_design::NumericSpec::new("").range(range).width(62.0).suffix(suffix);
        let shown = if mixed { dtool_design::NumericValue::Mixed } else { dtool_design::NumericValue::Value(f64::from(*value)) };
        let (response, event) = dtool_design::numeric_field(ui, ui.make_persistent_id(key), shown, &spec);
        let response = response.labelled_by(label);
        if let dtool_design::NumericEvent::Commit(new) = event {
            *value = new as f32;
        }
        let committed = matches!(event, dtool_design::NumericEvent::Commit(_));
        (response.has_focus() || response.lost_focus() || committed, committed)
    })
}

#[derive(Debug, Clone, Default)]
struct CurveEditor {
    selected: usize,
    dragging: bool,
}

impl CurveEditor {
    fn insert(&mut self, points: &mut Vec<[f32; 2]>, point: [f32; 2]) {
        self.selected = points.partition_point(|existing| existing[0] <= point[0]);
        points.insert(self.selected, point);
    }

    fn add_midpoint(&mut self, points: &mut Vec<[f32; 2]>) {
        let point = points.windows(2)
            .max_by(|a, b| (a[1][0] - a[0][0]).total_cmp(&(b[1][0] - b[0][0])))
            .map_or([0.5, 0.5], |pair| [
                (pair[0][0] + pair[1][0]) * 0.5,
                (pair[0][1] + pair[1][1]) * 0.5,
            ]);
        self.insert(points, point);
    }

    fn move_point(&self, points: &mut [[f32; 2]], point: [f32; 2]) {
        let left = self.selected.checked_sub(1).map_or(0.0, |index| points[index][0]);
        let right = points.get(self.selected + 1).map_or(1.0, |next| next[0]);
        points[self.selected] = [point[0].clamp(left.min(right), left.max(right)), point[1].clamp(0.0, 1.0)];
    }

    fn show(&mut self, ui: &mut egui::Ui, key: &str, points: &mut Vec<[f32; 2]>, height: f32, mixed: bool) -> (bool, bool) {
        ui.push_id(key, |ui| {
            let heading = if mixed { format!("{} · {}", text(key), dtool_design::MIXED_TEXT) } else { text(key).to_string() };
            let label = ui.label(heading);
            if mixed { label.on_hover_text(text("fx.ranges.mixed_curve")); }
            let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click_and_drag());
            let graph = rect.shrink(7.0);
            let at = |point: [f32; 2]| egui::pos2(graph.left() + point[0] * graph.width(), graph.bottom() - point[1] * graph.height());
            let to_point = |pos: egui::Pos2| [
                ((pos.x - graph.left()) / graph.width()).clamp(0.0, 1.0),
                ((graph.bottom() - pos.y) / graph.height()).clamp(0.0, 1.0),
            ];
            self.selected = self.selected.min(points.len().saturating_sub(1));
            let painter = ui.painter();
            let visuals = ui.visuals();
            painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
            let grid = egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);
            painter.rect_stroke(graph, 0.0, grid, egui::StrokeKind::Inside);
            for quarter in [0.25, 0.5, 0.75] {
                painter.vline(at([quarter, 0.0]).x, graph.y_range(), grid);
                painter.hline(graph.x_range(), at([0.0, quarter]).y, grid);
            }
            let line = egui::Stroke::new(1.5, visuals.widgets.noninteractive.fg_stroke.color);
            for pair in points.windows(2) {
                painter.line_segment([at(pair[0]), at(pair[1])], line);
            }
            for (index, point) in points.iter().copied().enumerate() {
                let selected = self.selected == index;
                let colour = if selected { visuals.selection.bg_fill } else { line.color };
                painter.circle_filled(at(point), if selected { 4.5 } else { 3.0 }, colour);
            }

            let mut editing = false;
            let mut changed = false;
            if let Some(point) = points.get(self.selected).copied() {
                let mut percent = point.map(|value| value * 100.0);
                ui.push_id(self.selected, |ui| {
                    for (axis, key) in [(0, "fx.ranges.input"), (1, "fx.ranges.output")] {
                        let (field_editing, committed) = number(ui, key, &mut percent[axis], 0.0..=100.0, "%", false);
                        editing |= field_editing;
                        changed |= committed;
                    }
                });
                let next = percent.map(|value| value / 100.0);
                if percent != point.map(|value| value * 100.0) {
                    self.move_point(points, next);
                }
            }
            if response.enabled() {
                let (press, release) = crate::input::primary_button_positions(ui);
                if let Some(pos) = press.filter(|pos| response.interact_rect.contains(*pos)) {
                    let hit = points.iter().enumerate()
                        .map(|(index, point)| (index, at(*point).distance(pos)))
                        .filter(|(_, distance)| *distance <= 9.0)
                        .min_by(|(_, a), (_, b)| a.total_cmp(b));
                    self.dragging = hit.is_some();
                    if let Some((index, _)) = hit {
                        self.selected = index;
                    }
                }
                let pointer = release.or_else(|| ui.input(|input| input.pointer.latest_pos()));
                let moved = response.dragged() || response.drag_stopped()
                    || press.zip(release).is_some_and(|(start, end)| start.distance(end) > 2.0);
                if self.dragging && moved && self.selected < points.len() && let Some(pos) = pointer {
                    let previous = points[self.selected];
                    self.move_point(points, to_point(pos));
                    changed |= points[self.selected] != previous;
                } else if !self.dragging && response.clicked()
                    && let Some(pos) = response.interact_pointer_pos().filter(|pos| graph.contains(*pos))
                {
                    self.insert(points, to_point(pos));
                    changed = true;
                }
                if release.is_some() || !ui.input(|input| input.pointer.primary_down()) {
                    self.dragging = false;
                }
            }

            ui.horizontal_wrapped(|ui| {
                if dtool_design::button(ui, text("fx.ranges.add")).clicked() {
                    self.add_midpoint(points);
                    changed = true;
                }
                if dtool_design::button_with(ui, text("fx.ranges.remove"), &dtool_design::ControlState::enabled(points.len() > 2)).clicked() {
                    points.remove(self.selected);
                    self.selected = self.selected.min(points.len().saturating_sub(1));
                    changed = true;
                }
                if dtool_design::button(ui, text("pref.reset")).clicked() {
                    *points = flat_curve();
                    *self = Self::default();
                    changed = true;
                }
            });
            (editing, changed)
        }).inner
    }
}
