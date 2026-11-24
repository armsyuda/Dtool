use serde::{Deserialize, Serialize};

use crate::document::Units;
use dtool_protocol::{Command, CommandError};

pub const RULER_SHOW: &str = "ruler.show";
pub const RULER_ORIGIN_SET: &str = "ruler.origin.set";
pub const RULER_ORIGIN_RESET: &str = "ruler.origin.reset";
pub const RULER_UNIT_SET: &str = "ruler.unit.set";
pub const RULER_COMMANDS: [&str; 4] = [RULER_SHOW, RULER_ORIGIN_SET, RULER_ORIGIN_RESET, RULER_UNIT_SET];
pub const MENU_SHOW_RULERS: &str = "menu.view.show.rulers";
pub const MENU_SHOW_TEXT_RULER: &str = "menu.view.show.text_ruler";

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RulerState {
    pub show: bool,
    pub show_text_ruler: bool,
    pub origin: [f32; 2],
    pub units: Option<Units>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub meters: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulerShowArgs {
    #[serde(default)]
    pub show: Option<bool>,
    #[serde(default)]
    pub show_text_ruler: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulerOriginArgs {
    pub origin: [f32; 2],
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulerUnitArgs {
    pub units: Option<RulerUnit>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
pub enum RulerUnit {
    Standard(Units),
    Metric(MetricUnit),
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum MetricUnit {
    #[serde(rename = "M", alias = "m", alias = "Meters", alias = "Metres")]
    Meter,
}

pub fn units_per_px(units: Units, dpi: f32) -> f32 {
    crate::units::from_px(1.0, units, dpi).unwrap_or(1.0)
}

impl RulerState {
    pub fn apply_show(&mut self, args: RulerShowArgs) {
        if let Some(show) = args.show {
            self.show = show;
        }
        if let Some(show_text_ruler) = args.show_text_ruler {
            self.show_text_ruler = show_text_ruler;
        }
    }

    pub fn set_origin(&mut self, origin: [f32; 2]) -> Result<(), CommandError> {
        if origin.iter().all(|v| v.is_finite()) {
            self.origin = origin;
            Ok(())
        } else {
            Err(crate::command::invalid_params_error())
        }
    }

    pub fn reset_origin(&mut self) {
        self.origin = [0.0, 0.0];
    }

    pub fn set_units(&mut self, units: Option<Units>) {
        self.units = units;
        self.meters = false;
    }

    pub fn effective_units(&self, document_units: Units) -> Units {
        self.units.unwrap_or(document_units)
    }

    pub fn display_scale(&self, document_units: Units, dpi: f32) -> f32 {
        let display = if self.meters { units_per_px(Units::Mm, dpi) / 1_000.0 }
            else { units_per_px(self.effective_units(document_units), dpi) };
        display / units_per_px(document_units, dpi)
    }

    pub fn unit_value(&self, fallback: Option<Units>) -> serde_json::Value {
        if self.meters { serde_json::json!("M") } else { serde_json::json!(self.units.or(fallback)) }
    }

    pub fn label(&self, doc_px: f32, axis: usize, scale: f32) -> i64 {
        ((doc_px - self.origin[axis.min(1)]) * scale).round() as i64
    }

    pub fn result(&self, name: &str, generation: u64) -> serde_json::Value {
        serde_json::json!({
            "doc_version": generation,
            "changes": [{ "name": name }],
            "show": self.show,
            "show_text_ruler": self.show_text_ruler,
            "origin": self.origin,
            "units": self.unit_value(None)
        })
    }
}

fn parse<T: for<'de> Deserialize<'de>>(params: &serde_json::Value) -> Result<T, CommandError> {
    let value = if params.is_null() { serde_json::json!({}) } else { params.clone() };
    serde_json::from_value(value).map_err(|_| crate::command::invalid_params_error())
}

fn params_empty(params: &serde_json::Value) -> bool {
    match params {
        serde_json::Value::Null => true,
        serde_json::Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

pub fn is_ruler_command(name: &str) -> bool {
    RULER_COMMANDS.contains(&name)
}

pub fn params_valid(command: &Command) -> bool {
    let mut probe = RulerState::default();
    run(&mut probe, 0, command).is_some_and(|r| r.is_ok())
}

pub fn run(state: &mut RulerState, generation: u64, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    let name = command.name.as_str();
    let params = &command.params;
    let outcome = match name {
        RULER_SHOW => parse::<RulerShowArgs>(params).map(|args| state.apply_show(args)),
        RULER_ORIGIN_SET => parse::<RulerOriginArgs>(params).and_then(|args| state.set_origin(args.origin)),
        RULER_ORIGIN_RESET => {
            if params_empty(params) {
                state.reset_origin();
                Ok(())
            } else {
                Err(crate::command::invalid_params_error())
            }
        }
        RULER_UNIT_SET => parse::<RulerUnitArgs>(params).map(|args| match args.units {
            Some(RulerUnit::Standard(units)) => state.set_units(Some(units)),
            Some(RulerUnit::Metric(MetricUnit::Meter)) => { state.set_units(None); state.meters = true; }
            None => state.set_units(None),
        }),
        _ => return None,
    };
    Some(outcome.map(|_| state.result(name, generation)))
}
