use crate::context_bar::{self, ContextBarModel, Stamp};
use crate::icon_cache::IconCache;
use crate::panels::{character, paragraph, text_frame};
use dtool_core::{Command, CommandError, ObjectKind, Session, StoryId};
use dtool_design::{numeric_field, numeric_state, NumericEvent, NumericSpec, NumericValue};
use dtool_tools::dtool_text::{Align, FontLibrary};
use dtool_strings::text;
use serde_json::{json, Value};

pub const BOLD_WEIGHT: u16 = 700;
pub const REGULAR_WEIGHT: u16 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextTarget {
    pub story: u64,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextBarEdit {
    Family(String),
    FontStyle(String),
    Size(f32),
    Bold(bool),
    Italic(bool),
    Align(Align),
    VerticalAlign(&'static str),
    Tracking(f32),
    PreviewNumber(Box<TextBarEdit>),
    CancelNumber,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FontHover {
    #[default]
    Closed,
    Open(Option<String>),
}

#[derive(Clone, Copy, Debug, Hash)]
enum TextControl {
    Family,
    Style,
    Size,
    Tracking,
    Emphasis,
    Alignment,
    FrameAlignment,
}

impl TextControl {
    fn menu_label(self) -> &'static str {
        text(match self {
            Self::Family => "context.text.font_family",
            Self::Style => "context.text.font_style",
            Self::Size => "context.text.font_size",
            Self::Tracking => "ctx.text.tracking",
            Self::Emphasis => "ctx.text.char_style",
            Self::Alignment => "context.text.align",
            Self::FrameAlignment => "context.text.vertical_align",
        })
    }

    fn separator(self) -> bool {
        matches!(self, Self::Emphasis | Self::Alignment | Self::FrameAlignment)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBarState {
    pub target: Option<TextTarget>,
    pub(crate) targets: Vec<TextTarget>,
    size_scales: Vec<f64>,
    pub family: Option<String>,
    pub style: Option<String>,
    pub size_pt: Option<f32>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub tracking: Option<f32>,
    pub paragraph_ranges: Vec<(StoryId, [usize; 2])>,
    pub alignment: Option<Align>,
    pub frames: Option<text_frame::FrameSelection>,
}

fn story_len(session: &Session, story: u64) -> Option<usize> {
    session.document.stories.iter().find(|s| s.id.0 == story).map(|s| s.text.chars().count())
}

pub fn target_of(session: &Session) -> Option<TextTarget> {
    if let Some(edit) = session.text_edit.as_ref() {
        let story = edit.story.0;
        let (start, end) = (edit.caret.min(edit.anchor), edit.caret.max(edit.anchor));
        if start < end {
            return Some(TextTarget { story, start, end });
        }
        let len = story_len(session, story)?;
        return Some(TextTarget { story, start: 0, end: len });
    }
    let story = session.selection.ids.iter().find_map(|id| match &session.document.get_object(*id)?.kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story, .. } => Some(story.0),
        ObjectKind::PathText(data) => Some(data.story.0),
        _ => None,
    })?;
    let len = story_len(session, story)?;
    (len > 0).then_some(TextTarget { story, start: 0, end: len })
}

fn agreed<T: PartialEq + Clone>(values: impl Iterator<Item = T>) -> Option<T> {
    let mut out: Option<T> = None;
    for value in values {
        match &out {
            None => out = Some(value),
            Some(seen) if *seen == value => {}
            Some(_) => return None,
        }
    }
    out
}

pub(crate) fn font_size_scale(session: &Session, story: StoryId) -> f64 {
    let scale_of = |id| {
        let object = session.document.get_object(id)?;
        (object.story_id() == Some(story)).then(|| match object.kind {
            ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } => f64::from(object.text_scale[1]),
            _ => 1.0,
        })
    };
    if let Some(scale) = session.text_edit.as_ref().filter(|edit| edit.story == story)
        .and_then(|edit| edit.object).and_then(scale_of)
    {
        return scale;
    }
    agreed(session.selection.ids.iter().copied().filter_map(scale_of)).unwrap_or(1.0)
}

fn range_params(target: &TextTarget) -> Value {
    json!({ "story": target.story, "range": { "start": target.start, "end": target.end } })
}

struct TargetStyle {
    family: Option<String>,
    style: Option<String>,
    size_pt: Option<f32>,
    weight: Option<u16>,
    italic: Option<bool>,
    tracking: Option<f32>,
}

impl TargetStyle {
    fn load(session: &mut Session, target: &TextTarget) -> Result<Self, CommandError> {
        let reply = session.run(&Command::with_params("text.style.query", range_params(target)))?;
        let family = match reply["family"].as_str() {
            Some("Mixed") => None,
            None | Some("") => Some("sans-serif".to_owned()),
            Some(name) => Some(name.to_owned()),
        };
        let runs: Vec<&Value> = session
            .document
            .stories
            .iter()
            .find(|s| s.id.0 == target.story)
            .map(|s| {
                s.ranges
                    .iter()
                    .filter(|r| {
                        let start = r.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let end = r.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
                        r.get("frame").is_none() && ((start < target.end && end > target.start)
                            || (s.text.is_empty() && start == 0 && end == 0))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let unstyled_empty = runs.is_empty() && session.document.stories.iter()
            .any(|story| story.id.0 == target.story && story.text.is_empty());
        let weight = reply["weight"].as_u64().map(|v| v as u16);
        let italic = agreed(runs.iter().map(|r| r.get("italic").and_then(Value::as_bool).unwrap_or(false)));
        let style = match reply["style"].as_str() {
            Some("Mixed") => None,
            None | Some("") => Some(dtool_tools::dtool_text::fonts::style_name(weight.unwrap_or(REGULAR_WEIGHT), italic.unwrap_or(false))),
            Some(name) => Some(name.to_owned()),
        };
        Ok(Self {
            family,
            style,
            size_pt: reply.get("size_pt").and_then(Value::as_f64).map(|v| v as f32)
                .or_else(|| unstyled_empty.then_some(dtool_core::text_cmd::DEFAULT_SIZE_PT)),
            weight,
            italic,
            tracking: agreed(runs.iter().map(|r| r.get("tracking").and_then(Value::as_f64).unwrap_or(0.0).to_bits())).map(f64::from_bits).map(|v| v as f32),
        })
    }
}

impl TextBarState {
    pub fn load(session: &mut Session) -> Result<Self, CommandError> {
        let frames = text_frame::read_selection(session);
        let targets: Vec<_> = if session.text_edit.is_some() {
            target_of(session).into_iter().collect()
        } else {
            session.selected_text_stories().into_iter().filter_map(|story| {
                let end = story_len(session, story.0)?;
                Some(TextTarget { story: story.0, start: 0, end })
            }).collect()
        };
        let styles = targets.iter().map(|target| TargetStyle::load(session, target))
            .collect::<Result<Vec<_>, _>>()?;
        let size_scales: Vec<_> = targets.iter()
            .map(|target| font_size_scale(session, StoryId(target.story)))
            .collect();
        let size_pt = agreed(styles.iter().zip(&size_scales)
            .map(|(style, scale)| style.size_pt.map(|size| (f64::from(size) * scale) as f32))).flatten();
        let paragraph_ranges = paragraph::active_ranges(session);
        let alignment = paragraph::query_style(session)
            .filter(|_| !paragraph::mixed_fields(session).contains(&"align"))
            .map(|style| style.align);
        Ok(Self {
            target: targets.first().copied(),
            targets,
            size_scales,
            paragraph_ranges,
            alignment,
            frames,
            family: agreed(styles.iter().map(|s| s.family.as_ref())).flatten().cloned(),
            style: agreed(styles.iter().map(|s| s.style.as_ref())).flatten().cloned(),
            size_pt,
            weight: agreed(styles.iter().map(|s| s.weight)).flatten(),
            italic: agreed(styles.iter().map(|s| s.italic)).flatten(),
            tracking: agreed(styles.iter().map(|s| s.tracking)).flatten(),
        })
    }

    pub fn has_controls(&self) -> bool {
        self.target.is_some() || self.frames.is_some()
    }

    pub fn bold(&self) -> Option<bool> {
        self.weight.map(|w| w >= BOLD_WEIGHT)
    }

    pub fn command(&self, edit: &TextBarEdit) -> Option<Command> {
        if let TextBarEdit::VerticalAlign(align) = edit {
            return self.frames.as_ref()?.set("valign", json!(align));
        }
        if let TextBarEdit::Align(align) = edit {
            return paragraph::patch_command(&self.paragraph_ranges, json!({ "align": align }));
        }
        let patch = match edit {
            TextBarEdit::Family(family) if !family.trim().is_empty() => {
                crate::font_combo::family_patch(&FontLibrary::shared(), family.trim())
            }
            TextBarEdit::FontStyle(style) => {
                crate::font_combo::style_patch(&FontLibrary::shared(), self.family.as_deref()?, style)?
            }
            TextBarEdit::Size(size) if size.is_finite() && *size > 0.0 => json!({ "size_pt": size }),
            TextBarEdit::Bold(on) => json!({ "style": null, "weight": if *on { BOLD_WEIGHT } else { REGULAR_WEIGHT } }),
            TextBarEdit::Italic(on) => json!({ "style": null, "italic": on }),
            TextBarEdit::Tracking(value) if value.is_finite() => json!({ "tracking": value }),
            _ => return None,
        };
        character::batched(self.targets.iter().zip(&self.size_scales).map(|(target, scale)| {
            let mut params = range_params(target);
            params["patch"] = match edit {
                TextBarEdit::Size(size) => json!({ "size_pt": f64::from(*size) / scale }),
                _ => patch.clone(),
            };
            Command::with_params("text.style", params)
        }).collect())
    }

    pub fn apply(&mut self, session: &mut Session, edit: &TextBarEdit) -> Result<bool, CommandError> {
        let Some(command) = self.command(edit) else {
            return Ok(false);
        };
        session.run(&command)?;
        *self = Self::load(session)?;
        Ok(true)
    }

    pub fn preview_command(&self, family: &str) -> Option<Command> {
        let mut params = match self.targets.as_slice() {
            [] => return None,
            [target] => range_params(target),
            targets => json!({ "targets": targets.iter().map(range_params).collect::<Vec<_>>() }),
        };
        params["family"] = json!(family);
        Some(Command::with_params("text.font.preview", params))
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, families: &[String]) -> Option<TextBarEdit> {
        self.ui_with_hover(ui, families).0
    }

    pub fn ui_with_hover(&mut self, ui: &mut egui::Ui, families: &[String]) -> (Option<TextBarEdit>, FontHover) {
        let (edit, hover, _) = ui.horizontal(|ui| {
            self.ui_with_hover_icons(ui, families, &mut IconCache::new(), None)
        }).inner;
        (edit, hover)
    }

    pub(crate) fn ui_with_hover_icons(
        &mut self,
        ui: &mut egui::Ui,
        families: &[String],
        icons: &mut IconCache,
        trailing: Option<(&ContextBarModel, Option<Stamp>)>,
    ) -> (Option<TextBarEdit>, FontHover, Option<Command>) {
        use TextControl::*;
        let mut controls = vec![Family, Style, Size, Tracking, Emphasis, Alignment];
        if self.frames.is_some() {
            controls.push(FrameAlignment);
        }
        let available = ui.available_width();
        let spacing = ui.spacing().item_spacing.x;
        let mut measure = ui.new_child(egui::UiBuilder::new()
            .id_salt("text.controls.measure")
            .max_rect(egui::Rect::from_min_size(ui.cursor().min, egui::vec2(10_000.0, ui.available_height())))
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .invisible()
            .sizing_pass());
        let origin = measure.min_rect().left();
        let ends: Vec<_> = controls.iter().map(|&control| {
            if control.separator() {
                measure.separator();
            }
            self.draw_control(&mut measure, control, families, icons, false);
            measure.min_rect().right() - origin
        }).collect();
        let reserve = context_bar::MORE_WIDTH + spacing;
        let overflow = ends.last().is_some_and(|width| *width + if trailing.is_some() { reserve } else { 0.0 } > available);
        let visible = if overflow {
            ends.iter().take_while(|width| **width + reserve <= available).count()
        } else {
            controls.len()
        };
        let mut out = None;
        let mut hover = FontHover::Closed;
        let mut trailing_command = None;
        let mut draw = |ui: &mut egui::Ui, control: TextControl, popup| {
            let (edit, family_hover) = ui.push_id(control, |ui| self.draw_control(ui, control, families, icons, popup)).inner;
            if edit.is_some() {
                out = edit;
            }
            if family_hover != FontHover::Closed {
                hover = family_hover;
            }
        };
        for &control in &controls[..visible] {
            if control.separator() {
                ui.separator();
            }
            draw(ui, control, false);
        }
        let popup_state = ui.make_persistent_id("text.controls.overflow");
        if overflow {
            let more = context_bar::more_button(ui);
            // Font menus use egui's popup memory. Keep their parent open while
            // a nested menu handles clicks, including font search and favorites.
            let mut open = ui.data(|data| data.get_temp::<bool>(popup_state)).unwrap_or(false);
            if more.clicked() {
                open = !open;
            }
            let nested_open = egui::Popup::is_any_open(ui.ctx());
            let menu = egui::Popup::menu(&more)
                .open_bool(&mut open)
                .align(egui::RectAlign::BOTTOM_END)
                .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                .show(|ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ui.ctx().content_rect().height() * 0.55).min(360.0))
                        .show(ui, |ui| {
                            ui.set_min_width(180.0);
                            for &control in &controls[visible..] {
                                ui.horizontal(|ui| {
                                    if !matches!(control, Size | Tracking) {
                                        ui.label(control.menu_label());
                                    }
                                    draw(ui, control, true);
                                });
                            }
                            if let Some((model, stamp)) = trailing {
                                ui.separator();
                                trailing_command = context_bar::show_menu_items(ui, &model.items)
                                    .and_then(|edit| model.bind(&edit, stamp));
                            }
                        });
                });
            if !nested_open && !more.clicked() && menu.is_some_and(|menu| menu.response.clicked_elsewhere()) {
                open = false;
            }
            ui.data_mut(|data| data.insert_temp(popup_state, open));
        } else {
            ui.data_mut(|data| data.remove::<bool>(popup_state));
            if let Some((model, stamp)) = trailing {
                trailing_command = context_bar::show_toolbar_options(ui, model, stamp, icons, true);
            }
        }
        (out, hover, trailing_command)
    }

    fn draw_control(
        &self,
        ui: &mut egui::Ui,
        control: TextControl,
        families: &[String],
        icons: &mut IconCache,
        menu: bool,
    ) -> (Option<TextBarEdit>, FontHover) {
        use TextControl::*;
        let mut hover = FontHover::Closed;
        let mixed = text("ctx.text.mixed");
        let enabled = match control {
            Alignment => !self.paragraph_ranges.is_empty(),
            FrameAlignment => self.frames.is_some(),
            _ => self.target.is_some(),
        };
        let edit = ui.add_enabled_ui(enabled, |ui| match control {
            Family => {
                let shown = self.family.clone().unwrap_or_else(|| mixed.to_owned());
                let key = egui::Id::unique("ctx.text.font_family.state");
                let mut fonts = ui.data(|data| data.get_temp::<crate::font_combo::FontComboState>(key)).unwrap_or_default();
                let families: Vec<&str> = if ui.is_sizing_pass() { Vec::new() } else { families.iter().map(String::as_str).collect() };
                let (picked, preview) = crate::font_combo::family_menu(ui, "ctx.text.font_family", &mut fonts, &families, &shown);
                if !ui.is_sizing_pass() {
                    ui.data_mut(|data| data.insert_temp(key, fonts));
                }
                if let Some(hovered) = preview {
                    hover = FontHover::Open(hovered);
                }
                picked.map(TextBarEdit::Family)
            }
            Style => {
                let mut style = self.style.clone().unwrap_or_else(|| mixed.to_owned());
                crate::font_combo::style_combo(
                    ui,
                    "ctx.text.font_style",
                    &FontLibrary::shared(),
                    self.family.as_deref().unwrap_or_default(),
                    &mut style,
                ).then_some(TextBarEdit::FontStyle(style))
            }
            Size => {
                let size = NumericSpec::new(text("char.size")).range(1.0..=1296.0)
                    .suffix("pt").width(56.0).mixed_hint(mixed)
                    .reset_to(f64::from(dtool_core::text_cmd::DEFAULT_SIZE_PT));
                self.number(ui, "char.size", self.size_pt, &size, TextBarEdit::Size)
            }
            Tracking => {
                let tracking = NumericSpec::new(text("ctx.text.tracking")).range(-1000.0..=10000.0)
                    .width(43.0).mixed_hint(mixed).reset_to(0.0);
                self.number(ui, "ctx.text.tracking", self.tracking, &tracking, TextBarEdit::Tracking)
            }
            Emphasis => {
                let bold = self.bold().unwrap_or(false);
                let italic = self.italic.unwrap_or(false);
                let bold_clicked = style_toggle(ui, icons, "ctx-text-bold", "ctx.text.bold", bold).clicked();
                let italic_clicked = style_toggle(ui, icons, "ctx-text-italic", "ctx.text.italic", italic).clicked();
                bold_clicked.then_some(TextBarEdit::Bold(!bold))
                    .or(italic_clicked.then_some(TextBarEdit::Italic(!italic)))
            }
            Alignment => paragraph::alignment_buttons(ui, self.alignment, false).map(TextBarEdit::Align),
            FrameAlignment => {
                let current = self.frames.as_ref().and_then(text_frame::FrameSelection::valign);
                let mut edit = None;
                for ((align, label), icon) in text_frame::VALIGNS.into_iter().zip([
                    "ctx-text-valign-top", "ctx-text-valign-center", "ctx-text-valign-bottom", "ctx-text-valign-justify",
                ]) {
                    if style_toggle(ui, icons, icon, label, current.as_deref() == Some(align)).clicked() {
                        edit = Some(TextBarEdit::VerticalAlign(align));
                    }
                }
                edit
            }
        }).inner;
        (edit, hover)
    }

    fn number(&self, ui: &mut egui::Ui, key: &str, current: Option<f32>, spec: &NumericSpec, edit: fn(f32) -> TextBarEdit) -> Option<TextBarEdit> {
        let id = ui.make_persistent_id((key, &self.targets));
        let state = numeric_state(ui.ctx(), id);
        let display = if state.scrubbing {
            NumericValue::Value(state.scrub)
        } else {
            current.map_or(NumericValue::Mixed, |value| NumericValue::Value(f64::from(value)))
        };
        let (response, event) = numeric_field(ui, id, display, spec);
        response.on_hover_text(text(key));
        match event {
            NumericEvent::Preview(value) => Some(TextBarEdit::PreviewNumber(Box::new(edit(value as f32)))),
            NumericEvent::Commit(value) if current != Some(value as f32) => Some(edit(value as f32)),
            NumericEvent::Commit(_) | NumericEvent::Cancel => Some(TextBarEdit::CancelNumber),
            NumericEvent::None => None,
        }
    }
}

fn style_toggle(ui: &mut egui::Ui, icons: &mut IconCache, icon: &str, key: &str, selected: bool) -> egui::Response {
    let label = text(key);
    let colour = if selected { ui.visuals().selection.stroke.color } else { dtool_design::icons::current_icon_colour() };
    let state = dtool_design::ControlState::default().tooltip(label);
    let response = if let Some(texture) = icons.texture(ui.ctx(), icon, colour, dtool_design::icon_texture_px(ui)) {
        dtool_design::icon_image_button_selected(ui, &texture, selected, &state)
    } else {
        ui.add(egui::Button::new(label).selected(selected)).on_hover_text(label)
    };
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    response
}
