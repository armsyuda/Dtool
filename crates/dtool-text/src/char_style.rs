use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kerning {
    Auto,
    Optical,
    Manual(f32),
}

impl Default for Kerning {
    fn default() -> Self {
        Kerning::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Caps {
    None,
    SmallCaps,
    AllCaps,
    AllSmallCaps,
    PetiteCaps,
}

impl Default for Caps {
    fn default() -> Self {
        Caps::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseOp {
    Lower,
    Upper,
    Toggle,
    Title,
    Sentence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Position {
    Normal,
    Superscript,
    Subscript,
}

impl Default for Position {
    fn default() -> Self {
        Position::Normal
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpticalAlign {
    None,
    Left,
    Right,
    Both,
}

impl Default for OpticalAlign {
    fn default() -> Self {
        OpticalAlign::None
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decoration {
    pub on: bool,
    #[serde(default, deserialize_with = "dtool_core::paint::legacy_fill::deserialize")]
    pub colour: Option<dtool_core::paint::Paint>,
    pub weight: Option<f32>,
    pub offset: Option<f32>,
}

impl Default for Decoration {
    fn default() -> Self {
        Decoration {
            on: false,
            colour: None,
            weight: None,
            offset: None,
        }
    }
}

pub type CharStyle = crate::story::CharStyle;

pub type CharPatch = crate::story::CharPatch;

impl CharPatch {
    pub fn apply(&self, style: &mut CharStyle) {
        if let Some(value) = &self.family { style.family = value.clone(); }
        if let Some(value) = &self.style { style.style = value.clone(); }
        if let Some(value) = &self.size_pt { style.size_pt = value.clone(); }
        if let Some(value) = &self.fill { style.fill = value.clone(); }
        if let Some(v) = self.weight { style.weight = v; }
        if let Some(v) = self.italic { style.italic = v; }
        if let Some(v) = self.h_scale { style.h_scale = v; }
        if let Some(v) = self.v_scale { style.v_scale = v; }
        if let Some(v) = self.shear_deg { style.shear_deg = v; }
        if let Some(v) = self.baseline_pt { style.baseline_pt = v; }
        if let Some(v) = self.position.clone() { style.position = v; }
        if let Some(v) = self.kerning.clone() { style.kerning = v; }
        if let Some(v) = self.tracking { style.tracking = v; }
        if let Some(v) = self.leading { style.leading = v; }
        if let Some(v) = self.caps { style.caps = v; }
        if let Some(v) = self.underline.clone() { style.underline = v; }
        if let Some(v) = self.strike.clone() { style.strike = v; }
        if let Some(v) = self.outline.clone() { style.outline = v; }
        if let Some(v) = self.background.clone() { style.background = v; }
        if let Some(v) = self.ligatures { style.ligatures = v; }
        if let Some(v) = self.opentype.clone() { style.opentype = v; }
        if let Some(v) = self.language.clone() { style.language = v; }
    }
}

pub fn apply_case(text: &str, op: CaseOp, lang: &str) -> String {
    match op {
        CaseOp::Lower => text.to_lowercase(),
        CaseOp::Upper => text.to_uppercase(),
        CaseOp::Toggle => {
            let mut out = String::with_capacity(text.len());
            for c in text.chars() {
                if c.is_uppercase() {
                    out.extend(c.to_lowercase());
                } else if c.is_lowercase() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
            }
            out
        }
        CaseOp::Title => {
            let mut out = String::with_capacity(text.len());
            let mut prev_is_letter = false;
            for c in text.chars() {
                if c.is_whitespace() || !c.is_alphanumeric() {
                    prev_is_letter = false;
                    out.push(c);
                } else if !prev_is_letter {
                    out.extend(c.to_uppercase());
                    prev_is_letter = true;
                } else {
                    out.extend(c.to_lowercase());
                    prev_is_letter = true;
                }
            }
            let _ = lang;
            out
        }
        CaseOp::Sentence => {
            let mut out = String::with_capacity(text.len());
            let mut at_start = true;
            for c in text.chars() {
                if c.is_whitespace() || !c.is_alphanumeric() {
                    out.push(c);
                    if c == '.' || c == '!' || c == '?' {
                        at_start = true;
                    }
                } else if at_start {
                    out.extend(c.to_uppercase());
                    at_start = false;
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            let _ = lang;
            out
        }
    }
}

pub fn tracking_tighten(current: f32) -> f32 {
    current - 20.0
}

pub fn tracking_loosen(current: f32) -> f32 {
    current + 20.0
}

pub fn tracking_tighten_more(current: f32) -> f32 {
    current - 100.0
}

pub fn tracking_loosen_more(current: f32) -> f32 {
    current + 100.0
}

pub fn tracking_use_default() -> f32 {
    0.0
}

pub fn tracking_use_none() -> f32 {
    0.0
}

pub fn baseline_raise(current: f32) -> f32 {
    current + 1.0
}

pub fn baseline_lower(current: f32) -> f32 {
    current - 1.0
}

pub fn baseline_precise_raise(current: f32) -> f32 {
    current + 0.1
}

pub fn baseline_precise_lower(current: f32) -> f32 {
    current - 0.1
}

pub fn baseline_use_default() -> f32 {
    0.0
}

pub fn toggle_underline(decoration: &mut Decoration) {
    decoration.on = !decoration.on;
}

pub fn underline_colour(decoration: &Decoration, text_fill: &str) -> String {
    decoration.colour_str().unwrap_or_else(|| text_fill.to_string())
}

pub fn underline_weight(decoration: &Decoration, font_post_weight: f32) -> f32 {
    decoration.weight.unwrap_or(font_post_weight)
}

pub fn underline_offset(decoration: &Decoration, font_post_offset: f32) -> f32 {
    decoration.offset.unwrap_or(font_post_offset)
}

pub fn toggle_strikethrough(decoration: &mut Decoration) {
    decoration.on = !decoration.on;
}

pub fn strikethrough_colour(decoration: &Decoration, text_fill: &str) -> String {
    decoration.colour_str().unwrap_or_else(|| text_fill.to_string())
}

pub fn strikethrough_weight(decoration: &Decoration, font_strikeout_size: f32) -> f32 {
    decoration.weight.unwrap_or(font_strikeout_size)
}

pub fn strikethrough_offset(decoration: &Decoration, font_strikeout_position: f32) -> f32 {
    decoration.offset.unwrap_or(font_strikeout_position)
}

pub fn has_opentype_subs(style: &CharStyle) -> bool {
    style.opentype.iter().any(|(tag, _)| tag == "subs")
}

pub fn has_opentype_sups(style: &CharStyle) -> bool {
    style.opentype.iter().any(|(tag, _)| tag == "sups")
}

pub fn subscript_size(size_pt: f32) -> f32 {
    size_pt * 0.583
}

pub fn subscript_shift(size_pt: f32) -> f32 {
    -size_pt * 0.333
}

pub fn superscript_size(size_pt: f32) -> f32 {
    size_pt * 0.583
}

pub fn superscript_shift(size_pt: f32) -> f32 {
    size_pt * 0.333
}

pub fn position_scale(style: &CharStyle) -> (f32, f32) {
    match style.position {
        Position::Normal => (1.0, 0.0),
        Position::Subscript => {
            if has_opentype_subs(style) {
                (1.0, 0.0)
            } else {
                (subscript_size(style.size_pt) / style.size_pt, subscript_shift(style.size_pt))
            }
        }
        Position::Superscript => {
            if has_opentype_sups(style) {
                (1.0, 0.0)
            } else {
                (superscript_size(style.size_pt) / style.size_pt, superscript_shift(style.size_pt))
            }
        }
    }
}

pub fn background_colour(style: &CharStyle) -> Option<String> {
    style.background_str()
}

pub fn background_paint(style: &CharStyle) -> Option<dtool_core::paint::Paint> {
    style.background.clone()
}

fn is_hang_left(ch: char) -> bool {
    matches!(ch, '\u{2018}' | '\u{201C}' | '\u{00AB}' | '\u{2039}' | '(' | '[' | '{' | '\u{00A1}')
}

fn is_hang_right(ch: char) -> bool {
    matches!(ch, '\u{2019}' | '\u{201D}' | '\u{00BB}' | '\u{203A}' | ')' | ']' | '}' | '.' | ',' | ';' | ':' | '?' | '!' | '\u{00BF}')
}

pub fn optical_hang_amount(ch: char, size_pt: f32) -> Option<f32> {
    if is_hang_left(ch) || is_hang_right(ch) {
        Some(size_pt * 0.15)
    } else {
        None
    }
}

pub fn compute_optical_hang(text: &str, size_pt: f32, align: OpticalAlign) -> (f32, f32) {
    if align == OpticalAlign::None || text.is_empty() {
        return (0.0, 0.0);
    }

    let chars: Vec<char> = text.chars().collect();
    let mut left_hang = 0.0f32;
    let mut right_hang = 0.0f32;

    if align == OpticalAlign::Left || align == OpticalAlign::Both {
        for &ch in chars.iter() {
            if is_hang_left(ch) {
                left_hang += optical_hang_amount(ch, size_pt).unwrap_or(0.0);
            } else {
                break;
            }
        }
    }

    if align == OpticalAlign::Right || align == OpticalAlign::Both {
        for &ch in chars.iter().rev() {
            if is_hang_right(ch) {
                right_hang += optical_hang_amount(ch, size_pt).unwrap_or(0.0);
            } else {
                break;
            }
        }
    }

    (left_hang, right_hang)
}

impl Decoration {
    pub fn colour_str(&self) -> Option<String> {
        self.colour.as_ref().and_then(dtool_core::paint::paint_to_legacy_str)
    }

    pub fn set_colour_str(&mut self, value: Option<&str>) {
        self.colour = value.and_then(dtool_core::paint::paint_from_legacy_str);
    }
}

pub fn underline_paint(decoration: &Decoration, text_fill: &dtool_core::paint::Paint) -> dtool_core::paint::Paint {
    decoration.colour.clone().unwrap_or_else(|| text_fill.clone())
}

pub fn strikethrough_paint(decoration: &Decoration, text_fill: &dtool_core::paint::Paint) -> dtool_core::paint::Paint {
    decoration.colour.clone().unwrap_or_else(|| text_fill.clone())
}