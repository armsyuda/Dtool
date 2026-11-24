use crate::char_style::{Kerning, Caps, Position, Decoration, OpticalAlign};
use serde::{Deserialize, Serialize};
use crate::para_style::{DropCap, FlowStart, Hyphenation, InitialWords, Justification, ListProps, ParagraphDecoration, SpaceBeforeRule, Special, TabStop};
use crate::shape::ShapedGlyph;

fn default_char_weight() -> u16 { 400 }
fn default_char_scale() -> f32 { 1.0 }
fn default_char_language() -> String { "ko-KR".to_string() }
fn default_tab_interval() -> f32 { 36.0 }

fn query_field<T: Clone + PartialEq>(styles: &[&CharStyle], value: impl Fn(&CharStyle) -> T) -> CharPatchField<T> {
    let Some(first) = styles.first() else { return CharPatchField::Mixed; };
    let first = value(first);
    if styles.iter().skip(1).all(|style| value(style) == first) { CharPatchField::Same(first) } else { CharPatchField::Mixed }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct StoryId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl TextRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
    JustifyLeft,
    JustifyCenter,
    JustifyRight,
    JustifyAll,
    TowardsSpine,
    AwayFromSpine,
}

impl Default for Align {
    fn default() -> Self {
        Align::Left
    }
}

impl Align {
    pub fn is_justified(self) -> bool {
        matches!(self, Self::JustifyLeft | Self::JustifyCenter | Self::JustifyRight | Self::JustifyAll)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharStyle {
    pub family: String,
    pub style: String,
    pub size_pt: f32,
    #[serde(default, deserialize_with = "dtool_core::paint::legacy_fill::deserialize")]
    pub fill: Option<dtool_core::paint::Paint>,
    #[serde(default)]
    pub tracking: f32,
    #[serde(default)]
    pub leading: Option<f32>,
    #[serde(default)]
    pub baseline_pt: f32,
    #[serde(default = "default_char_weight")]
    pub weight: u16,
    #[serde(default)]
    pub italic: bool,
    #[serde(default = "default_char_scale")]
    pub h_scale: f32,
    #[serde(default = "default_char_scale")]
    pub v_scale: f32,
    #[serde(default)]
    pub shear_deg: f32,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub kerning: Kerning,
    #[serde(default)]
    pub caps: Caps,
    #[serde(default)]
    pub underline: Decoration,
    #[serde(default)]
    pub strike: Decoration,
    #[serde(default)]
    pub outline: Option<(String, f32)>,
    #[serde(default, deserialize_with = "dtool_core::paint::legacy_fill::deserialize")]
    pub background: Option<dtool_core::paint::Paint>,
    #[serde(default)]
    pub ligatures: Option<bool>,
    #[serde(default)]
    pub opentype: Vec<(String, u32)>,
    #[serde(default = "default_char_language")]
    pub language: String,
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            family: String::new(),
            style: String::new(),
            size_pt: 12.0,
            fill: None,
            tracking: 0.0,
            leading: None,
            baseline_pt: 0.0,
            weight: 400,
            italic: false,
            h_scale: 1.0,
            v_scale: 1.0,
            shear_deg: 0.0,
            position: Position::default(),
            kerning: Kerning::default(),
            caps: Caps::default(),
            underline: Decoration::default(),
            strike: Decoration::default(),
            outline: None,
            background: None,
            ligatures: None,
            opentype: Vec::new(),
            language: "ko-KR".to_string(),

        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParaStyle {
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub optical_align: OpticalAlign,
    #[serde(default = "crate::para_style::default_indent_left")]
    pub indent_left: f32,
    #[serde(default = "crate::para_style::default_indent_right")]
    pub indent_right: f32,
    #[serde(default = "crate::para_style::default_indent_first")]
    pub indent_first: f32,
    #[serde(default = "crate::para_style::default_space_before")]
    pub space_before: f32,
    #[serde(default = "crate::para_style::default_space_after")]
    pub space_after: f32,
    #[serde(default = "crate::para_style::default_space_rule")]
    pub space_rule: SpaceBeforeRule,
    #[serde(default = "crate::para_style::default_leading")]
    pub leading: f32,
    #[serde(default = "default_baseline_grid")]
    pub baseline_grid: bool,
    #[serde(default, deserialize_with = "deserialize_keep_with_next")]
    pub keep_with_next: u32,
    #[serde(default)]
    pub keep_with_previous: bool,
    #[serde(default)]
    pub keep_together: bool,
    #[serde(default)]
    pub prevent_orphans: bool,
    #[serde(default)]
    pub prevent_widows: bool,
    #[serde(default)]
    pub flow_start: FlowStart,
    #[serde(default)]
    pub tabs: Vec<TabStop>,
    #[serde(default)]
    pub drop_cap: DropCap,
    #[serde(default)]
    pub list: ListProps,
    #[serde(default)]
    pub hyphenation: Hyphenation,
    #[serde(default)]
    pub justification: Justification,
    #[serde(default)]
    pub initial_words: InitialWords,
    #[serde(default)]
    pub paragraph_decorations: Vec<ParagraphDecoration>,
    #[serde(default = "default_tab_interval")]
    pub default_tab_interval: f32,
}

fn default_baseline_grid() -> bool {
    true
}

fn deserialize_keep_with_next<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let count = f64::deserialize(deserializer)?;
    if (0.0..=9.0).contains(&count) && count.fract() == 0.0 {
        Ok(count as u32)
    } else {
        Err(serde::de::Error::custom("keep_with_next must be an integer from 0 to 9"))
    }
}

impl Default for ParaStyle {
    fn default() -> Self {
        Self {
            align: Align::Left,
            optical_align: OpticalAlign::default(),
            indent_left: crate::para_style::default_indent_left(),
            indent_right: crate::para_style::default_indent_right(),
            indent_first: crate::para_style::default_indent_first(),
            space_before: crate::para_style::default_space_before(),
            space_after: crate::para_style::default_space_after(),
            space_rule: crate::para_style::default_space_rule(),
            leading: crate::para_style::default_leading(),
            baseline_grid: default_baseline_grid(),
            keep_with_next: 0,
            keep_with_previous: false,
            keep_together: false,
            prevent_orphans: false,
            prevent_widows: false,
            flow_start: FlowStart::default(),
            tabs: Vec::new(),
            drop_cap: DropCap::default(),
            list: ListProps::default(),
            hyphenation: Hyphenation::default(),
            justification: Justification::default(),
            initial_words: InitialWords::default(),
            paragraph_decorations: Vec::new(),
            default_tab_interval: default_tab_interval(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run<T> {
    pub range: TextRange,
    pub style: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub id: StoryId,
    pub text: String,
    #[serde(default)]
    pub chars: Vec<Run<CharStyle>>,
    #[serde(default)]
    pub paras: Vec<Run<ParaStyle>>,
    #[serde(default)]
    pub specials: Vec<(usize, Special)>,
    #[serde(default)]
    pub styles: Vec<Run<StyleRef>>,
    /// The scale from paragraph metrics in points to the document layout coordinates.
    #[serde(skip)]
    pub layout_units_per_point: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum StyleLevel {
    #[default]
    Character,
    Paragraph,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StyleRef {
    pub level: StyleLevel,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overrides: Vec<(String, String)>,
}

impl Default for Story {
    fn default() -> Self {
        Self {
            id: StoryId(0),
            text: String::new(),
            chars: Vec::new(),
            paras: Vec::new(),
            specials: Vec::new(),
            styles: Vec::new(),
            layout_units_per_point: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CharPatchField<T> {
    Same(T),
    Mixed,
}

#[derive(Debug, Clone, Default)]
pub struct CharPatch {
    pub family: Option<String>,
    pub style: Option<String>,
    pub size_pt: Option<f32>,
    pub fill: Option<Option<dtool_core::paint::Paint>>,
    pub tracking: Option<f32>,
    pub leading: Option<Option<f32>>,
    pub baseline_pt: Option<f32>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub h_scale: Option<f32>,
    pub v_scale: Option<f32>,
    pub shear_deg: Option<f32>,
    pub position: Option<Position>,
    pub kerning: Option<Kerning>,
    pub caps: Option<Caps>,
    pub underline: Option<Decoration>,
    pub strike: Option<Decoration>,
    pub outline: Option<Option<(String, f32)>>,
    pub background: Option<Option<dtool_core::paint::Paint>>,
    pub ligatures: Option<Option<bool>>,
    pub opentype: Option<Vec<(String, u32)>>,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CharQuery {
    pub family: CharPatchField<String>,
    pub style: CharPatchField<String>,
    pub size_pt: CharPatchField<f32>,
    pub fill: CharPatchField<Option<dtool_core::paint::Paint>>,
    pub tracking: CharPatchField<f32>,
    pub leading: CharPatchField<Option<f32>>,
    pub baseline_pt: CharPatchField<f32>,
    pub weight: CharPatchField<u16>,
    pub italic: CharPatchField<bool>,
    pub h_scale: CharPatchField<f32>,
    pub v_scale: CharPatchField<f32>,
    pub shear_deg: CharPatchField<f32>,
    pub position: CharPatchField<Position>,
    pub kerning: CharPatchField<Kerning>,
    pub caps: CharPatchField<Caps>,
    pub underline: CharPatchField<Decoration>,
    pub strike: CharPatchField<Decoration>,
    pub outline: CharPatchField<Option<(String, f32)>>,
    pub background: CharPatchField<Option<dtool_core::paint::Paint>>,
    pub ligatures: CharPatchField<Option<bool>>,
    pub opentype: CharPatchField<Vec<(String, u32)>>,
    pub language: CharPatchField<String>,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub range: TextRange,
    pub glyphs: Vec<ShapedGlyph>,
    /// Drawing-only glyphs at the beginning of a list line.
    pub list_marker_len: usize,
    pub(crate) tab_rules: Vec<(usize, TabRule)>,
    pub baseline: f32,
    pub width: f32,
    pub break_after: Option<crate::para_style::BreakKind>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum TabRule {
    Underline,
    Strikeout,
}

impl Line {
    pub fn body_glyphs(&self) -> &[ShapedGlyph] {
        &self.glyphs[self.list_marker_len..]
    }
}

fn scalar_byte(text: &str, at: usize) -> Option<usize> {
    if at == 0 {
        return Some(0);
    }
    let mut n = 0;
    for (i, _) in text.char_indices() {
        if n == at {
            return Some(i);
        }
        n += 1;
    }
    if n == at {
        Some(text.len())
    } else {
        None
    }
}

fn shift_runs<T>(runs: &mut [Run<T>], at: usize, delta: isize) {
    for run in runs {
        if delta > 0 {
            let added = delta as usize;
            if run.range.start >= at {
                run.range.start += added;
                run.range.end += added;
            } else if run.range.end > at {
                run.range.end += added;
            }
        } else {
            let removed = (-delta) as usize;
            let del_end = at + removed;
            if run.range.end <= at {
                continue;
            }
            if run.range.start >= del_end {
                run.range.start -= removed;
                run.range.end -= removed;
            } else {
                if run.range.start >= at {
                    run.range.start = at;
                }
                if run.range.end > del_end {
                    run.range.end -= removed;
                } else {
                    run.range.end = at;
                }
            }
        }
    }
}

fn shift_specials(specials: &mut Vec<(usize, Special)>, at: usize, delta: isize) {
    if delta > 0 {
        let added = delta as usize;
        for (pos, _) in specials.iter_mut() {
            if *pos >= at {
                *pos += added;
            }
        }
    } else {
        let removed = (-delta) as usize;
        let del_end = at + removed;
        specials.retain(|(pos, _)| *pos < at || *pos >= del_end);
        for (pos, _) in specials.iter_mut() {
            if *pos >= del_end {
                *pos -= removed;
            }
        }
    }
}

impl Story {
    pub fn insert_special(&mut self, at: usize, special: Special) {
        if !self.specials.iter().any(|(pos, _)| *pos == at) {
            self.specials.push((at, special));
            self.specials.sort_by_key(|(pos, _)| *pos);
        }
    }

    pub fn insert(&mut self, at: usize, s: &str) {
        let Some(byte) = scalar_byte(&self.text, at) else {
            return;
        };
        let added = s.chars().count();
        self.text.insert_str(byte, s);
        if added == 0 {
            return;
        }
        shift_runs(&mut self.chars, at, added as isize);
        shift_runs(&mut self.paras, at, added as isize);
        shift_specials(&mut self.specials, at, added as isize);
        shift_runs(&mut self.styles, at, added as isize);
    }

    pub fn applied_style(&self, level: StyleLevel, at: usize) -> Option<&StyleRef> {
        self.styles.iter().find(|run| run.style.level == level && run.range.start <= at && at < run.range.end).map(|run| &run.style)
    }

    pub fn applied_styles(&self, level: StyleLevel) -> Vec<&Run<StyleRef>> {
        self.styles.iter().filter(|run| run.style.level == level).collect()
    }

    pub fn clear_applied_style(&mut self, level: StyleLevel, r: TextRange) -> bool {
        if r.start >= r.end {
            return false;
        }
        let before = self.styles.clone();
        let mut out = Vec::new();
        for run in self.styles.drain(..) {
            if run.style.level != level || run.range.end <= r.start || run.range.start >= r.end {
                out.push(run);
                continue;
            }
            if run.range.start < r.start {
                out.push(Run { range: TextRange::new(run.range.start, r.start), style: run.style.clone() });
            }
            if run.range.end > r.end {
                out.push(Run { range: TextRange::new(r.end, run.range.end), style: run.style });
            }
        }
        out.sort_by_key(|run| (run.range.start, run.style.level == StyleLevel::Paragraph));
        self.styles = out;
        self.styles != before
    }

    pub fn set_applied_style(&mut self, r: TextRange, style: StyleRef) -> bool {
        let len = self.text.chars().count();
        if r.start >= r.end || r.end > len {
            return false;
        }
        let before = self.styles.clone();
        self.clear_applied_style(style.level, r);
        self.styles.push(Run { range: r, style });
        self.styles.sort_by_key(|run| (run.range.start, run.style.level == StyleLevel::Paragraph));
        self.styles != before
    }

    pub fn delete(&mut self, r: TextRange) {
        if r.start >= r.end {
            return;
        }
        let Some(start_byte) = scalar_byte(&self.text, r.start) else {
            return;
        };
        let Some(end_byte) = scalar_byte(&self.text, r.end) else {
            return;
        };
        if start_byte >= end_byte || end_byte > self.text.len() {
            return;
        }
        let mut new_text = String::new();
        new_text.push_str(&self.text[..start_byte]);
        new_text.push_str(&self.text[end_byte..]);
        self.text = new_text;
        let removed = (r.end - r.start) as isize;
        shift_runs(&mut self.chars, r.start, -removed);
        shift_runs(&mut self.paras, r.start, -removed);
        shift_specials(&mut self.specials, r.start, -removed);
        shift_runs(&mut self.styles, r.start, -removed);
        self.chars.retain(|run| run.range.start < run.range.end);
        self.paras.retain(|run| run.range.start < run.range.end);
        self.styles.retain(|run| run.range.start < run.range.end);
    }

    pub fn patch_char(&mut self, r: TextRange, patch: &CharPatch) {
        if r.start >= r.end { return; }
        let mut runs = Vec::new();
        for run in &self.chars {
            if run.range.start >= r.end || run.range.end <= r.start {
                runs.push(run.clone());
                continue;
            }
            if run.range.start < r.start {
                runs.push(Run { range: TextRange::new(run.range.start, r.start), style: run.style.clone() });
            }
            let mut style = run.style.clone();
            patch.apply(&mut style);
            runs.push(Run { range: TextRange::new(run.range.start.max(r.start), run.range.end.min(r.end)), style });
            if run.range.end > r.end {
                runs.push(Run { range: TextRange::new(r.end, run.range.end), style: run.style.clone() });
            }
        }
        self.chars = runs;
    }

    pub fn query_char(&self, r: TextRange) -> CharQuery {
        let styles: Vec<&CharStyle> = self.chars.iter().filter(|run| run.range.start < r.end && run.range.end > r.start).map(|run| &run.style).collect();
        CharQuery {
            family: query_field(&styles, |style| style.family.clone()),
            style: query_field(&styles, |style| style.style.clone()),
            size_pt: query_field(&styles, |style| style.size_pt.clone()),
            fill: query_field(&styles, |style| style.fill.clone()),
            tracking: query_field(&styles, |style| style.tracking.clone()),
            leading: query_field(&styles, |style| style.leading.clone()),
            baseline_pt: query_field(&styles, |style| style.baseline_pt.clone()),
            weight: query_field(&styles, |style| style.weight.clone()),
            italic: query_field(&styles, |style| style.italic.clone()),
            h_scale: query_field(&styles, |style| style.h_scale.clone()),
            v_scale: query_field(&styles, |style| style.v_scale.clone()),
            shear_deg: query_field(&styles, |style| style.shear_deg.clone()),
            position: query_field(&styles, |style| style.position.clone()),
            kerning: query_field(&styles, |style| style.kerning.clone()),
            caps: query_field(&styles, |style| style.caps.clone()),
            underline: query_field(&styles, |style| style.underline.clone()),
            strike: query_field(&styles, |style| style.strike.clone()),
            outline: query_field(&styles, |style| style.outline.clone()),
            background: query_field(&styles, |style| style.background.clone()),
            ligatures: query_field(&styles, |style| style.ligatures.clone()),
            opentype: query_field(&styles, |style| style.opentype.clone()),
            language: query_field(&styles, |style| style.language.clone()),
        }
    }

    pub fn select_word(&self, at: usize) -> TextRange {
        word_range(&self.text, at)
    }

    pub fn select_paragraph(&self, at: usize) -> TextRange {
        paragraph_range(&self.text, at)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_paragraph_break(c: char) -> bool {
    c == '\n' || c == '\r' || c == '\u{2029}'
}

pub fn word_range(text: &str, at: usize) -> TextRange {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return TextRange::new(0, 0);
    }
    let mut at = at.min(chars.len());
    if (at == chars.len() || !is_word_char(chars[at])) && at > 0 && is_word_char(chars[at - 1]) {
        at -= 1;
    }
    if at == chars.len() {
        return TextRange::new(at, at);
    }
    if !is_word_char(chars[at]) {
        return TextRange::new(at, at + 1);
    }
    let mut start = at;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    TextRange::new(start, end)
}

pub fn paragraph_range(text: &str, at: usize) -> TextRange {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return TextRange::new(0, 0);
    }
    let mut at = at.min(chars.len());
    if at == chars.len() {
        if at > 0 && !is_paragraph_break(chars[at - 1]) {
            at -= 1;
        } else {
            return TextRange::new(at, at);
        }
    }
    let mut start = at;
    if chars[start] == '\n' && start > 0 && chars[start - 1] == '\r' {
        start -= 1;
    }
    while start > 0 && !is_paragraph_break(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && !is_paragraph_break(chars[end]) {
        end += 1;
    }
    if end < chars.len() {
        if chars[end] == '\r' && end + 1 < chars.len() && chars[end + 1] == '\n' {
            end += 2;
        } else {
            end += 1;
        }
    }
    TextRange::new(start, end)
}
impl CharStyle {
    pub fn fill_str(&self) -> Option<String> {
        self.fill.as_ref().and_then(dtool_core::paint::paint_to_legacy_str)
    }

    pub fn set_fill_str(&mut self, value: Option<&str>) {
        self.fill = value.and_then(dtool_core::paint::paint_from_legacy_str);
    }

    pub fn background_str(&self) -> Option<String> {
        self.background.as_ref().and_then(dtool_core::paint::paint_to_legacy_str)
    }

    pub fn set_background_str(&mut self, value: Option<&str>) {
        self.background = value.and_then(dtool_core::paint::paint_from_legacy_str);
    }
}

#[cfg(test)]
mod typed_fill_contracts {
    use super::CharStyle;
    use dtool_core::paint::{legacy_fill, Colour, Paint};
    use serde::de::value::{Error, StrDeserializer, UnitDeserializer};

    #[test]
    fn char_fill_loader_accepts_old_strings() {
        assert_eq!(legacy_fill::deserialize(StrDeserializer::<Error>::new("red")).unwrap(), Some(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }));
        assert_eq!(legacy_fill::deserialize(StrDeserializer::<Error>::new("#102030")).unwrap(), Some(Paint::Solid { colour: Colour::new(16, 32, 48, 255) }));
        assert_eq!(legacy_fill::deserialize(UnitDeserializer::<Error>::new()).unwrap(), None);
    }

    #[test]
    fn char_style_background_is_typed_with_string_accessor() {
        let mut style = CharStyle::default();
        style.set_background_str(Some("yellow"));
        assert_eq!(style.background, Some(Paint::Solid { colour: Colour::new(255, 255, 0, 255) }));
        assert_eq!(style.background_str().as_deref(), Some("#ffff00"));
        style.set_background_str(None);
        assert_eq!(style.background, None);
    }

    #[test]
    fn char_style_fill_is_typed_with_string_accessor() {
        let mut style = CharStyle::default();
        style.set_fill_str(Some("#0a141e"));
        assert_eq!(style.fill, Some(Paint::Solid { colour: Colour::new(10, 20, 30, 255) }));
        assert_eq!(style.fill_str().as_deref(), Some("#0a141e"));
        style.set_fill_str(None);
        assert_eq!(style.fill, None);
    }
}
