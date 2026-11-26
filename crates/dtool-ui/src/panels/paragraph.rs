#[path = "../controls/tab_stop_editor.rs"]
mod tab_stop_editor;

use dtool_core::{document::Units, Session, StoryId};
use dtool_design::{numeric_field, panel_frame, panel_header, space, toggle, NumericEvent, NumericSpec, NumericValue};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use dtool_tools::dtool_text::para_style::{FlowStart, ListKind, ListProps, NumberFormat, SpaceBeforeRule};
use dtool_tools::dtool_text::{Align, ParaStyle};
use serde_json::{json, Value};

pub const ALIGN_OPTIONS: [(Align, &str); 7] = [
    (Align::Left, "para.align.left"),
    (Align::Center, "para.align.center"),
    (Align::Right, "para.align.right"),
    (Align::JustifyLeft, "menu.text.alignment.justify_left"),
    (Align::JustifyCenter, "menu.text.alignment.justify_centre"),
    (Align::JustifyRight, "menu.text.alignment.justify_right"),
    (Align::JustifyAll, "menu.text.alignment.justify_all"),
];

pub const SPACE_RULE_OPTIONS: [(SpaceBeforeRule, &str); 3] = [
    (SpaceBeforeRule::Always, "para.space.rule.always"),
    (SpaceBeforeRule::OnlyBetweenParagraphs, "para.space.rule.between"),
    (SpaceBeforeRule::OnlyAtColumnTop, "para.space.rule.column_top"),
];

const FLOW_START_OPTIONS: [(FlowStart, &str); 6] = [
    (FlowStart::Anywhere, "para.flow.anywhere"),
    (FlowStart::NextColumn, "para.flow.next_column"),
    (FlowStart::NextFrame, "para.flow.next_frame"),
    (FlowStart::NextPage, "para.flow.next_page"),
    (FlowStart::NextOddPage, "para.flow.next_odd_page"),
    (FlowStart::NextEvenPage, "para.flow.next_even_page"),
];

const NUMBER_FORMATS: [(NumberFormat, &str); 8] = [
    (NumberFormat::Arabic, "para.list.number.arabic"),
    (NumberFormat::Arabic2, "01"),
    (NumberFormat::Arabic3, "001"),
    (NumberFormat::Arabic4, "0001"),
    (NumberFormat::AlphaLower, "para.list.number.alpha_lower"),
    (NumberFormat::AlphaUpper, "para.list.number.alpha_upper"),
    (NumberFormat::RomanLower, "para.list.number.roman_lower"),
    (NumberFormat::RomanUpper, "para.list.number.roman_upper"),
];

#[derive(Clone, Copy, PartialEq)]
enum ListMode { None, Bullet, Number }

impl From<&ListKind> for ListMode {
    fn from(kind: &ListKind) -> Self {
        match kind {
            ListKind::None => Self::None,
            ListKind::Bullet(_) => Self::Bullet,
            ListKind::Number(_) => Self::Number,
        }
    }
}

enum ListEdit {
    Mode(ListMode),
    Bullet(String),
    Format(NumberFormat),
    Level(u32),
    Tab(f32),
    Restart(u32),
    Continue,
}

enum ParagraphEdit {
    Style(Value),
    List(ListEdit),
    Detail { section: &'static str, field: &'static str, value: Value },
}

#[derive(Default)]
pub struct ParagraphPanel {
    pub style: ParaStyle,
    pub error: Option<String>,
    pub mixed: Vec<&'static str>,
    pub drag: Option<(&'static str, f32)>,
    tab_editor: tab_stop_editor::TabStopEditorState,
    decoration_index: usize,
    icons: crate::icon_cache::IconCache,
}

pub fn query_style(session: &mut Session) -> Option<ParaStyle> {
    let (story, range) = active_ranges(session).into_iter().next()?;
    let reply = session.run_named("text.para.query", json!({ "story": story.0, "range": range })).ok()?;
    Some(serde_json::from_value(reply["style"].clone()).unwrap_or_default())
}

pub fn apply_patch(session: &mut Session, patch: Value) -> Result<Value, CommandError> {
    let command = patch_command(&active_ranges(session), patch).ok_or_else(CommandError::invalid_params)?;
    session.run(&command)
}

pub(crate) fn patch_command(ranges: &[(StoryId, [usize; 2])], patch: Value) -> Option<Command> {
    super::character::batched(ranges.iter().map(|(story, range)| {
        Command::with_params("text.para.set", json!({ "story": story.0, "range": range, "patch": patch }))
    }).collect())
}

pub(crate) fn active_ranges(session: &Session) -> Vec<(StoryId, [usize; 2])> {
    if session.text_edit.is_some() {
        return session.active_para_range().into_iter().collect();
    }
    session.selected_text_stories().into_iter().filter_map(|id| {
        let story = session.document.stories.iter().find(|story| story.id == id)?;
        Some((id, [0, story.text.chars().count()]))
    }).collect()
}

pub const PARA_FIELDS: &[&str] = &[
    "align", "indent_left", "indent_right", "indent_first", "leading", "space_before",
    "space_after", "space_rule", "hyphenation", "drop_cap", "list.mode", "list.kind",
    "list.level", "list.tab", "tabs", "default_tab_interval", "baseline_grid",
    "keep_with_next", "keep_with_previous", "keep_together", "prevent_orphans", "prevent_widows", "flow_start",
    "hyphenation.on", "hyphenation.min_word", "hyphenation.min_prefix", "hyphenation.min_suffix",
    "hyphenation.max_consecutive", "hyphenation.zone", "drop_cap.on", "drop_cap.lines",
    "drop_cap.chars", "drop_cap.distance",
    "justification.enabled", "justification.word_min", "justification.word_ideal", "justification.word_max",
    "justification.letter_min", "justification.letter_ideal", "justification.letter_max",
    "initial_words.on", "initial_words.max_words", "initial_words.end_chars", "initial_words.style",
    "paragraph_decorations",
];

pub fn paragraph_ranges(session: &Session) -> Option<(StoryId, Vec<[usize; 2]>)> {
    let (story, [start, end]) = session.active_para_range()?;
    let story = session.document.stories.iter().find(|s| s.id == story)?;
    Some((story.id, split_paragraphs(&story.text, start, end)))
}

fn split_paragraphs(text: &str, start: usize, end: usize) -> Vec<[usize; 2]> {
    let mut ranges = Vec::new();
    let mut from = start;
    for (i, ch) in text.chars().enumerate().take(end).skip(start) {
        if matches!(ch, '\n' | '\u{2029}') {
            ranges.push([from, i + 1]);
            from = i + 1;
        }
    }
    if from < end || ranges.is_empty() {
        ranges.push([from, end.max(from)]);
    }
    ranges
}

fn target_paragraphs(session: &Session) -> impl Iterator<Item = (&dtool_core::document::Story, Vec<[usize; 2]>)> {
    active_ranges(session).into_iter().filter_map(|(id, [start, end])| {
        let story = session.document.stories.iter().find(|story| story.id == id)?;
        Some((story, split_paragraphs(&story.text, start, end)))
    })
}

pub fn mixed_fields(session: &mut Session) -> Vec<&'static str> {
    let styles: Vec<Value> = target_paragraphs(session)
        .flat_map(|(story, ranges)| ranges.into_iter().map(move |[start, end]| {
            let style = dtool_core::text_cmd::query_para_style(&story.paras, start, end);
            serde_json::to_value(serde_json::from_value::<ParaStyle>(style).unwrap_or_default()).unwrap_or(Value::Null)
        }))
        .collect();
    PARA_FIELDS.iter().copied().filter(|field| styles.windows(2).any(|pair| {
        if *field == "list.mode" {
            let mode = |style: &Value| {
                let kind = &style["list"]["kind"];
                if kind.get("Bullet").is_some() { ListMode::Bullet }
                else if kind.get("Number").is_some() { ListMode::Number }
                else { ListMode::None }
            };
            mode(&pair[0]) != mode(&pair[1])
        } else if let Some((section, field)) = field.split_once('.') {
            pair[0][section][field] != pair[1][section][field]
        } else {
            pair[0][*field] != pair[1][*field]
        }
    })).collect()
}

fn detail_command(session: &Session, section: &str, field: &str, value: Value) -> Result<Option<Command>, CommandError> {
    let mut commands = Vec::new();
    for (story, ranges) in target_paragraphs(session) {
        for [start, end] in ranges {
            let style = dtool_core::text_cmd::query_para_style(&story.paras, start, end);
            let style: ParaStyle = serde_json::from_value(style).unwrap_or_default();
            let mut detail = match section {
                "hyphenation" => json!(style.hyphenation),
                "drop_cap" => json!(style.drop_cap),
                "justification" => json!(style.justification),
                "initial_words" => json!(style.initial_words),
                _ => return Err(CommandError::invalid_params()),
            };
            let value = if section == "justification" && let Some(number) = value.as_f64() {
                let options = style.justification;
                let (low, high) = match field {
                    "word_min" => (1.0, options.word_ideal),
                    "word_ideal" => (options.word_min, options.word_max),
                    "word_max" => (options.word_ideal, 1000.0),
                    "letter_min" => (-100.0, options.letter_ideal),
                    "letter_ideal" => (options.letter_min, options.letter_max),
                    "letter_max" => (options.letter_ideal, 100.0),
                    _ => return Err(CommandError::invalid_params()),
                };
                json!((number as f32).clamp(low.min(high), low.max(high)))
            } else {
                value.clone()
            };
            if detail[field] != value {
                detail[field] = value.clone();
                commands.push(Command::with_params("text.para.set", json!({
                    "story": story.id.0, "range": [start, end], "patch": { (section): detail },
                })));
            }
        }
    }
    Ok(super::character::batched(commands))
}

fn list_command(session: &Session, edit: ListEdit) -> Option<Command> {
    let mut commands = Vec::new();
    for (story, ranges) in target_paragraphs(session) {
        for [start, end] in ranges {
            let current = dtool_core::text_cmd::query_para_style(&story.paras, start, end);
            let mut list: ListProps = serde_json::from_value(current["list"].clone()).unwrap_or_default();
            let previous = list.clone();
            let mode = ListMode::from(&list.kind);
            match &edit {
                ListEdit::Mode(next) if *next != mode => {
                    list.kind = match next {
                        ListMode::None => ListKind::None,
                        ListMode::Bullet => ListKind::Bullet("•".into()),
                        ListMode::Number => ListKind::Number(NumberFormat::Arabic),
                    };
                    list.restart = false;
                    list.start_at = None;
                }
                ListEdit::Bullet(symbol) if mode == ListMode::Bullet => {
                    list.kind = ListKind::Bullet(symbol.clone());
                }
                ListEdit::Format(format) if mode == ListMode::Number => list.kind = ListKind::Number(format.clone()),
                ListEdit::Level(level) if mode != ListMode::None => list.level = *level,
                ListEdit::Tab(tab) if mode != ListMode::None => list.tab = *tab,
                ListEdit::Restart(first) if mode == ListMode::Number => {
                    list.restart = true;
                    list.start_at = Some(*first);
                }
                ListEdit::Continue if mode == ListMode::Number => {
                    list.restart = false;
                    list.start_at = None;
                }
                _ => continue,
            }
            if list != previous {
                commands.push(Command::with_params("text.para.set", json!({
                    "story": story.id.0, "range": [start, end], "patch": { "list": list },
                })));
            }
            if matches!(edit, ListEdit::Restart(_)) { break; }
        }
    }
    super::character::batched(commands)
}

fn first_number_start(session: &Session) -> u32 {
    target_paragraphs(session).flat_map(|(story, ranges)| ranges.into_iter().map(move |range| (story, range))).find_map(|(story, [start, end])| {
        let style = dtool_core::text_cmd::query_para_style(&story.paras, start, end);
        let list: ListProps = serde_json::from_value(style["list"].clone()).ok()?;
        matches!(list.kind, ListKind::Number(_)).then(|| list.start_at.unwrap_or(1))
    }).unwrap_or(1)
}

fn label_for(key: &str, field: &str, mixed: &[&'static str]) -> String {
    if mixed.contains(&field) { format!("{} ({})", text(key), text("panel.transform.mixed")) } else { text(key).to_string() }
}

pub fn drag_commit(drag: &mut Option<(&'static str, f32)>, field: &'static str, edited: f32, changed: bool, dragging: bool, stopped: bool) -> Option<f32> {
    if dragging {
        if changed || drag.is_some_and(|(f, _)| f == field) {
            *drag = Some((field, edited));
        }
        return None;
    }
    if stopped || drag.is_some_and(|(f, _)| f == field) {
        let value = drag.filter(|(f, _)| *f == field).map(|(_, v)| v).unwrap_or(edited);
        *drag = None;
        return Some(value);
    }
    changed.then_some(edited)
}

fn panel_width(ui: &egui::Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x).max(0.0)
}

fn paragraph_field<R>(
    ui: &mut egui::Ui,
    label: &str,
    control_width: f32,
    control: impl FnOnce(&mut egui::Ui, egui::Response) -> R,
) -> R {
    let width = panel_width(ui);
    let caption_width = (width - control_width - ui.spacing().item_spacing.x).min(110.0);
    if caption_width < 48.0 {
        let caption = ui.add(dtool_design::numeric_caption(label));
        control(ui, caption)
    } else {
        ui.horizontal(|ui| {
            let caption = ui.allocate_ui_with_layout(
                egui::vec2(caption_width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(caption_width);
                    ui.add(dtool_design::numeric_caption(label).truncate())
                },
            ).inner;
            control(ui, caption)
        }).inner
    }
}

fn number(ui: &mut egui::Ui, key: &str, field: &'static str, (value, default): (f32, f32), mixed: &[&'static str], drag: &mut Option<(&'static str, f32)>, units: Option<Units>) -> Option<f32> {
    let label = label_for(key, field, mixed);
    let width = 62.0;
    paragraph_field(ui, text(key), width, |ui, caption| {
        let mut spec = NumericSpec::new("").width(width.min(panel_width(ui))).enabled(ui.is_enabled());
        if field == "leading" {
            spec = spec.step(0.05).range(0.01..=f64::INFINITY);
        }
        if let Some(units) = units {
            spec = spec.suffix(dtool_core::commands::ruler_units::suffix(units));
        }
        let id = ui.make_persistent_id(field);
        let mixed = mixed.contains(&field);
        let display = if mixed { NumericValue::Mixed } else { NumericValue::Value(value.into()) };
        let (response, event) = numeric_field(ui, id, display, &spec);
        response.labelled_by(caption.id).on_hover_text(label);
        if ui.is_enabled() && caption.double_clicked() {
            return (mixed || default != value).then_some(default);
        }
        match event {
            NumericEvent::Commit(edited) => (mixed || edited as f32 != value).then_some(edited as f32),
            NumericEvent::Preview(edited) => {
                *drag = Some((field, edited as f32));
                Some(edited as f32)
            }
            NumericEvent::None => {
                let state = ui.data(|data| data.get_temp::<dtool_design::NumericState>(id))?;
                if !state.scrubbing || !state.previewed { return None; }
                *drag = Some((field, state.scrub as f32));
                Some(state.scrub as f32)
            }
            NumericEvent::Cancel => {
                crate::app_canvas::clear_geometry_preview(ui);
                None
            }
        }
    })
}

fn choice<T: PartialEq + Clone>(ui: &mut egui::Ui, id: &str, options: &[(T, &str)], mut current: T, mixed: bool) -> Option<T> {
    let labels: Vec<_> = options.iter().map(|(value, key)| (value.clone(), text(key))).collect();
    let width = panel_width(ui).min(104.0);
    paragraph_field(ui, text(id), width, |ui, caption| {
        ui.scope(|ui| {
            ui.spacing_mut().combo_width = panel_width(ui).min(width);
            let state = dtool_design::ControlState::default().mixed(mixed);
            dtool_design::dropdown_value(ui, id, &mut current, &labels, &state)
                .labelled_by(caption.id).changed().then_some(current)
        }).inner
    })
}

impl ParagraphPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::Workspace) {
        panel_frame(ui, |ui| {
            panel_header(ui, text("para.title"));
            let viewport = ui.available_rect_before_wrap().intersect(ui.clip_rect());
            egui::ScrollArea::vertical().id_salt("paragraph.body")
                .max_height(viewport.height().max(0.0))
                .min_scrolled_height(0.0)
                .auto_shrink([false, true])
                .show(ui, |ui| self.show_controls(ui, workspace));
        });
    }

    fn show_controls(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::Workspace) {
        let document = workspace.active();
        let current = workspace.session_mut().and_then(|session| query_style(session).map(|style| (style, mixed_fields(session))));
        let Some((style, mixed)) = current else {
            self.style = ParaStyle::default();
            self.mixed.clear();
            ui.add_enabled_ui(false, |ui| { self.edits(ui, Units::Pt, 72.0, 1, &[], ui.scope_id()); })
                .response.on_disabled_hover_text(text("para.no_story"));
            return;
        };
        self.style = style;
        self.mixed = mixed;
        let Some(session) = workspace.session_mut() else { return; };
        let start_at = first_number_start(session);
        let styles = session.document.text_styles.names(dtool_core::text_styles::StyleKind::Character);
        let ranges = active_ranges(session);
        let edit = ui.push_id((document, session.generation, &ranges), |ui| {
            let end_chars_id = ui.make_persistent_id("initial_words.end_chars");
            self.edits(ui, session.document.units, session.document.dpi, start_at, &styles, end_chars_id)
        }).inner;
        if let Some(edit) = edit {
            let command = match edit {
                ParagraphEdit::Style(patch) => Ok(patch_command(&ranges, patch)),
                ParagraphEdit::List(edit) => Ok(list_command(session, edit)),
                ParagraphEdit::Detail { section, field, value } => detail_command(session, section, field, value),
            };
            let result = command.and_then(|command| {
                if let Some(command) = command {
                    if self.drag.is_some() {
                        crate::app_canvas::stage_geometry_preview(ui, session, command);
                    } else {
                        session.run(&command)?;
                    }
                }
                Ok(())
            });
            if self.drag.is_none() {
                self.error = result.err().map(|error| text(&error.message_key).to_string());
            }
        }
        if let Some(message) = &self.error {
            ui.label(message.as_str());
        }
    }

    fn edits(&mut self, ui: &mut egui::Ui, units: Units, dpi: f32, start_at: u32, styles: &[String], end_chars_id: egui::Id) -> Option<ParagraphEdit> {
        let style = self.style.clone();
        let defaults = ParaStyle::default();
        self.drag = None;
        let mut patch = None;
        let mut set = |key: &str, value: Value| {
            patch = Some(json!({ key: value }));
        };
        if let Some(align) = alignment_buttons(ui, (!self.mixed.contains(&"align")).then_some(style.align), true) {
            set("align", json!(align));
        }
        let indent_scale = dtool_core::units::units_per_point(units, dpi);
        let list_edit = self.list_edits(ui, &style.list, units, indent_scale, start_at);
        let mut detail_edit = None;
        ui.collapsing(text("para.justification"), |ui| {
            let options = style.justification;
            let mut enabled = options.enabled && !self.mixed.contains(&"justification.enabled");
            if toggle(ui, &mut enabled, label_for("para.justification.custom", "justification.enabled", &self.mixed)).changed() {
                detail_edit = Some(ParagraphEdit::Detail { section: "justification", field: "enabled", value: json!(enabled) });
            }
            ui.add_enabled_ui(enabled, |ui| {
                let reset = defaults.justification;
                for (id, field, value) in [
                    ("justification.word_min", "word_min", (options.word_min, reset.word_min)),
                    ("justification.word_ideal", "word_ideal", (options.word_ideal, reset.word_ideal)),
                    ("justification.word_max", "word_max", (options.word_max, reset.word_max)),
                    ("justification.letter_min", "letter_min", (options.letter_min, reset.letter_min)),
                    ("justification.letter_ideal", "letter_ideal", (options.letter_ideal, reset.letter_ideal)),
                    ("justification.letter_max", "letter_max", (options.letter_max, reset.letter_max)),
                ] {
                    let label = format!("para.{id}");
                    if let Some(value) = number(ui, &label, id, value, &self.mixed, &mut self.drag, None) {
                        detail_edit = Some(ParagraphEdit::Detail { section: "justification", field, value: json!(value) });
                    }
                }
            });
        });
        for (key, field, value, default) in [
            ("para.indent.left", "indent_left", style.indent_left, defaults.indent_left),
            ("para.indent.right", "indent_right", style.indent_right, defaults.indent_right),
            ("para.indent.first", "indent_first", style.indent_first, defaults.indent_first),
        ] {
            if let Some(value) = number(ui, key, field, (value * indent_scale, default * indent_scale), &self.mixed, &mut self.drag, Some(units)) {
                set(field, json!(value / indent_scale));
            }
        }
        ui.add_space(space("sm"));
        ui.horizontal(|ui| {
            ui.label(text("para.spacing"));
            let mixed = self.mixed.contains(&"baseline_grid");
            let label = label_for("para.baseline_grid", "baseline_grid", &self.mixed);
            if self.icons.studio_button(ui, "cmd-grid", &label, style.baseline_grid && !mixed).clicked() {
                set("baseline_grid", json!(mixed || !style.baseline_grid));
            }
        });
        if let Some(value) = number(ui, "para.leading", "leading", (style.leading, defaults.leading), &self.mixed, &mut self.drag, None) {
            if value > 0.0 {
                set("leading", json!(value));
            }
        }
        for (key, field, value) in [
            ("para.space.before", "space_before", (style.space_before, defaults.space_before)),
            ("para.space.after", "space_after", (style.space_after, defaults.space_after)),
        ] {
            if let Some(value) = number(ui, key, field, value, &self.mixed, &mut self.drag, Some(Units::Pt)) {
                set(field, json!(value.max(0.0)));
            }
        }
        if let Some(rule) = choice(ui, "para.space_rule", &SPACE_RULE_OPTIONS, style.space_rule.clone(), self.mixed.contains(&"space_rule")) {
            set("space_rule", json!(rule));
        }
        ui.add_space(space("sm"));
        ui.collapsing(text("para.flow"), |ui| {
            if let Some(value) = choice(ui, "para.flow.start", &FLOW_START_OPTIONS, style.flow_start,
                self.mixed.contains(&"flow_start"))
            {
                set("flow_start", json!(value));
            }
            for (key, field, enabled) in [
                ("para.keep.with.previous", "keep_with_previous", style.keep_with_previous),
                ("para.keep.together", "keep_together", style.keep_together),
                ("para.prevent.orphans", "prevent_orphans", style.prevent_orphans),
                ("para.prevent.widows", "prevent_widows", style.prevent_widows),
            ] {
                let mut value = enabled && !self.mixed.contains(&field);
                if toggle(ui, &mut value, label_for(key, field, &self.mixed)).changed() {
                    set(field, json!(value));
                }
            }
            if let Some(value) = number(ui, "para.keep.with.next", "keep_with_next",
                (style.keep_with_next as f32, defaults.keep_with_next as f32), &self.mixed, &mut self.drag, None)
            {
                set("keep_with_next", json!(value.round().clamp(0.0, 9.0) as u32));
            }
        });
        ui.collapsing(text("para.hyphenation"), |ui| {
            let hyphen = &style.hyphenation;
            let mut enabled = hyphen.on && !self.mixed.contains(&"hyphenation.on");
            if toggle(ui, &mut enabled, label_for("para.hyphenation", "hyphenation.on", &self.mixed)).changed() {
                detail_edit = Some(ParagraphEdit::Detail { section: "hyphenation", field: "on", value: json!(enabled) });
            }
            ui.add_enabled_ui(enabled, |ui| {
                let reset = &defaults.hyphenation;
                for (key, id, field, value, default, min) in [
                    ("para.hyphen.min.word", "hyphenation.min_word", "min_word", hyphen.min_word, reset.min_word, 2.0),
                    ("para.hyphen.min.prefix", "hyphenation.min_prefix", "min_prefix", hyphen.min_prefix, reset.min_prefix, 1.0),
                    ("para.hyphen.min.suffix", "hyphenation.min_suffix", "min_suffix", hyphen.min_suffix, reset.min_suffix, 1.0),
                    ("para.hyphen.max.consecutive", "hyphenation.max_consecutive", "max_consecutive", hyphen.max_consecutive, reset.max_consecutive, 0.0),
                ] {
                    if let Some(value) = number(ui, key, id, (value as f32, default as f32), &self.mixed, &mut self.drag, None) {
                        detail_edit = Some(ParagraphEdit::Detail {
                            section: "hyphenation", field, value: json!(value.round().max(min) as u32),
                        });
                    }
                }
                let absolute = hyphen.zone >= 1.0;
                let (key, value, unit) = if absolute {
                    ("para.hyphen.zone", hyphen.zone, Some(Units::Pt))
                } else {
                    ("para.hyphen.zone_percent", hyphen.zone * 100.0, None)
                };
                let default = if absolute { reset.zone } else { reset.zone * 100.0 };
                if let Some(value) = number(ui, key, "hyphenation.zone", (value, default), &self.mixed, &mut self.drag, unit) {
                    let zone = if absolute { value.max(0.0) } else { value.clamp(0.0, 99.0) / 100.0 };
                    detail_edit = Some(ParagraphEdit::Detail { section: "hyphenation", field: "zone", value: json!(zone) });
                }
            });
        });
        ui.collapsing(text("para.drop_caps"), |ui| {
            let cap = &style.drop_cap;
            let mut enabled = cap.on && !self.mixed.contains(&"drop_cap.on");
            if toggle(ui, &mut enabled, label_for("para.drop_caps", "drop_cap.on", &self.mixed)).changed() {
                detail_edit = Some(ParagraphEdit::Detail { section: "drop_cap", field: "on", value: json!(enabled) });
            }
            ui.add_enabled_ui(enabled, |ui| {
                let reset = &defaults.drop_cap;
                for (key, id, field, value, default) in [
                    ("para.drop.lines", "drop_cap.lines", "lines", cap.lines, reset.lines),
                    ("para.drop.chars", "drop_cap.chars", "chars", cap.chars, reset.chars),
                ] {
                    if let Some(value) = number(ui, key, id, (value as f32, default as f32), &self.mixed, &mut self.drag, None) {
                        detail_edit = Some(ParagraphEdit::Detail {
                            section: "drop_cap", field, value: json!(value.round().max(1.0) as u32),
                        });
                    }
                }
                if let Some(value) = number(ui, "para.drop.distance", "drop_cap.distance",
                    (cap.distance * indent_scale, reset.distance * indent_scale), &self.mixed, &mut self.drag, Some(units))
                {
                    detail_edit = Some(ParagraphEdit::Detail {
                        section: "drop_cap", field: "distance", value: json!(value.max(0.0) / indent_scale),
                    });
                }
            });
        });
        if let Some(edit) = self.initial_words(ui, &style.initial_words, styles, end_chars_id) {
            detail_edit = Some(edit);
        }
        if let Some(decorations) = self.paragraph_decorations(ui, &style, units, indent_scale) {
            set("paragraph_decorations", json!(decorations));
        }
        ui.add_space(space("sm"));
        ui.label(label_for("para.tab_stops", "tabs", &self.mixed));
        self.tab_editor.sync_from(&style.tabs, style.default_tab_interval);
        use tab_stop_editor::TabStopEditorAction;
        match self.tab_editor.show(ui) {
            TabStopEditorAction::None => {}
            TabStopEditorAction::PreviewTabs(tabs) => {
                self.drag = Some(("tabs", 0.0));
                patch = Some(json!({ "tabs": tabs }));
            }
            TabStopEditorAction::PreviewInterval(interval) => {
                self.drag = Some(("default_tab_interval", interval));
                patch = Some(json!({ "default_tab_interval": interval }));
            }
            TabStopEditorAction::IntervalChanged => patch = Some(json!({
                "default_tab_interval": self.tab_editor.default_interval_pt(),
            })),
            _ => patch = Some(json!({ "tabs": self.tab_editor.sorted_stops() })),
        }
        list_edit.map(ParagraphEdit::List).or(detail_edit).or_else(|| patch.map(ParagraphEdit::Style))
    }

    fn paragraph_decorations(
        &mut self, ui: &mut egui::Ui, style: &ParaStyle, units: Units, scale: f32,
    ) -> Option<Vec<dtool_tools::dtool_text::para_style::ParagraphDecoration>> {
        use dtool_core::paint::Colour;
        use dtool_tools::dtool_text::para_style::ParagraphDecoration;
        let mut decorations = style.paragraph_decorations.clone();
        let mut changed = false;
        ui.collapsing(text("para.decorations"), |ui| {
            self.decoration_index = self.decoration_index.min(decorations.len().saturating_sub(1));
            ui.horizontal(|ui| {
                if !decorations.is_empty() {
                    let name = |index| format!("{} {}", text("para.decorations"), index + 1);
                    egui::ComboBox::from_id_salt("paragraph.decoration.index").width(85.0)
                        .selected_text(name(self.decoration_index)).show_ui(ui, |ui| {
                            for index in 0..decorations.len() {
                                ui.selectable_value(&mut self.decoration_index, index, name(index));
                            }
                        });
                }
                if ui.button("+").on_hover_text(text("para.decorations.add")).clicked() {
                    self.decoration_index = decorations.len();
                    decorations.push(ParagraphDecoration::default());
                    changed = true;
                }
                if ui.add_enabled(!decorations.is_empty(), egui::Button::new("−"))
                    .on_hover_text(text("para.decorations.remove")).clicked()
                {
                    decorations.remove(self.decoration_index);
                    changed = true;
                }
            });
            let Some(decoration) = decorations.get_mut(self.decoration_index) else { return };
            ui.horizontal_wrapped(|ui| {
                for (index, icon, label) in [
                    (0, "tool-align-left", "para.decorations.left"),
                    (1, "tool-align-top", "para.decorations.top"),
                    (2, "tool-align-right", "para.decorations.right"),
                    (3, "tool-align-bottom", "para.decorations.bottom"),
                ] {
                    if self.icons.studio_button(ui, icon, text(label), decoration.sides[index]).clicked() {
                        decoration.sides[index] = !decoration.sides[index];
                        changed = true;
                    }
                }
            });
            let colour = |ui: &mut egui::Ui, colour: &mut Colour| {
                let mut display = egui::Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a);
                if ui.color_edit_button_srgba(&mut display).changed() {
                    let [r, g, b, a] = display.to_srgba_unmultiplied();
                    *colour = Colour::new(r, g, b, a);
                    true
                } else {
                    false
                }
            };
            dtool_design::studio_row(ui, text("para.decorations.stroke"), |ui, _| {
                changed |= colour(ui, &mut decoration.stroke);
            });
            ui.horizontal_wrapped(|ui| {
                let mut fill = decoration.fill.is_some();
                if toggle(ui, &mut fill, text("para.decorations.fill")).changed() {
                    decoration.fill = fill.then(|| Colour::new(232, 232, 232, 255));
                    changed = true;
                }
                if let Some(fill) = &mut decoration.fill { changed |= colour(ui, fill); }
            });
            let defaults = ParagraphDecoration::default();
            for (label, key, field, default) in [
                ("para.decorations.width", "paragraph_decoration.width", &mut decoration.width, defaults.width),
                ("para.decorations.padding", "paragraph_decoration.padding", &mut decoration.padding, defaults.padding),
            ] {
                if let Some(value) = number(ui, label, key, (*field * scale, default * scale), &[], &mut self.drag, Some(units)) {
                    *field = value.max(0.0) / scale;
                    changed = true;
                }
            }
        });
        changed.then_some(decorations)
    }

    fn initial_words(&mut self, ui: &mut egui::Ui, options: &dtool_tools::dtool_text::para_style::InitialWords, styles: &[String], id: egui::Id) -> Option<ParagraphEdit> {
        let mut edit = None;
        let mut set = |field, value| {
            edit = Some(ParagraphEdit::Detail { section: "initial_words", field, value });
        };
        ui.collapsing(text("para.initial_words"), |ui| {
            let mut enabled = options.on && !self.mixed.contains(&"initial_words.on");
            if toggle(ui, &mut enabled, label_for("para.initial_words", "initial_words.on", &self.mixed)).changed() {
                set("on", json!(enabled));
            }
            ui.add_enabled_ui(enabled, |ui| {
                if let Some(value) = number(ui, "para.initial.max_words", "initial_words.max_words",
                    (options.max_words as f32, dtool_tools::dtool_text::para_style::InitialWords::default().max_words as f32),
                    &self.mixed, &mut self.drag, None)
                {
                    set("max_words", json!(value.round().clamp(1.0, 999.0) as u32));
                }
                dtool_design::studio_row(ui, text("para.initial.end_chars"), |ui, label| {
                    let mixed = self.mixed.contains(&"initial_words.end_chars");
                    let original = if mixed { "" } else { options.end_chars.as_str() };
                    let mut draft = ui.data_mut(|data| data.get_temp::<String>(id)).unwrap_or_else(|| original.to_owned());
                    let response = ui.add(egui::TextEdit::singleline(&mut draft).id(id).desired_width(76.0)
                        .hint_text(if mixed { "—" } else { "" })).labelled_by(label);
                    if response.lost_focus() {
                        ui.data_mut(|data| data.remove::<String>(id));
                        if draft != original {
                            set("end_chars", json!(draft));
                        }
                    } else if response.changed() {
                        ui.data_mut(|data| data.insert_temp(id, draft));
                    }
                });
                dtool_design::studio_row(ui, text("para.initial.style"), |ui, _| {
                    let mixed = self.mixed.contains(&"initial_words.style");
                    let none = text("para.initial.style.none");
                    let current = if mixed { "—" } else { options.style.as_deref().unwrap_or(none) };
                    egui::ComboBox::from_id_salt("initial_words.style").selected_text(current).show_ui(ui, |ui| {
                        if ui.selectable_label(!mixed && options.style.is_none(), none).clicked() {
                            set("style", Value::Null);
                        }
                        for name in styles {
                            if ui.selectable_label(!mixed && options.style.as_ref() == Some(name), name).clicked() {
                                set("style", json!(name));
                            }
                        }
                    });
                });
            });
        });
        edit
    }

    fn list_edits(&mut self, ui: &mut egui::Ui, list: &ListProps, units: Units, scale: f32, start_at: u32) -> Option<ListEdit> {
        let mode = (!self.mixed.contains(&"list.mode")).then(|| ListMode::from(&list.kind));
        let mut edit = None;
        let compact = panel_width(ui) < 190.0;
        if compact {
            ui.label(text("para.lists"));
        }
        ui.horizontal_wrapped(|ui| {
            if !compact {
                ui.label(text("para.lists"));
            }
            for (kind, icon, key) in [
                (ListMode::None, "para-list-none", "para.list.none"),
                (ListMode::Bullet, "para-list-bullet", "para.list.bullet"),
                (ListMode::Number, "para-list-number", "menu.text.list.number"),
            ] {
                let label = text(key);
                let selected = mode == Some(kind);
                if self.icons.studio_button(ui, icon, label, selected).clicked() {
                    edit = Some(ListEdit::Mode(kind));
                }
            }
        });
        if let ListKind::Bullet(symbol) = &list.kind
            && mode == Some(ListMode::Bullet)
        {
            ui.horizontal(|ui| {
                ui.label(text("para.list.bullet_symbol"));
                let mixed = self.mixed.contains(&"list.kind");
                let current = if mixed { "—" } else { symbol.as_str() };
                ui.menu_button(current, |ui| {
                    ui.horizontal(|ui| {
                        for mark in ["•", "○", "▪", "▫", "◆", "–", "✓"] {
                            if ui.selectable_label(!mixed && symbol == mark, mark).clicked() {
                                edit = Some(ListEdit::Bullet(mark.into()));
                                ui.close();
                            }
                        }
                    });
                });
            });
        }
        if matches!(mode, Some(ListMode::Bullet | ListMode::Number)) {
            let defaults = ListProps::default();
            if let Some(level) = number(ui, "para.list.level", "list.level", (list.level as f32, defaults.level as f32), &self.mixed, &mut self.drag, None) {
                edit = Some(ListEdit::Level(level.round().clamp(1.0, 9.0) as u32));
            }
            if let Some(tab) = number(ui, "para.list.tab", "list.tab", (list.tab_step() * scale, defaults.tab_step() * scale), &self.mixed, &mut self.drag, Some(units)) {
                edit = Some(ListEdit::Tab(tab.max(0.0) / scale));
            }
        }
        if let ListKind::Number(format) = &list.kind {
            if mode == Some(ListMode::Number) {
                if let Some(format) = choice(ui, "para.list.format", &NUMBER_FORMATS, format.clone(), self.mixed.contains(&"list.kind")) {
                    edit = Some(ListEdit::Format(format));
                }
                if let Some(start) = number(ui, "para.list.start", "list.start_at", (start_at as f32, 1.0), &[], &mut self.drag, None) {
                    edit = Some(ListEdit::Restart(start.round().clamp(1.0, 99999.0) as u32));
                }
                ui.horizontal_wrapped(|ui| {
                    if dtool_design::button(ui, text("para.list.restart")).clicked() {
                        edit = Some(ListEdit::Restart(start_at));
                    }
                    if dtool_design::button(ui, text("para.list.continue")).clicked() {
                        edit = Some(ListEdit::Continue);
                    }
                });
            }
        }
        edit
    }
}

pub(crate) fn alignment_buttons(ui: &mut egui::Ui, current: Option<Align>, wrap: bool) -> Option<Align> {
    let mut picked = None;
    let compact = wrap && panel_width(ui) < 180.0;
    let draw = |ui: &mut egui::Ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let height = ui.spacing().interact_size.y.max(23.0);
        for (index, (align, label)) in ALIGN_OPTIONS.into_iter().enumerate() {
            if compact && index == 3 {
                ui.end_row();
            }
            let selected = current == Some(align);
            let response = dtool_design::with_icon_selection(ui, |ui| {
                ui.add(egui::Button::new("").selected(selected).min_size(egui::vec2(24.0, height)))
                    .on_hover_text(text(label))
            });
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, text(label)));
            let colour = if selected && response.enabled() { dtool_design::selected_icon_ink(ui) }
                else { ui.style().interact(&response).fg_stroke.color };
            let centre = response.rect.center();
            for line in 0..4 {
                let short = match align {
                    Align::JustifyAll => false,
                    Align::JustifyLeft | Align::JustifyCenter | Align::JustifyRight => line == 3,
                    _ => line % 2 == 1,
                };
                let width = if short { 8.0 } else { 14.0 };
                let left = match align {
                    Align::Center | Align::JustifyCenter => centre.x - width / 2.0,
                    Align::Right | Align::JustifyRight => centre.x + 7.0 - width,
                    _ => centre.x - 7.0,
                };
                ui.painter().hline(left..=left + width, centre.y - 4.5 + line as f32 * 3.0, egui::Stroke::new(1.0, colour));
            }
            if response.clicked() { picked = Some(align); }
        }
    };
    if wrap {
        ui.horizontal_wrapped(draw);
    } else {
        ui.horizontal(draw);
    }
    picked
}
