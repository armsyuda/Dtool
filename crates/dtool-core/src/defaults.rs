use crate::paint::{Colour, Paint, StrokeStyle};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextDefaults {
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub alignment: TextAlign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectDefaults {
    pub shadow_enabled: bool,
    pub shadow_offset_x: f64,
    pub shadow_offset_y: f64,
    pub shadow_blur: f64,
    pub shadow_color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectDefaults {
    pub fill: Paint,
    pub stroke: StrokeStyle,
    pub text: TextDefaults,
    pub effects: EffectDefaults,
    #[serde(default)]
    pub unit: Option<String>,
}

pub const DEFAULTS_UNIT_PT: &str = "pt";
pub const LEGACY_DEFAULTS_DPI: f32 = 96.0;

impl Default for ObjectDefaults {
    fn default() -> Self {
        Self {
            fill: Paint::None,
            stroke: Self::default_stroke(),
            text: TextDefaults {
                font_family: String::from("sans-serif"),
                font_size: 12.0,
                line_height: 1.5,
                alignment: TextAlign::Left,
            },
            effects: EffectDefaults {
                shadow_enabled: false,
                shadow_offset_x: 0.0,
                shadow_offset_y: 0.0,
                shadow_blur: 0.0,
                shadow_color: [0, 0, 0, 255],
            },
            unit: Some(DEFAULTS_UNIT_PT.to_string()),
        }
    }
}

impl ObjectDefaults {
    pub fn default_stroke() -> StrokeStyle {
        StrokeStyle {
            paint: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
            width: 1.0,
            ..StrokeStyle::default()
        }
    }

    pub fn sync_from_selection(&mut self, document: &crate::document::Document, selection: &crate::Selection) {
        if let Some(obj_id) = selection.ids.first() {
            if let Some(obj) = document.get_object(*obj_id) {
                if document.units == crate::document::Units::Px {
                    self.sync_from_object(obj, document.dpi);
                } else {
                    let units_per_point = crate::units::units_per_point(document.units, document.dpi);
                    self.fill = obj.fill.clone();
                    self.stroke = convert_stroke(&obj.stroke, |length| length / units_per_point);
                    self.unit = Some(DEFAULTS_UNIT_PT.to_string());
                }
            }
        }
    }

    pub fn sync_from_object(&mut self, obj: &crate::object::Object, dpi: f32) {
        self.fill = obj.fill.clone();
        self.stroke = convert_stroke(&obj.stroke, |px| crate::units::px_to_pt(px, dpi));
        self.unit = Some(DEFAULTS_UNIT_PT.to_string());
    }

    pub fn apply_to(&self, obj: &mut crate::object::Object, dpi: f32) {
        self.apply_in_document(obj, dpi);
    }

    pub fn stroke_for(&self, dpi: f32) -> StrokeStyle {
        convert_stroke(&self.stroke, |pt| crate::units::pt_to_px(pt, dpi))
    }

    pub fn stroke_for_document(&self, document: &crate::document::Document) -> StrokeStyle {
        if document.units == crate::document::Units::Px {
            return self.stroke_for(document.dpi);
        }
        let units_per_point = crate::units::units_per_point(document.units, document.dpi);
        convert_stroke(&self.stroke, |pt| pt * units_per_point)
    }

    pub fn apply_in_document(&self, obj: &mut crate::object::Object, dpi: f32) {
        obj.fill = self.fill.clone();
        obj.stroke = self.stroke_for(dpi);
    }

    pub fn with_pt_unit(mut self) -> Self {
        self.unit = Some(DEFAULTS_UNIT_PT.to_string());
        self
    }

    pub fn from_json(value: serde_json::Value) -> Option<Self> {
        let legacy = value.get("unit").map_or(true, serde_json::Value::is_null);
        let mut defaults: Self = serde_json::from_value(value).ok()?;
        if legacy {
            defaults.stroke = convert_stroke(&defaults.stroke, |px| crate::units::px_to_pt(px, LEGACY_DEFAULTS_DPI));
            return Some(defaults.with_pt_unit());
        }
        (defaults.unit.as_deref() == Some(DEFAULTS_UNIT_PT)).then_some(defaults)
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn save_app(&self) -> Result<(), String> {
        let path = Self::settings_path()?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json).map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_app() -> Result<Self, String> {
        let path = Self::settings_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let defaults = serde_json::from_str::<serde_json::Value>(&content).ok().and_then(Self::from_json);
        Ok(defaults.unwrap_or_default())
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn revert(&mut self, previous: &Self) {
        *self = previous.clone();
    }

    pub(crate) fn settings_path() -> Result<PathBuf, String> {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.is_empty() {
                return Ok(PathBuf::from(appdata).join("dtool").join("defaults.json"));
            }
        }
        let dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from);

        match dir {
            Ok(d) => Ok(d.join(".config").join("dtool").join("defaults.json")),
            Err(_) => Err(String::from("CONFIG_DIR_NOT_FOUND")),
        }
    }
}

fn convert_stroke(stroke: &StrokeStyle, convert: impl Fn(f32) -> f32) -> StrokeStyle {
    StrokeStyle { width: convert(stroke.width), dash: stroke.dash.iter().map(|value| convert(*value)).collect(), ..stroke.clone() }
}
