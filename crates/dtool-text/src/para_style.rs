use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TabAlign {
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Leader {
    None,
    Character(char),
    Underline,
    Strikeout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabStop {
    pub pos: f32,
    #[serde(default = "default_tab_align")]
    pub align: TabAlign,
    #[serde(default = "default_decimal_char")]
    pub decimal: char,
    #[serde(default = "default_leader")]
    pub leader: Leader,
    #[serde(default)]
    pub from_right: bool,
}

fn default_tab_align() -> TabAlign {
    TabAlign::Left
}

fn default_decimal_char() -> char {
    '.'
}

fn default_leader() -> Leader {
    Leader::None
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SpaceBeforeRule {
    Always,
    OnlyBetweenParagraphs,
    OnlyAtColumnTop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hyphenation {
    #[serde(default)]
    pub on: bool,
    #[serde(default = "default_min_word")]
    pub min_word: u32,
    #[serde(default = "default_min_prefix")]
    pub min_prefix: u32,
    #[serde(default = "default_min_suffix")]
    pub min_suffix: u32,
    #[serde(default = "default_max_consecutive")]
    pub max_consecutive: u32,
    #[serde(default = "default_zone")]
    pub zone: f32,
}

fn default_min_word() -> u32 { 4 }
fn default_min_prefix() -> u32 { 3 }
fn default_min_suffix() -> u32 { 3 }
fn default_max_consecutive() -> u32 { 3 }
fn default_zone() -> f32 { 0.5 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParagraphDecoration {
    pub sides: [bool; 4],
    pub stroke: dtool_core::paint::Colour,
    pub fill: Option<dtool_core::paint::Colour>,
    pub width: f32,
    pub padding: f32,
}

impl Default for ParagraphDecoration {
    fn default() -> Self {
        Self {
            sides: [false, false, false, true],
            stroke: dtool_core::paint::Colour::new(0, 0, 0, 255),
            fill: None,
            width: 1.0,
            padding: 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Justification {
    pub enabled: bool,
    pub word_min: f32,
    pub word_ideal: f32,
    pub word_max: f32,
    pub letter_min: f32,
    pub letter_ideal: f32,
    pub letter_max: f32,
}

impl Default for Justification {
    fn default() -> Self {
        Self {
            enabled: false,
            word_min: 80.0,
            word_ideal: 100.0,
            word_max: 133.0,
            letter_min: 0.0,
            letter_ideal: 0.0,
            letter_max: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DropCap {
    #[serde(default)]
    pub on: bool,
    #[serde(default = "default_drop_cap_lines")]
    pub lines: u32,
    #[serde(default = "default_drop_cap_chars")]
    pub chars: u32,
    #[serde(default = "default_drop_cap_distance")]
    pub distance: f32,
    #[serde(default)]
    pub style: Option<String>,
}

fn default_drop_cap_lines() -> u32 { 1 }
fn default_drop_cap_chars() -> u32 { 1 }
fn default_drop_cap_distance() -> f32 { 0.0 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InitialWords {
    #[serde(default)]
    pub on: bool,
    #[serde(default = "default_initial_words_max")]
    pub max_words: u32,
    #[serde(default = "default_initial_words_end_chars")]
    pub end_chars: String,
    #[serde(default)]
    pub style: Option<String>,
}

fn default_initial_words_max() -> u32 { 1 }
fn default_initial_words_end_chars() -> String { ".".to_string() }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NumberFormat {
    Arabic,
    Arabic2,
    Arabic3,
    Arabic4,
    AlphaLower,
    AlphaUpper,
    RomanLower,
    RomanUpper,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ListKind {
    None,
    Bullet(String),
    Number(NumberFormat),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListProps {
    #[serde(default = "default_list_kind")]
    pub kind: ListKind,
    #[serde(default = "default_list_level")]
    pub level: u32,
    #[serde(default)]
    pub start_at: Option<u32>,
    #[serde(default)]
    pub restart: bool,
    #[serde(default = "default_list_tab")]
    pub tab: f32,
}

fn default_list_kind() -> ListKind {
    ListKind::None
}

fn default_list_level() -> u32 { 1 }
fn default_list_tab() -> f32 { 0.0 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowStart {
    #[default]
    Anywhere,
    NextColumn,
    NextFrame,
    NextPage,
    NextOddPage,
    NextEvenPage,
}

pub fn default_indent_left() -> f32 { 0.0 }
pub fn default_indent_right() -> f32 { 0.0 }
pub fn default_indent_first() -> f32 { 0.0 }
pub fn default_space_before() -> f32 { 0.0 }
pub fn default_space_after() -> f32 { 0.0 }
pub fn default_space_rule() -> SpaceBeforeRule {
    SpaceBeforeRule::Always
}
pub fn default_leading() -> f32 { 1.2 }

impl Default for Hyphenation {
    fn default() -> Self {
        Self {
            on: false,
            min_word: default_min_word(),
            min_prefix: default_min_prefix(),
            min_suffix: default_min_suffix(),
            max_consecutive: default_max_consecutive(),
            zone: default_zone(),
        }
    }
}

impl Default for DropCap {
    fn default() -> Self {
        Self {
            on: false,
            lines: default_drop_cap_lines(),
            chars: default_drop_cap_chars(),
            distance: default_drop_cap_distance(),
            style: None,
        }
    }
}

impl Default for InitialWords {
    fn default() -> Self {
        Self {
            on: false,
            max_words: default_initial_words_max(),
            end_chars: default_initial_words_end_chars(),
            style: None,
        }
    }
}

impl Default for ListProps {
    fn default() -> Self {
        Self {
            kind: default_list_kind(),
            level: default_list_level(),
            start_at: None,
            restart: false,
            tab: default_list_tab(),
        }
    }
}

impl ListProps {
    pub fn tab_step(&self) -> f32 {
        if self.tab.is_finite() && self.tab > 0.0 { self.tab } else { 18.0 }
    }
}

pub fn validate_tabs(tabs: &[TabStop]) -> Result<(), String> {
    let mut prev: Option<f32> = None;
    for tab in tabs {
        if !tab.pos.is_finite() || tab.pos < 0.0 {
            return Err("tab position must be finite and non-negative".to_string());
        }
        if let Some(p) = prev {
            if tab.pos < p {
                return Err("tab positions must be sorted".to_string());
            }
        }
        prev = Some(tab.pos);
        if let Leader::Character(c) = &tab.leader {
            if *c == '\t' || *c == '\n' || *c == '\r' {
                return Err("leader character must not be a control character".to_string());
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParaStylePatch {
    #[serde(default)]
    pub indent_left: Option<f32>,
    #[serde(default)]
    pub indent_right: Option<f32>,
    #[serde(default)]
    pub indent_first: Option<f32>,
    #[serde(default)]
    pub space_before: Option<f32>,
    #[serde(default)]
    pub space_after: Option<f32>,
    #[serde(default)]
    pub space_rule: Option<SpaceBeforeRule>,
    #[serde(default)]
    pub leading: Option<f32>,
    #[serde(default)]
    pub tabs: Option<Vec<TabStop>>,
    #[serde(default)]
    pub hyphenation: Option<Hyphenation>,
    #[serde(default)]
    pub drop_cap: Option<DropCap>,
    #[serde(default)]
    pub initial_words: Option<InitialWords>,
    #[serde(default)]
    pub list: Option<ListProps>,
    #[serde(default)]
    pub keep_with_next: Option<u32>,
    #[serde(default)]
    pub keep_with_previous: Option<bool>,
    #[serde(default)]
    pub keep_together: Option<bool>,
    #[serde(default)]
    pub prevent_widows: Option<bool>,
    #[serde(default)]
    pub prevent_orphans: Option<bool>,
    #[serde(default)]
    pub flow_start: Option<FlowStart>,
    #[serde(default)]
    pub baseline_grid: Option<bool>,
    #[serde(default)]
    pub decorations: Option<Vec<String>>,
}

impl ParaStylePatch {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(v) = self.space_before {
            if v < 0.0 {
                return Err("space_before must not be negative".to_string());
            }
        }
        if let Some(v) = self.space_after {
            if v < 0.0 {
                return Err("space_after must not be negative".to_string());
            }
        }
        if let Some(v) = self.leading {
            if v <= 0.0 {
                return Err("leading must be positive".to_string());
            }
        }
        if let Some(v) = self.keep_with_next {
            if v > 9 {
                return Err("keep_with_next must be between 0 and 9".to_string());
            }
        }
        if let Some(tabs) = &self.tabs {
            validate_tabs(tabs)?;
        }
        if let Some(list) = &self.list {
            if list.level == 0 || list.level > 9 {
                return Err("list level must be between 1 and 9".to_string());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakKind {
    Line,
    Column,
    Frame,
    Page,
    OddPage,
    EvenPage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Special {
    RightIndentTab,
    IndentToHere,
    Break(BreakKind),
}
