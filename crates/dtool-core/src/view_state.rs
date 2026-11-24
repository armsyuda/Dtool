use serde::{Deserialize, Serialize};

fn default_true() -> bool { true }

pub const ZOOM_MIN: f32 = 0.01;
pub const ZOOM_MAX: f32 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    Normal,
    Outline,
    Pixel,
    Pixel2x,
    Wireframe,
}

impl Default for ViewMode {
    fn default() -> Self { ViewMode::Normal }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasFlags {
    pub force_pixel_alignment: bool,
    pub clip_to_canvas: bool,
    pub preview_mode: bool,
    pub view_mode: ViewMode,
    #[serde(default)]
    pub show_bleed: bool,
    #[serde(default)]
    pub show_margins: bool,
    #[serde(default)]
    pub show_page_edges_in_preview: bool,
    #[serde(default = "default_true")]
    pub show_pixel_selection: bool,
    #[serde(default)]
    pub highlight_missing_fonts: bool,
    #[serde(default)]
    pub highlight_missing_chars: bool,
    #[serde(default)]
    pub show_invisibles: bool,
    #[serde(default)]
    pub isolate_target: Option<crate::object::ObjectId>,
    #[serde(skip)]
    pub mask_view: Option<crate::object::ObjectId>,
}

impl Default for CanvasFlags {
    fn default() -> Self {
        CanvasFlags {
            force_pixel_alignment: false,
            clip_to_canvas: false,
            preview_mode: false,
            show_bleed: false,
            show_margins: false,
            show_page_edges_in_preview: false,
            show_pixel_selection: true,
            highlight_missing_fonts: false,
            highlight_missing_chars: false,
            show_invisibles: false,
            view_mode: ViewMode::Normal,
            isolate_target: None,
            mask_view: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    pub zoom: f32,
    pub scroll: [f32; 2],
    pub canvas_flags: CanvasFlags,
    #[serde(default)]
    pub rotation_deg: f32,
    #[serde(default)]
    pub soft_proof_enabled: bool,
    #[serde(default)]
    pub soft_proof_profile: String,
    #[serde(default)]
    pub soft_proof_intent: String,
    #[serde(default)]
    pub soft_proof_gamut_check: bool,
    #[serde(default = "default_true")]
    pub soft_proof_bpc: bool,
    #[serde(default)]
    pub xray: bool,
    #[serde(default)]
    pub colour_target: ColourTargetState,
}

impl ViewState {
    pub fn isolate_target(&self) -> Option<crate::object::ObjectId> {
        self.canvas_flags.isolate_target
    }

    pub fn set_isolate_target(&mut self, target: Option<crate::object::ObjectId>) {
        self.canvas_flags.isolate_target = target;
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self { zoom: 1.0, scroll: [0.0, 0.0], canvas_flags: CanvasFlags::default(), rotation_deg: 0.0, soft_proof_enabled: false, soft_proof_profile: String::new(), soft_proof_intent: String::new(), soft_proof_gamut_check: false, soft_proof_bpc: true, xray: false, colour_target: ColourTargetState::default() }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourTarget {
    #[default]
    Fill,
    Stroke,
    TextFill,
    TextStroke,
}

impl ColourTarget {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fill" => Some(Self::Fill),
            "stroke" => Some(Self::Stroke),
            "text_fill" => Some(Self::TextFill),
            "text_stroke" => Some(Self::TextStroke),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Stroke => "stroke",
            Self::TextFill => "text_fill",
            Self::TextStroke => "text_stroke",
        }
    }

    pub fn is_text(self) -> bool {
        matches!(self, Self::TextFill | Self::TextStroke)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColourTargetState {
    #[serde(default)]
    pub target: ColourTarget,
    #[serde(default)]
    pub story: Option<u64>,
    #[serde(default)]
    pub range: Option<(usize, usize)>,
}

impl ViewState {
    pub fn highlight(&self) -> (bool, bool) {
        (self.canvas_flags.highlight_missing_fonts, self.canvas_flags.highlight_missing_chars)
    }

    pub fn set_highlight(&mut self, missing_fonts: Option<bool>, missing_chars: Option<bool>) {
        if let Some(value) = missing_fonts {
            self.canvas_flags.highlight_missing_fonts = value;
        }
        if let Some(value) = missing_chars {
            self.canvas_flags.highlight_missing_chars = value;
        }
    }
}
