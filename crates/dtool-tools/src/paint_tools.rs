use dtool_core::brush::{BrushSettings, OpParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintTool {
    PaintBrush,
    Erase,
    Pixel,
    BackgroundErase,
    FloodErase,
    FloodFill,
}

impl PaintTool {
    pub fn tool_id(&self) -> &'static str {
        match self {
            PaintTool::PaintBrush => "paint_brush",
            PaintTool::Erase => "erase_brush",
            PaintTool::Pixel => "pixel",
            PaintTool::BackgroundErase => "background_erase_brush",
            PaintTool::FloodErase => "flood_erase",
            PaintTool::FloodFill => "flood_fill",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaintToolOptions {
    pub settings: BrushSettings,
    pub params: OpParams,
}

impl Default for PaintToolOptions {
    fn default() -> Self {
        Self {
            settings: BrushSettings::default(),
            params: OpParams::default(),
        }
    }
}

impl PaintTool {
    pub fn canonical_id(&self) -> &'static str {
        dtool_core::context_toolbar::canonical_tool(self.tool_id()).unwrap_or("tool.paint_brush")
    }
}

impl PaintToolOptions {
    pub fn from_store(store: &dtool_core::context_toolbar::ToolOptionStore, tool: &str) -> Self {
        let mut out = Self::default();
        if let Some(v) = store.number(tool, "brush.width") { out.settings.size_px = v as f32; }
        if let Some(v) = store.number(tool, "brush.opacity") { out.settings.opacity = (v / 100.0) as f32; }
        if let Some(v) = store.number(tool, "brush.flow") { out.settings.flow = (v / 100.0) as f32; }
        if let Some(v) = store.number(tool, "brush.hardness") { out.settings.hardness = (v / 100.0) as f32; }
        if let Some(v) = store.number(tool, "fill.tolerance") { out.params.tolerance = (v / 100.0) as f32; }
        if let Some(v) = store.flag(tool, "wand.contiguous") { out.params.contiguous = v; }
        out
    }

    pub fn write_store(&self, store: &mut dtool_core::context_toolbar::ToolOptionStore, tool: &str) {
        store.set_number(tool, "brush.width", f64::from(self.settings.size_px));
        store.set_number(tool, "brush.opacity", f64::from(self.settings.opacity) * 100.0);
        store.set_number(tool, "brush.flow", f64::from(self.settings.flow) * 100.0);
        store.set_number(tool, "brush.hardness", f64::from(self.settings.hardness) * 100.0);
        store.set_number(tool, "fill.tolerance", f64::from(self.params.tolerance) * 100.0);
    }
}

pub fn size_step(size: f32, up: bool) -> f32 {
    let delta = size * 0.1;
    if up {
        (size + delta).max(1.0)
    } else {
        (size - delta).max(1.0)
    }
}

pub fn opacity_from_digits(digits: &[u8]) -> f32 {
    if digits.is_empty() {
        return 1.0;
    }

    let mut val = 0.0;
    for &d in digits {
        val = val * 10.0 + d as f32;
    }

    (val / 100.0).clamp(0.0, 1.0)
}
