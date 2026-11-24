use dtool_core::SelectionOp;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SelectionTool {
    Rectangle,
    Ellipse,
    Row,
    Column,
    Freehand,
    FloodSelect,
    SelectionBrush,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionToolOptions {
    pub op: SelectionOp,
    pub feather: f32,
    pub antialias: bool,
    pub from_center: bool,
    pub width: f32,
    pub height: f32,
    pub lasso_mode: dtool_core::LassoMode,
    pub tolerance: f32,
    pub contiguous: bool,
    pub source: dtool_core::SampleSource,
    pub snap_to_edges: bool,
    pub all_layers: bool,
    pub soft_edges: bool,
}

impl Default for SelectionToolOptions {
    fn default() -> Self {
        Self {
            op: SelectionOp::New,
            feather: 0.0,
            antialias: true,
            from_center: false,
            width: 0.0,
            height: 0.0,
            lasso_mode: dtool_core::LassoMode::Freehand,
            tolerance: 32.0,
            contiguous: true,
            source: dtool_core::SampleSource::CurrentLayer,
            snap_to_edges: false,
            all_layers: false,
            soft_edges: false,
        }
    }
}

pub fn modifier_op(shift: bool, alt: bool) -> Option<SelectionOp> {
    match (shift, alt) {
        (true, false) => Some(SelectionOp::Add),
        (false, true) => Some(SelectionOp::Subtract),
        (true, true) => Some(SelectionOp::Intersect),
        _ => None,
    }
}

pub struct EllipseDragState {
    pub start: [f32; 2],
    pub current: [f32; 2],
    pub shift_held: bool,
    pub from_center: bool,
}

impl EllipseDragState {
    pub fn new(start: [f32; 2], from_center: bool) -> Self {
        Self {
            start,
            current: start,
            shift_held: false,
            from_center,
        }
    }

    pub fn update(&mut self, point: [f32; 2], shift: bool) {
        self.current = point;
        self.shift_held = shift;
    }

    pub fn rect(&self) -> [f32; 4] {
        let dx = self.current[0] - self.start[0];
        let dy = self.current[1] - self.start[1];

        if self.from_center {
            let half_w = dx.abs();
            let half_h = dy.abs();
            let w = half_w * 2.0;
            let h = half_h * 2.0;
            return [self.start[0] - half_w, self.start[1] - half_h, w, h];
        }

        if self.shift_held {
            let size = dx.abs().max(dy.abs());
            let sign_x = if dx >= 0.0 { 1.0 } else { -1.0 };
            let sign_y = if dy >= 0.0 { 1.0 } else { -1.0 };
            return [self.start[0], self.start[1], size * sign_x, size * sign_y];
        }

        [self.start[0], self.start[1], dx, dy]
    }
}

#[derive(Debug, Clone)]
pub struct LassoState {
    pub points: Vec<[f32; 2]>,
    pub mode: dtool_core::LassoMode,
    pub op: SelectionOp,
    pub antialias: bool,
}

impl Default for LassoState {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            mode: dtool_core::LassoMode::Freehand,
            op: SelectionOp::New,
            antialias: true,
        }
    }
}

impl LassoState {
    pub fn pointer_down(&mut self, point: [f32; 2]) {
        if self.mode == dtool_core::LassoMode::Freehand {
            self.points.clear();
        }
        self.add_point(point);
    }

    pub fn pointer_move(&mut self, point: [f32; 2]) {
        if self.mode == dtool_core::LassoMode::Freehand && !self.points.is_empty() {
            self.add_point(point);
        }
    }

    pub fn pointer_up(&mut self, point: [f32; 2]) -> Option<dtool_core::Command> {
        if self.mode == dtool_core::LassoMode::Freehand {
            self.add_point(point);
            self.finish()
        } else {
            None
        }
    }

    pub fn double_click(&mut self, point: [f32; 2]) -> Option<dtool_core::Command> {
        self.add_point(point);
        self.finish()
    }

    pub fn enter(&mut self) -> Option<dtool_core::Command> {
        self.finish()
    }

    pub fn cancel(&mut self) {
        self.points.clear();
    }

    fn add_point(&mut self, point: [f32; 2]) {
        if point.iter().all(|value| value.is_finite()) && self.points.last() != Some(&point) {
            self.points.push(point);
        }
    }

    pub fn finish(&mut self) -> Option<dtool_core::Command> {
        if self.points.len() < 3 {
            return None;
        }
        let points = std::mem::take(&mut self.points);
        let mut command = dtool_core::Command::new("selection.lasso");
        let params = &mut command.params;
        params["points"] = points.iter().map(|point| point.to_vec()).collect::<Vec<Vec<f32>>>().into();
        params["mode"] = lasso_mode_key(self.mode).into();
        params["op"] = selection_op_key(self.op).into();
        params["antialias"] = self.antialias.into();
        Some(command)
    }
}

pub struct FloodSelectState {
    pub options: SelectionToolOptions,
}

impl Default for FloodSelectState {
    fn default() -> Self {
        Self {
            options: SelectionToolOptions::default(),
        }
    }
}

impl FloodSelectState {
    pub fn click(&self, x: u32, y: u32) -> dtool_core::Command {
        let scope = if self.options.all_layers { "all_layers" } else { sample_source_key(self.options.source) };
        let mut command = dtool_core::Command::new("selection.magicWand");
        let params = &mut command.params;
        params["at"]["x"] = x.into();
        params["at"]["y"] = y.into();
        params["contiguous"] = self.options.contiguous.into();
        params["antialias"] = self.options.antialias.into();
        params["tolerance"] = self.options.tolerance.into();
        params["sampling_scope"] = scope.into();
        params["op"] = selection_op_key(self.options.op).into();
        command
    }
}

#[derive(Debug, Clone)]
pub struct SelectionBrushState {
    pub points: Vec<[f32; 3]>,
    pub options: SelectionToolOptions,
    pub size_px: f32,
    pub hardness: f32,
}

impl Default for SelectionBrushState {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            options: SelectionToolOptions::default(),
            size_px: 10.0,
            hardness: 1.0,
        }
    }
}

impl SelectionBrushState {
    pub fn pointer_down(&mut self, x: f32, y: f32, pressure: f32) {
        self.points.clear();
        self.add_point(x, y, pressure);
    }

    pub fn pointer_move(&mut self, x: f32, y: f32, pressure: f32) {
        if !self.points.is_empty() {
            self.add_point(x, y, pressure);
        }
    }

    pub fn pointer_up(&mut self) -> Option<dtool_core::Command> {
        if self.points.is_empty() {
            return None;
        }
        let points = std::mem::take(&mut self.points);
        let mut command = dtool_core::Command::new("selection.brush");
        let params = &mut command.params;
        params["points"] = points.iter().map(|point| point.to_vec()).collect::<Vec<Vec<f32>>>().into();
        params["size_px"] = self.size_px.into();
        params["hardness"] = self.hardness.into();
        params["op"] = selection_op_key(self.options.op).into();
        params["snap_to_edges"] = self.options.snap_to_edges.into();
        params["all_layers"] = self.options.all_layers.into();
        Some(command)
    }

    fn add_point(&mut self, x: f32, y: f32, pressure: f32) {
        let point = [x, y, pressure];
        if x.is_finite() && y.is_finite() && self.points.last() != Some(&point) {
            self.points.push(point);
        }
    }
}

pub struct RowMarqueeState {
    pub options: SelectionToolOptions,
}

impl Default for RowMarqueeState {
    fn default() -> Self {
        Self {
            options: SelectionToolOptions::default(),
        }
    }
}

impl RowMarqueeState {
    pub fn click(&self, doc_width: u32, y_px: f32) -> dtool_core::Command {
        let h = if self.options.height > 0.0 { self.options.height } else { 1.0 };
        let y = y_px.floor();

        let mut command = dtool_core::Command::new("selection.rect");
        let params = &mut command.params;
        params["rect"]["x"] = 0_i64.into();
        params["rect"]["y"] = y.into();
        params["rect"]["w"] = doc_width.into();
        params["rect"]["h"] = h.into();
        params["op"] = selection_op_key(self.options.op).into();
        params["antialias"] = self.options.antialias.into();
        command
    }
}

pub struct ColumnMarqueeState {
    pub options: SelectionToolOptions,
}

impl Default for ColumnMarqueeState {
    fn default() -> Self {
        Self {
            options: SelectionToolOptions::default(),
        }
    }
}

impl ColumnMarqueeState {
    pub fn click(&self, doc_height: u32, x_px: f32) -> dtool_core::Command {
        let w = if self.options.width > 0.0 { self.options.width } else { 1.0 };
        let x = x_px.floor();

        let mut command = dtool_core::Command::new("selection.rect");
        let params = &mut command.params;
        params["rect"]["x"] = x.into();
        params["rect"]["y"] = 0_i64.into();
        params["rect"]["w"] = w.into();
        params["rect"]["h"] = doc_height.into();
        params["op"] = selection_op_key(self.options.op).into();
        params["antialias"] = self.options.antialias.into();
        command
    }
}

pub fn selection_op_key(op: SelectionOp) -> &'static str {
    match op {
        SelectionOp::New => "new",
        SelectionOp::Add => "add",
        SelectionOp::Subtract => "subtract",
        SelectionOp::Intersect => "intersect",
    }
}

pub fn lasso_mode_key(mode: dtool_core::LassoMode) -> &'static str {
    match mode {
        dtool_core::LassoMode::Freehand => "freehand",
        dtool_core::LassoMode::Polygonal => "polygonal",
        dtool_core::LassoMode::Magnetic => "magnetic",
    }
}

pub fn sample_source_key(source: dtool_core::SampleSource) -> &'static str {
    match source {
        dtool_core::SampleSource::CurrentLayer => "current_layer",
        dtool_core::SampleSource::AllLayers => "all_layers",
    }
}

impl SelectionTool {
    pub fn tool_id(&self) -> &'static str {
        match self {
            SelectionTool::Rectangle => "tool.marquee.rectangle",
            SelectionTool::Ellipse => "tool.marquee.ellipse",
            SelectionTool::Row => "tool.marquee.row",
            SelectionTool::Column => "tool.marquee.column",
            SelectionTool::Freehand => "tool.marquee.freehand",
            SelectionTool::FloodSelect => "tool.magic_wand",
            SelectionTool::SelectionBrush => "tool.selection_brush",
        }
    }
}

fn op_name(op: SelectionOp) -> &'static str {
    match op {
        SelectionOp::New => "new",
        SelectionOp::Add => "add",
        SelectionOp::Subtract => "subtract",
        SelectionOp::Intersect => "intersect",
    }
}

fn op_from(name: &str) -> Option<SelectionOp> {
    match name {
        "new" => Some(SelectionOp::New),
        "add" => Some(SelectionOp::Add),
        "subtract" => Some(SelectionOp::Subtract),
        "intersect" => Some(SelectionOp::Intersect),
        _ => None,
    }
}

impl SelectionToolOptions {
    pub fn from_store(store: &dtool_core::context_toolbar::ToolOptionStore, tool: &str) -> Self {
        let mut out = Self::default();
        if let Some(op) = store.choice(tool, "marquee.op").as_deref().and_then(op_from) { out.op = op; }
        if let Some(v) = store.number(tool, "marquee.feather") { out.feather = v as f32; }
        if let Some(v) = store.flag(tool, "marquee.antialias") { out.antialias = v; }
        if let Some(v) = store.flag(tool, "marquee.from_center") { out.from_center = v; }
        if let Some(v) = store.number(tool, "marquee.width") { out.width = v as f32; }
        if let Some(v) = store.number(tool, "marquee.height") { out.height = v as f32; }
        if let Some(mode) = store.choice(tool, "lasso.mode") {
            out.lasso_mode = match mode.as_str() {
                "polygonal" => dtool_core::LassoMode::Polygonal,
                "magnetic" => dtool_core::LassoMode::Magnetic,
                _ => dtool_core::LassoMode::Freehand,
            };
        }
        if let Some(v) = store.number(tool, "wand.tolerance") { out.tolerance = v as f32; }
        if let Some(v) = store.flag(tool, "wand.contiguous") { out.contiguous = v; }
        if let Some(source) = store.choice(tool, "wand.source") {
            out.source = if source == "all_layers" { dtool_core::SampleSource::AllLayers } else { dtool_core::SampleSource::CurrentLayer };
        }
        if let Some(v) = store.flag(tool, "selection_brush.all_layers") { out.all_layers = v; }
        if let Some(v) = store.flag(tool, "selection_brush.snap_to_edges") { out.snap_to_edges = v; }
        if let Some(v) = store.flag(tool, "selection_brush.soft_edges") { out.soft_edges = v; }
        out
    }

    pub fn write_store(&self, store: &mut dtool_core::context_toolbar::ToolOptionStore, tool: &str) {
        store.set_choice(tool, "marquee.op", op_name(self.op));
        store.set_number(tool, "marquee.feather", f64::from(self.feather));
        store.set_flag(tool, "marquee.antialias", self.antialias);
        store.set_flag(tool, "marquee.from_center", self.from_center);
        store.set_number(tool, "marquee.width", f64::from(self.width));
        store.set_number(tool, "marquee.height", f64::from(self.height));
        let mode = match self.lasso_mode {
            dtool_core::LassoMode::Freehand => "freehand",
            dtool_core::LassoMode::Polygonal => "polygonal",
            dtool_core::LassoMode::Magnetic => "magnetic",
        };
        store.set_choice(tool, "lasso.mode", mode);
        store.set_number(tool, "wand.tolerance", f64::from(self.tolerance));
        store.set_flag(tool, "wand.contiguous", self.contiguous);
        let source = match self.source {
            dtool_core::SampleSource::AllLayers => "all_layers",
            dtool_core::SampleSource::CurrentLayer => "current_layer",
        };
        store.set_choice(tool, "wand.source", source);
        store.set_flag(tool, "selection_brush.all_layers", self.all_layers);
        store.set_flag(tool, "selection_brush.snap_to_edges", self.snap_to_edges);
        store.set_flag(tool, "selection_brush.soft_edges", self.soft_edges);
    }
}

#[cfg(test)]
mod tool_option_store_tests {
    use super::*;

    #[test]
    fn selection_options_round_trip_through_store() {
        let mut store = dtool_core::context_toolbar::ToolOptionStore::default();
        let mut options = SelectionToolOptions::default();
        options.op = SelectionOp::Subtract;
        options.feather = 4.0;
        options.from_center = true;
        options.write_store(&mut store, SelectionTool::Rectangle.tool_id());
        let back = SelectionToolOptions::from_store(&store, SelectionTool::Rectangle.tool_id());
        assert_eq!(back.op, SelectionOp::Subtract);
        assert_eq!(back.feather, 4.0);
        assert!(back.from_center);
        let wand = SelectionToolOptions::from_store(&store, SelectionTool::FloodSelect.tool_id());
        assert_eq!(wand.op, SelectionOp::New);
    }
}
