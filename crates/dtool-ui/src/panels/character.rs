use crate::context::text_bar::{BOLD_WEIGHT, REGULAR_WEIGHT};
use dtool_core::Session;
use dtool_design::{numeric_field, panel_frame, panel_header, NumericEvent, NumericSpec, NumericValue};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use dtool_tools::dtool_text::char_style::{baseline_lower, baseline_raise, tracking_loosen, tracking_tighten};
use serde_json::{json, Map, Value};

pub const STYLE_COMMAND: &str = "text.style";
pub const BATCH_COMMAND: &str = "edit.batch";
pub const POSITION_NORMAL: &str = "Normal";
pub const POSITION_SUPER: &str = "Superscript";
pub const POSITION_SUB: &str = "Subscript";

pub(crate) fn batched(mut commands: Vec<Command>) -> Option<Command> {
    match commands.len() {
        0 => None,
        1 => commands.pop(),
        _ => Some(Command::with_params(BATCH_COMMAND, json!({ "commands": commands }))),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Same(Value),
    Mixed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub style: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharSelection {
    pub story: u64,
    pub start: usize,
    pub end: usize,
    pub segments: Vec<Segment>,
}

fn bound(run: &Value, key: &str) -> usize {
    run.get(key).and_then(Value::as_u64).unwrap_or(0) as usize
}

pub fn segments(ranges: &[Value], start: usize, end: usize) -> Vec<Segment> {
    let mut cuts = vec![start, end];
    for run in ranges.iter().filter(|run| run.is_object()) {
        for at in [bound(run, "start"), bound(run, "end")] {
            if at > start && at < end {
                cuts.push(at);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter(|pair| pair[0] < pair[1])
        .map(|pair| {
            let mut style = Map::new();
            for run in ranges.iter().filter(|run| run.is_object() && bound(run, "start") <= pair[0] && bound(run, "end") >= pair[1]) {
                for (key, value) in run.as_object().into_iter().flatten() {
                    if key != "start" && key != "end" {
                        style.insert(key.clone(), value.clone());
                    }
                }
            }
            Segment { start: pair[0], end: pair[1], style }
        })
        .collect()
}

pub fn active_range(session: &Session) -> Option<(u64, usize, usize)> {
    let story = session.active_story_id()?;
    let len = session.document.stories.iter().find(|s| s.id == story)?.text.chars().count();
    match &session.text_edit {
        Some(edit) if edit.story == story => Some((story.0, edit.anchor.min(edit.caret).min(len), edit.anchor.max(edit.caret).min(len))),
        _ => Some((story.0, 0, len)),
    }
}

pub fn selection_at(session: &Session, story: u64, start: usize, end: usize) -> Option<CharSelection> {
    let found = session.document.stories.iter().find(|s| s.id.0 == story)?;
    let (low, high) = if start < end { (start, end) } else { (start.saturating_sub(1), start.max(1)) };
    let segments = if found.text.is_empty() {
        let style = found.ranges.iter().rev().find(|range| range.get("frame").is_none()).and_then(Value::as_object).cloned().unwrap_or_default();
        vec![Segment { start: 0, end: 0, style }]
    } else {
        segments(&found.ranges, low, high)
    };
    Some(CharSelection { story, start, end, segments })
}

pub fn read_selection(session: &Session) -> Option<CharSelection> {
    let (story, start, end) = active_range(session)?;
    selection_at(session, story, start, end)
}

impl CharSelection {
    fn can_style(&self) -> bool {
        self.start < self.end || self.segments.first().is_some_and(|segment| segment.start == segment.end)
    }
    pub fn field(&self, key: &str) -> Field {
        let mut values = self.segments.iter().map(|segment| segment.style.get(key).cloned().unwrap_or(Value::Null));
        let Some(first) = values.next() else { return Field::Same(Value::Null) };
        if values.all(|value| value == first) {
            Field::Same(first)
        } else {
            Field::Mixed
        }
    }

    pub fn number(&self, key: &str, fallback: f64) -> Option<f64> {
        match self.field(key) {
            Field::Same(value) => Some(value.as_f64().unwrap_or(fallback)),
            Field::Mixed => None,
        }
    }

    pub fn text_value(&self, key: &str) -> Option<String> {
        match self.field(key) {
            Field::Same(value) => Some(value.as_str().unwrap_or("").to_string()),
            Field::Mixed => None,
        }
    }

    pub fn decoration_on(&self, key: &str) -> Option<bool> {
        match self.field(key) {
            Field::Same(value) => Some(value.get("on").and_then(Value::as_bool).unwrap_or(false)),
            Field::Mixed => {
                let mut states = self.segments.iter().map(|s| s.style.get(key).and_then(|d| d.get("on")).and_then(Value::as_bool).unwrap_or(false));
                let first = states.next()?;
                states.all(|on| on == first).then_some(first)
            }
        }
    }

    fn style_command(&self, start: usize, end: usize, patch: Value) -> Command {
        Command::with_params(STYLE_COMMAND, json!({ "story": self.story, "range": { "start": start, "end": end }, "patch": patch }))
    }

    pub fn set(&self, key: &str, value: Value) -> Option<Command> {
        self.can_style().then(|| self.style_command(self.start, self.end, json!({ key: value })))
    }

    pub fn per_segment(&self, key: &str, change: impl Fn(Option<&Value>) -> Option<Value>) -> Option<Command> {
        batched(self.segment_commands(key, change))
    }

    fn segment_commands(&self, key: &str, change: impl Fn(Option<&Value>) -> Option<Value>) -> Vec<Command> {
        if !self.can_style() {
            return Vec::new();
        }
        let mut commands: Vec<Command> = Vec::new();
        for segment in &self.segments {
            let current = segment.style.get(key);
            if let Some(next) = change(current) {
                if Some(&next) != current {
                    commands.push(self.style_command(segment.start, segment.end, json!({ key: next })));
                }
            }
        }
        commands
    }

    pub fn nudge(&self, key: &str, step: fn(f32) -> f32) -> Option<Command> {
        self.per_segment(key, |current| {
            let value = current.and_then(Value::as_f64).unwrap_or(0.0) as f32;
            Some(json!(step(value)))
        })
    }

    pub fn decoration(&self, key: &str, on: bool) -> Option<Command> {
        self.per_segment(key, |current| {
            let mut next = current.filter(|value| value.is_object()).cloned().unwrap_or_else(|| json!({}));
            next["on"] = json!(on);
            Some(next)
        })
    }

    pub fn toggle_position(&self, target: &str) -> Option<Command> {
        let next = if self.field("position") == Field::Same(json!(target)) { POSITION_NORMAL } else { target };
        self.set("position", json!(next))
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Result<Value, CommandError> {
    session.run(command)
}

#[derive(Default)]
pub struct CharacterPanel {
    pub selection: Option<CharSelection>,
    pub error: Option<String>,
    fonts: crate::font_combo::FontComboState,
    icons: crate::icon_cache::IconCache,
    numeric_preview: Option<Command>,
    previewing_number: bool,
    cancel_number: bool,
}

fn icon(ui: &mut egui::Ui, symbol: &str, key: &str, active: bool) -> bool {
    let label = text(key);
    let response = ui.selectable_label(active, symbol).on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), active, label));
    response.clicked()
}

fn panel_width(ui: &egui::Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x)
}

fn field_row<R>(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui, egui::Id) -> R) -> R {
    if panel_width(ui) < 190.0 {
        let caption = ui.add(dtool_design::numeric_caption(label));
        control(ui, caption.id)
    } else {
        ui.horizontal(|ui| {
            let caption_width = 76.0;
            let caption = ui.allocate_ui_with_layout(
                egui::vec2(caption_width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(caption_width);
                    ui.add(dtool_design::numeric_caption(label).truncate()).on_hover_text(label)
                },
            ).inner;
            control(ui, caption.id)
        }).inner
    }
}

fn font_row<R>(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    field_row(ui, label, |ui, _| {
        ui.scope(|ui| {
            ui.spacing_mut().combo_width = panel_width(ui).clamp(24.0, 196.0);
            control(ui)
        }).inner
    })
}

fn number_field(ui: &mut egui::Ui, key: &str, label: egui::Id, value: Option<f64>, unit: &str, default: f64) -> NumericEvent {
    let display = value.map_or(NumericValue::Mixed, NumericValue::Value);
    let range = if key == "char.size" { 1.0..=1296.0 } else { -10000.0..=10000.0 };
    let spec = NumericSpec::new("").width(58.0).suffix(unit).range(range);
    let (response, event) = numeric_field(ui, ui.make_persistent_id(key), display, &spec);
    response.labelled_by(label);
    if ui.is_enabled() && ui.ctx().read_response(label).is_some_and(|response| response.double_clicked()) {
        NumericEvent::Commit(default)
    } else {
        event
    }
}

impl CharacterPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::Workspace) {
        panel_frame(ui, |ui| {
            panel_header(ui, text("char.title"));
            let targets: Vec<_> = workspace.session().filter(|session| session.text_edit.is_none()).map(|session| {
                session.selected_text_stories().into_iter().filter_map(|id| {
                    let story = session.document.stories.iter().find(|story| story.id == id)?;
                    selection_at(session, id.0, 0, story.text.chars().count())
                }).collect()
            }).unwrap_or_default();
            self.selection = if let Some(first) = targets.first() {
                let mut combined = first.clone();
                combined.segments.extend(targets.iter().skip(1).flat_map(|target| target.segments.iter().cloned()));
                Some(combined)
            } else {
                workspace.session().and_then(read_selection)
            };
            let Some(selection) = self.selection.clone() else {
                ui.add_enabled_ui(false, |ui| { self.edits(ui, &CharSelection::default()); }).response.on_disabled_hover_text(text("char.no_story"));
                return;
            };
            let Some(session) = workspace.session_mut() else { return; };
            let targets = if targets.is_empty() { std::slice::from_ref(&selection) } else { &targets };
            let scales: Vec<_> = targets.iter().map(|target| {
                crate::context::text_bar::font_size_scale(session, dtool_core::StoryId(target.story))
            }).collect();
            let command = self.edits_for(ui, &selection, targets, &scales);
            use dtool_tools::dtool_text::font_preview::{cancel_style_preview, set_style_preview};
            if self.previewing_number && (self.cancel_number || command.is_some()
                || !ui.input(|input| input.pointer.primary_down()))
            {
                cancel_style_preview();
                self.previewing_number = false;
            }
            if let Some(preview) = self.numeric_preview.take() {
                match set_style_preview(&session.document, &preview) {
                    Ok(()) => self.previewing_number = true,
                    Err(error) => self.error = Some(text(&error.message_key).to_string()),
                }
            }
            if let Some(command) = command {
                self.error = apply(session, &command).err().map(|error| text(&error.message_key).to_string());
            }
            if let Some(message) = &self.error {
                ui.label(message.as_str());
            }
        });
    }

    pub fn edits(&mut self, ui: &mut egui::Ui, selection: &CharSelection) -> Option<Command> {
        self.edits_for(ui, selection, std::slice::from_ref(selection), &[1.0])
    }

    fn numeric_command(&mut self, event: NumericEvent, command: impl FnOnce(f64) -> Option<Command>) -> Option<Command> {
        match event {
            NumericEvent::Preview(value) => {
                self.numeric_preview = command(value);
                None
            }
            NumericEvent::Commit(value) => command(value),
            NumericEvent::Cancel => {
                self.cancel_number = true;
                None
            }
            NumericEvent::None => None,
        }
    }

    fn edits_for(&mut self, ui: &mut egui::Ui, selection: &CharSelection, targets: &[CharSelection], size_scales: &[f64]) -> Option<Command> {
        self.cancel_number = false;
        self.numeric_preview = None;
        let patch = |value: Value| batched(targets.iter().filter(|target| target.can_style()).map(|target| {
            target.style_command(target.start, target.end, value.clone())
        }).collect());
        let nudge = |key: &str, step: fn(f32) -> f32| batched(targets.iter().flat_map(|target| {
            target.segment_commands(key, |current| {
                let value = current.and_then(Value::as_f64).unwrap_or(0.0) as f32;
                Some(json!(step(value)))
            })
        }).collect());
        let mut command = None;
        let library = dtool_tools::dtool_text::FontLibrary::shared();
        let family = selection.text_value("family");
        let family_name = family.as_deref().map(|name| if name.is_empty() { "sans-serif" } else { name }).unwrap_or(dtool_design::MIXED_TEXT);
        if let Some(picked) = font_row(ui, text("char.family"), |ui| {
            crate::font_combo::font_combo(ui, "character.family", &mut self.fonts, &library, family_name)
        }) {
            command = patch(crate::font_combo::family_patch(&library, &picked));
        }
        let weight = selection.number("weight", f64::from(REGULAR_WEIGHT));
        let italic = selection.field("italic");
        let mut face = match (selection.text_value("style"), weight, &italic) {
            (Some(name), _, _) if !name.is_empty() => name,
            (Some(_), Some(weight), Field::Same(value)) => {
                dtool_tools::dtool_text::fonts::style_name(weight as u16, value.as_bool().unwrap_or(false))
            }
            _ => dtool_design::MIXED_TEXT.into(),
        };
        if font_row(ui, text("char.font_style"), |ui| {
            crate::font_combo::style_combo(ui, "character.style", &library, family_name, &mut face)
        }) {
            if let Some(value) = crate::font_combo::style_patch(&library, family_name, &face) {
                command = patch(value);
            }
        }
        let mut sizes = targets.iter().zip(size_scales)
            .map(|(target, scale)| target.number("size_pt", 12.0).map(|size| size * scale));
        let size_pt = sizes.next().flatten().filter(|first| sizes.all(|size| size == Some(*first)));
        let size = field_row(ui, text("char.size"), |ui, label| {
            number_field(ui, "char.size", label, size_pt, "pt", f64::from(dtool_core::text_cmd::DEFAULT_SIZE_PT))
        });
        if let Some(change) = self.numeric_command(size, |size| {
            (size_pt != Some(size)).then(|| batched(targets.iter().zip(size_scales)
                .filter(|(target, _)| target.can_style())
                .map(|(target, scale)| target.style_command(target.start, target.end, json!({"size_pt": size / scale})))
                .collect())).flatten()
        }) {
            command = Some(change);
        }
        let wrap_position = panel_width(ui) < 6.0 * 24.0 + 5.0 * ui.spacing().item_spacing.x;
        ui.horizontal_wrapped(|ui| {
            let bold = weight.is_some_and(|weight| weight >= f64::from(BOLD_WEIGHT));
            if self.icons.studio_button(ui, "ctx-text-bold", text("ctx.text.bold"), bold).clicked() {
                let weight = if bold { REGULAR_WEIGHT } else { BOLD_WEIGHT };
                command = patch(json!({"style": null, "weight": weight}));
            }
            let italic = italic == Field::Same(json!(true));
            if self.icons.studio_button(ui, "ctx-text-italic", text("ctx.text.italic"), italic).clicked() {
                command = patch(json!({"style": null, "italic": !italic}));
            }
            for (key, label, asset) in [
                ("underline", "menu.text.underline", "ctx-text-underline"),
                ("strike", "menu.text.strikethrough", "ctx-text-strike"),
            ] {
                let on = selection.decoration_on(key) == Some(true);
                if self.icons.studio_button(ui, asset, text(label), on).clicked() {
                    command = batched(targets.iter().flat_map(|target| target.segment_commands(key, |current| {
                        let mut next = current.filter(|value| value.is_object()).cloned().unwrap_or_else(|| json!({}));
                        next["on"] = json!(!on);
                        Some(next)
                    })).collect());
                }
            }
            if wrap_position {
                ui.end_row();
            }
            for (label, asset, target) in [
                ("menu.text.superscript", "ctx-text-superscript", POSITION_SUPER),
                ("menu.text.subscript", "ctx-text-subscript", POSITION_SUB),
            ] {
                let on = selection.field("position") == Field::Same(json!(target));
                if self.icons.studio_button(ui, asset, text(label), on).clicked() {
                    command = patch(json!({"position": if on { POSITION_NORMAL } else { target }}));
                }
            }
        });
        ui.separator();
        field_row(ui, text("char.tracking"), |ui, label| {
            ui.horizontal(|ui| {
                let tracking = selection.number("tracking", 0.0);
                let event = number_field(ui, "char.tracking", label, tracking, "", 0.0);
                if let Some(change) = self.numeric_command(event, |value| {
                    (tracking != Some(value)).then(|| patch(json!({"tracking": value}))).flatten()
                }) {
                    command = Some(change);
                }
                if icon(ui, "−", "menu.text.tighten", false) {
                    command = nudge("tracking", tracking_tighten);
                }
                if icon(ui, "+", "menu.text.loosen", false) {
                    command = nudge("tracking", tracking_loosen);
                }
            });
        });
        field_row(ui, text("char.baseline"), |ui, label| {
            ui.horizontal(|ui| {
                let baseline = selection.number("baseline_pt", 0.0);
                let event = number_field(ui, "char.baseline", label, baseline, "pt", 0.0);
                if let Some(change) = self.numeric_command(event, |value| {
                    (baseline != Some(value)).then(|| patch(json!({"baseline_pt": value}))).flatten()
                }) {
                    command = Some(change);
                }
                if icon(ui, "↑", "menu.text.raise", false) {
                    command = nudge("baseline_pt", baseline_raise);
                }
                if icon(ui, "↓", "menu.text.lower", false) {
                    command = nudge("baseline_pt", baseline_lower);
                }
            });
        });
        command
    }
}
