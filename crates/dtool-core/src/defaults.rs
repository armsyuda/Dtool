use crate::layer_style::{EffectKind, EffectParams, LayerEffect, default_effect};
use crate::paint::{Colour, Paint, StrokeStyle};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextDefaults {
    pub font_family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    pub font_size: f64,
    pub line_height: f64,
    #[serde(default)]
    pub explicit_leading: bool,
    pub alignment: TextAlign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    #[serde(alias = "JustifyLeft")]
    Justify,
    JustifyCenter,
    JustifyRight,
    JustifyAll,
    TowardsSpine,
    AwayFromSpine,
}

impl TextAlign {
    pub(crate) fn story_value(self) -> serde_json::Value {
        if self == Self::Justify {
            return "JustifyLeft".into();
        }
        serde_json::json!(self)
    }
}

impl Default for TextDefaults {
    fn default() -> Self {
        Self {
            font_family: "sans-serif".into(),
            font_style: None,
            font_weight: None,
            italic: None,
            font_size: f64::from(crate::text_cmd::DEFAULT_SIZE_PT),
            line_height: 1.5,
            explicit_leading: false,
            alignment: TextAlign::Left,
        }
    }
}

impl TextDefaults {
    pub(crate) fn from_story(story: &crate::Story) -> Self {
        let mut defaults = Self::default();
        let first = |runs: &[serde_json::Value]| runs.iter().rev().find(|run| {
            run.is_object() && run.get("frame").is_none()
                && run["start"].as_u64() == Some(0)
                && (story.text.is_empty() || run["end"].as_u64().is_some_and(|end| end > 0))
        }).cloned().unwrap_or_default();
        let character = first(&story.ranges);
        let paragraph = first(&story.paras);
        if let Some(family) = character["family"].as_str().filter(|name| !name.trim().is_empty()) {
            defaults.font_family = family.to_owned();
        }
        defaults.font_style = character["style"].as_str().filter(|name| !name.trim().is_empty()).map(str::to_owned);
        defaults.font_weight = character["weight"].as_u64().and_then(|weight| weight.try_into().ok());
        defaults.italic = character["italic"].as_bool();
        if let Some(size) = character["size_pt"].as_f64().filter(|size| (*size as f32).is_finite() && *size as f32 > 0.0) {
            defaults.font_size = size;
        }
        if let Some(leading) = paragraph["leading"].as_f64().filter(|leading| (*leading as f32).is_finite() && *leading as f32 > 0.0) {
            defaults.line_height = leading;
            defaults.explicit_leading = true;
        }
        if let Ok(alignment) = serde_json::from_value(paragraph["align"].clone()) {
            defaults.alignment = alignment;
        }
        defaults
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectDefaults {
    pub shadow_enabled: bool,
    pub shadow_offset_x: f64,
    pub shadow_offset_y: f64,
    pub shadow_blur: f64,
    pub shadow_color: [u8; 4],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<LayerEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_size_pt: Option<[f32; 2]>,
}

impl Default for EffectDefaults {
    fn default() -> Self {
        Self {
            shadow_enabled: false,
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            shadow_blur: 0.0,
            shadow_color: [0, 0, 0, 255],
            stack: Vec::new(),
            reference_size_pt: None,
        }
    }
}

impl EffectDefaults {
    fn sync(object: &crate::Object, units_per_point: f32) -> Self {
        if object.effects.is_empty() {
            return Self::default();
        }
        let mut stack = object.effects.clone();
        for effect in &mut stack {
            effect.scale_lengths(units_per_point.recip());
        }
        Self {
            stack,
            reference_size_pt: Some(object.size.map(|size| size.abs() / units_per_point)),
            ..Self::default()
        }
    }

    pub(crate) fn for_object(&self, units_per_point: f32, size: [f32; 2]) -> Vec<LayerEffect> {
        let mut stack = self.stack.clone();
        if stack.is_empty() && self.shadow_enabled {
            let mut effect = default_effect(EffectKind::OuterShadow);
            if let EffectParams::Shadow { radius, offset, angle, colour, .. } = &mut effect.params {
                *radius = self.shadow_blur as f32;
                *offset = self.shadow_offset_x.hypot(self.shadow_offset_y) as f32;
                *angle = self.shadow_offset_y.atan2(self.shadow_offset_x).to_degrees() as f32;
                let [r, g, b, a] = self.shadow_color;
                *colour = Colour::new(r, g, b, a);
            }
            stack.push(effect);
        }
        let transform = self.reference_size_pt.map(|basis| {
            let axis = |index: usize| {
                let source = basis[index] * units_per_point;
                let ratio = f64::from(size[index].abs()) / f64::from(source);
                if ratio.is_finite() && ratio > 0.0 { ratio } else { 1.0 }
            };
            kurbo::Affine::scale_non_uniform(axis(0), axis(1)).as_coeffs()
        });
        for effect in &mut stack {
            effect.scale_lengths(units_per_point);
            if let Some(matrix) = transform {
                effect.transform_paint(matrix);
            }
        }
        stack.retain(LayerEffect::is_valid);
        stack
    }
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
            text: TextDefaults::default(),
            effects: EffectDefaults::default(),
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
                    self.effects = EffectDefaults::sync(obj, units_per_point);
                    self.unit = Some(DEFAULTS_UNIT_PT.to_string());
                }
                if let Some(story) = obj.story_id().and_then(|id| document.stories.iter().find(|story| story.id == id)) {
                    self.text = TextDefaults::from_story(story);
                }
            }
        }
    }

    pub fn sync_from_object(&mut self, obj: &crate::object::Object, dpi: f32) {
        self.fill = obj.fill.clone();
        self.stroke = convert_stroke(&obj.stroke, |px| crate::units::px_to_pt(px, dpi));
        self.effects = EffectDefaults::sync(obj, crate::units::units_per_point(crate::document::Units::Px, dpi));
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
        obj.effects = self.effects.for_object(crate::units::units_per_point(crate::document::Units::Px, dpi), obj.size);
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
        crate::native_io::write_atomic(path, json.as_bytes()).map_err(|error| error.write_error().code)
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
    StrokeStyle { width: convert(stroke.width), dash: stroke.dash.iter().map(|value| convert(*value)).collect(), dash_offset: convert(stroke.dash_offset), ..stroke.clone() }
}
