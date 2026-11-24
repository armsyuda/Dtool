use dtool_core::pinning::{pin_of, BasePosition, PinSettings, ScaleTo, PIN_FLOAT, PIN_INLINE, PIN_UNPIN};
use dtool_core::{CommandError, ObjectId, Session, StoryId};
use dtool_design::{button, dropdown, number_field, panel_frame, panel_header, tab, toggle};
use dtool_strings::text;
use serde_json::{json, Value};

pub const PINNING_PANEL_KEY: &str = "pinning";
pub const PINNING_MENU_KEY: &str = "menu.window.text.pinning";

const SCALE_OPTIONS: [&str; 3] = ["None", "Font size", "Line height"];
const BASE_OPTIONS: [&str; 4] = ["Baseline", "Top", "Middle", "Bottom"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinMode {
    #[default]
    Inline,
    Float,
}

impl PinMode {
    pub fn command_name(&self) -> &'static str {
        match self {
            PinMode::Inline => PIN_INLINE,
            PinMode::Float => PIN_FLOAT,
        }
    }

    pub fn label_key(&self) -> &'static str {
        match self {
            PinMode::Inline => "pinning.inline",
            PinMode::Float => "pinning.float",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PinningPanelState {
    pub open: bool,
    pub mode: PinMode,
    pub settings: PinSettings,
    pub index: usize,
    pub last_error: Option<String>,
}

impl Default for PinningPanelState {
    fn default() -> Self {
        Self {
            open: true,
            mode: PinMode::Inline,
            settings: PinSettings::default(),
            index: 0,
            last_error: None,
        }
    }
}

impl PinningPanelState {
    pub fn sync_from(&mut self, session: &Session, object: ObjectId) -> bool {
        match pin_of(&session.document, object) {
            Some(record) => {
                self.mode = if record.pin.is_inline() {
                    PinMode::Inline
                } else {
                    PinMode::Float
                };
                self.settings = record.settings;
                self.settings.offset = if record.pin.is_inline() {
                    record.settings.offset
                } else {
                    record.pin.offset()
                };
                self.index = record.pin.index();
                true
            }
            None => false,
        }
    }

    pub fn pin_params(&self, object: ObjectId, story: StoryId) -> Value {
        json!({
            "object": object.0,
            "story": story.0,
            "index": self.index,
            "scale_to": self.settings.scale_to.key(),
            "base_position": self.settings.base_position.key(),
            "offset": [self.settings.offset[0], self.settings.offset[1]],
            "prevent_wrap": self.settings.prevent_wrap,
            "borders": self.settings.borders,
        })
    }

    fn record(&mut self, result: Result<Value, CommandError>) -> Result<Value, CommandError> {
        self.last_error = result.as_ref().err().map(|e| e.message_key.clone());
        result
    }
}

pub fn pin_selected(
    session: &mut Session,
    state: &mut PinningPanelState,
    object: ObjectId,
    story: StoryId,
) -> Result<Value, CommandError> {
    let result = session.run_named(state.mode.command_name(), state.pin_params(object, story));
    state.record(result)
}

pub fn unpin_selected(
    session: &mut Session,
    state: &mut PinningPanelState,
    object: ObjectId,
) -> Result<Value, CommandError> {
    let result = session.run_named(PIN_UNPIN, json!({ "object": object.0 }));
    state.record(result)
}

pub fn show_pinning_panel(
    ui: &mut egui::Ui,
    session: &mut Session,
    state: &mut PinningPanelState,
    object: Option<ObjectId>,
    story: Option<StoryId>,
) -> Option<Result<Value, CommandError>> {
    if !state.open {
        return None;
    }
    let pinned = object
        .map(|o| pin_of(&session.document, o).is_some())
        .unwrap_or(false);
    let target_story = match (pinned, object) {
        (true, Some(o)) => pin_of(&session.document, o).map(|r| r.pin.story()).or(story),
        _ => story,
    };
    panel_frame(ui, |ui| {
        panel_header(ui, text("panel.pinning"));
        let mut action: Option<Result<Value, CommandError>> = None;
        let mut changed = false;

        ui.horizontal(|ui| {
            if tab(ui, state.mode == PinMode::Float, text("pinning.float")).clicked() && state.mode != PinMode::Float {
                state.mode = PinMode::Float;
                changed = true;
            }
            if tab(ui, state.mode == PinMode::Inline, text("pinning.inline")).clicked()
                && state.mode != PinMode::Inline
            {
                state.mode = PinMode::Inline;
                changed = true;
            }
        });

        let mut scale_index = ScaleTo::ALL
            .iter()
            .position(|v| *v == state.settings.scale_to)
            .unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label("Scale to");
            dropdown(ui, "pinning.scale_to", &mut scale_index, &SCALE_OPTIONS);
        });
        if let Some(value) = ScaleTo::ALL.get(scale_index).copied() {
            if value != state.settings.scale_to {
                state.settings.scale_to = value;
                changed = true;
            }
        }

        let mut base_index = BasePosition::ALL
            .iter()
            .position(|v| *v == state.settings.base_position)
            .unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label("Base position");
            dropdown(ui, "pinning.base_position", &mut base_index, &BASE_OPTIONS);
        });
        if let Some(value) = BasePosition::ALL.get(base_index).copied() {
            if value != state.settings.base_position {
                state.settings.base_position = value;
                changed = true;
            }
        }

        ui.horizontal(|ui| {
            ui.label("Offset");
            let mut x = state.settings.offset[0];
            let mut y = state.settings.offset[1];
            number_field(ui, &mut x, "X");
            number_field(ui, &mut y, "Y");
            if x.is_finite() && y.is_finite() && (x != state.settings.offset[0] || y != state.settings.offset[1])
            {
                state.settings.offset = [x, y];
                changed = true;
            }
        });

        let mut prevent_wrap = state.settings.prevent_wrap;
        if toggle(ui, &mut prevent_wrap, "Prevent text wrap").changed() {
            state.settings.prevent_wrap = prevent_wrap;
            changed = true;
        }
        let mut borders = state.settings.borders;
        if toggle(ui, &mut borders, "Borders").changed() {
            state.settings.borders = borders;
            changed = true;
        }

        ui.horizontal(|ui| {
            ui.label("Index");
            ui.add(egui::DragValue::new(&mut state.index));
        });

        let can_pin = object.is_some() && target_story.is_some();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(can_pin, |ui| {
                if button(ui, text(state.mode.label_key())).clicked() {
                    if let (Some(o), Some(s)) = (object, target_story) {
                        action = Some(pin_selected(session, state, o, s));
                    }
                }
            });
            ui.add_enabled_ui(pinned, |ui| {
                if button(ui, text("pinning.unpin")).clicked() {
                    if let Some(o) = object {
                        action = Some(unpin_selected(session, state, o));
                    }
                }
            });
        });

        if action.is_none() && changed && pinned {
            if let (Some(o), Some(s)) = (object, target_story) {
                if let Some(record) = pin_of(&session.document, o) {
                    state.index = record.pin.index();
                }
                action = Some(pin_selected(session, state, o, s));
            }
        }

        if let Some(error) = &state.last_error {
            ui.label(text(error));
        }
        action
    })
    .inner
}
