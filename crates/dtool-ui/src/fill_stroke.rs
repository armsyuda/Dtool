use dtool_core::paint::{project_display_paint, DiffusionCurveCreationMode, DiffusionPaint, MeshPaint, Paint, PaintRole, StrokeStyle};
use dtool_design;
use dtool_strings::text;
use egui;

pub const PAINT_SWAP: &str = "paint.swap";
pub const PAINT_NONE: &str = "paint.none";
pub const PAINT_DEFAULT: &str = "paint.default";

pub struct PaintOwners<'a> {
    pub source: Option<&'a mut Paint>,
    pub frame: Option<&'a mut Paint>,
    pub cell: Option<&'a mut Paint>,
    pub edge: Option<&'a mut StrokeStyle>,
    pub glyph: Option<&'a mut Paint>,
}

impl PaintOwners<'_> {
    pub fn display(&self, role: PaintRole) -> Paint {
        project_display_paint(role, self.source.as_deref(), self.frame.as_deref(), self.cell.as_deref(), self.edge.as_deref(), self.glyph.as_deref())
    }

    pub fn paint_mut(&mut self, role: PaintRole) -> Option<&mut Paint> {
        match role {
            PaintRole::SourceObject => self.source.as_deref_mut(),
            PaintRole::PictureFrame => self.frame.as_deref_mut(),
            PaintRole::Cell => self.cell.as_deref_mut(),
            PaintRole::Edge => self.edge.as_deref_mut().map(|edge| &mut edge.paint),
            PaintRole::Glyph => self.glyph.as_deref_mut(),
        }
    }

    pub fn snapshot(&mut self, role: PaintRole) -> Option<PaintSnapshot<'_>> {
        let paint = self.paint_mut(role)?;
        let original = paint.clone();
        Some(PaintSnapshot { role, paint, original, settled: false })
    }
}

pub struct PaintSnapshot<'a> {
    pub role: PaintRole,
    paint: &'a mut Paint,
    original: Paint,
    settled: bool,
}

impl PaintSnapshot<'_> {
    pub fn current(&self) -> &Paint { self.paint }
    pub fn set(&mut self, paint: Paint) { *self.paint = paint; }
    pub fn commit(mut self) -> bool {
        self.settled = true;
        *self.paint != self.original
    }
    pub fn cancel(mut self) {
        *self.paint = self.original.clone();
        self.settled = true;
    }
}

impl Drop for PaintSnapshot<'_> {
    fn drop(&mut self) { if !self.settled { *self.paint = self.original.clone(); } }
}

#[derive(Clone, PartialEq)]
pub enum WellState {
    Single(Paint),
    Mixed,
    None,
}

pub const PLANNED_FILL_KINDS: [&str; 2] = ["fill.kind.mesh", "fill.kind.diffusion"];

pub const FILL_MENU_KINDS: [&str; 3] = ["fill.kind.solid", "fill.kind.gradient", "fill.kind.hatch"];

pub fn default_fill_for_kind(key: &str) -> Option<Paint> {
    match key {
        "fill.kind.solid" => Some(solid_paint(0, 0, 0, 255)),
        "fill.kind.gradient" => Some(Paint::Gradient {
            kind: dtool_core::paint::GradientKind::Linear,
            stops: vec![
                dtool_core::paint::GradientStop { offset: 0.0, colour: dtool_core::paint::Colour::new(255, 255, 255, 255) },
                dtool_core::paint::GradientStop { offset: 1.0, colour: dtool_core::paint::Colour::new(0, 0, 0, 255) },
            ],
            geom: dtool_core::paint::GradientGeom::default(),
        }),
        "fill.kind.hatch" => Some(Paint::Hatch(dtool_core::paint::Hatch {
            lines: vec![dtool_core::paint::HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 4.0, dash: Vec::new() }],
            colour: dtool_core::paint::Colour::new(0, 0, 0, 255),
            background: None,
            weight: 1.0,
            units: dtool_core::paint::HatchUnits::Document,
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        })),
        _ => None,
    }
}

pub fn fill_kind_planned(key: &str) -> bool {
    PLANNED_FILL_KINDS.contains(&key)
}

pub fn fill_kind_label(key: &str) -> String {
    if fill_kind_planned(key) {
        format!("{} ({})", text(key), text("menu.planned.suffix"))
    } else {
        text(key).to_string()
    }
}

pub struct FillStrokeWells {
    pub fill_state: WellState,
    pub stroke_state: WellState,
    pub active_popup: Option<PaintRole>,
    pub popup_original: Option<Paint>,
    pub display: dtool_core::paint::DisplayColourCtx,
    pub display_icc: Option<dtool_core::colour::model::ObjectIcc>,
}

impl Default for FillStrokeWells {
    fn default() -> Self {
        Self {
            fill_state: WellState::None,
            stroke_state: WellState::None,
            active_popup: None,
            popup_original: None,
            display: dtool_core::paint::DisplayColourCtx::app_default(),
            display_icc: None,
        }
    }
}

impl FillStrokeWells {
    pub fn new(owners: &PaintOwners<'_>) -> Self {
        let fill_paint = owners.display(PaintRole::SourceObject);
        let stroke_paint = owners.edge.as_ref().map(|s| s.paint.clone()).unwrap_or(Paint::None);
        
        Self {
            fill_state: WellState::Single(fill_paint),
            stroke_state: WellState::Single(stroke_paint),
            active_popup: None,
            popup_original: None,
            display: dtool_core::paint::DisplayColourCtx::app_default(),
            display_icc: None,
        }
    }

    pub fn new_from_selection(owners: &[PaintOwners<'_>]) -> Self {
        Self {
            fill_state: shared_state(owners.iter().filter(|o| o.source.is_some()).map(|o| o.display(PaintRole::SourceObject))),
            stroke_state: shared_state(owners.iter().filter_map(|o| o.edge.as_ref().map(|s| s.paint.clone()))),
            active_popup: None,
            popup_original: None,
            display: dtool_core::paint::DisplayColourCtx::app_default(),
            display_icc: None,
        }
    }

    pub fn set_display(&mut self, document: Option<&dtool_core::document::Document>, icc: Option<dtool_core::colour::model::ObjectIcc>) {
        self.display = dtool_core::paint::DisplayColourCtx::for_optional(document);
        self.display_icc = icc;
    }

    pub fn show_context_bar(&mut self, ui: &mut egui::Ui, owners: &PaintOwners<'_>) -> Option<&'static str> {
        let mut action = None;
        if dtool_design::button(ui, text("fill.well")).clicked() {
            self.open_popup_for(owners, PaintRole::SourceObject);
        }

        if dtool_design::button(ui, text("stroke.well")).clicked() {
            self.open_popup_for(owners, PaintRole::Edge);
        }


        if dtool_design::icon_button(ui, text("paint.swap")).clicked() {
            self.swap_paints();
            action = Some(PAINT_SWAP);
        }

        if dtool_design::icon_button(ui, text("paint.none")).clicked() {
            self.set_none();
            action = Some(PAINT_NONE);
        }

        if dtool_design::icon_button(ui, text("paint.default")).clicked() {
            self.set_default();
            action = Some(PAINT_DEFAULT);
        }
        action
    }

    pub fn bar_command(&self, action: &str, ids: &[dtool_core::object::ObjectId]) -> Option<dtool_protocol::Command> {
        if ids.is_empty() {
            return None;
        }
        let ids: Vec<u64> = ids.iter().map(|id| id.0).collect();
        match action {
            PAINT_SWAP | PAINT_DEFAULT => Some(dtool_protocol::Command::with_params(action, serde_json::json!({ "ids": ids }))),
            PAINT_NONE => {
                let roles: &[&str] = match self.active_popup {
                    Some(PaintRole::SourceObject) => &["fill"],
                    Some(PaintRole::Edge) => &["stroke"],
                    Some(_) => return None,
                    None => &["fill", "stroke"],
                };
                let mut params: Vec<serde_json::Value> = roles.iter().map(|role| serde_json::json!({ "ids": ids, "role": role })).collect();
                if params.len() == 1 {
                    Some(dtool_protocol::Command::with_params(PAINT_NONE, params.remove(0)))
                } else {
                    let commands: Vec<serde_json::Value> = params.into_iter().map(|params| serde_json::json!({ "name": PAINT_NONE, "params": params })).collect();
                    Some(dtool_protocol::Command::with_params("edit.batch", serde_json::json!({ "commands": commands })))
                }
            }
            _ => None,
        }
    }

    pub fn show_popup(&mut self, ui: &mut egui::Ui) {
        if let Some(role) = self.active_popup {
            dtool_design::dialog_frame(ui, |ui| {
                match role {
                    PaintRole::Edge => self.show_stroke_editor(ui),
                    _ => self.show_fill_editor(ui),
                }
            });
        }
    }

    fn show_fill_editor(&mut self, ui: &mut egui::Ui) {
        match self.fill_state.clone() {
            WellState::Mixed => {
                mixed_well(ui, "fill.well", text("fill.mixed"));
            }
            WellState::Single(paint) => {
                match &paint {
                    Paint::Solid { .. } | Paint::Gradient { .. } => {
                        edit_well_colours(ui, &mut self.fill_state, &self.display, self.display_icc.as_ref());
                    }
                    Paint::Mesh(mesh) => {
                        ui.add_enabled_ui(false, |ui| self.show_mesh_controls(ui, mesh));
                    }
                    Paint::Diffusion(diffusion) => {
                        ui.add_enabled_ui(false, |ui| self.show_diffusion_controls(ui, diffusion));
                    }
                    Paint::Hatch(hatch) => {
                        self.show_hatch_controls(ui, hatch);
                    }
                    _ => {
                        ui.label(text("paint.type.other"));
                    }
                }
            }
            WellState::None => {
                ui.label(text("paint.none"));
            }
        }

        dtool_design::panel_header(ui, text("fill.kind.header"));

        show_paint_kinds(ui, &mut self.fill_state, &FILL_MENU_KINDS);
    }

    pub fn edit_hatch(&mut self, hatch: dtool_core::paint::Hatch) -> bool {
        let next = WellState::Single(Paint::Hatch(hatch));
        if matches!(&self.fill_state, WellState::Single(Paint::Hatch(_))) && self.fill_state != next {
            self.fill_state = next;
            return true;
        }
        false
    }

    fn show_hatch_controls(&mut self, ui: &mut egui::Ui, hatch: &dtool_core::paint::Hatch) {
        let mut state = crate::hatch_editor::HatchEditorState { hatch: Some(hatch.clone()), original: Some(hatch.clone()) };
        if crate::hatch_editor::show(ui, &mut state) {
            if let Some(next) = state.hatch {
                self.edit_hatch(next);
            }
        }
    }

    fn show_mesh_controls(&mut self, ui: &mut egui::Ui, mesh: &MeshPaint) {
        dtool_design::panel_header(ui, text("fill.kind.mesh"));
        ui.label(format!("{} {}", text("fill.mesh.vertices"), mesh.vertex_count()));
        ui.label(format!("{} {}", text("fill.mesh.triangles"), mesh.triangle_count()));

        if dtool_design::button(ui, text("fill.mesh.add_vertex")).clicked() {
            if let WellState::Single(paint) = &mut self.fill_state {
                if let Paint::Mesh(m) = paint {
                    m.add_vertex([0.0, 0.0], dtool_core::paint::Colour::new(255, 255, 255, 255));
                }
            }
        }

        if dtool_design::button(ui, text("fill.mesh.clear")).clicked() {
            self.fill_state = WellState::Single(Paint::Mesh(MeshPaint::new()));
        }
    }

    fn show_diffusion_controls(&mut self, ui: &mut egui::Ui, diffusion: &DiffusionPaint) {
        dtool_design::panel_header(ui, text("fill.kind.diffusion"));

        let mode_label = match diffusion.mode {
            DiffusionCurveCreationMode::Manual => text("fill.diffusion.mode.manual"),
            DiffusionCurveCreationMode::Automatic => text("fill.diffusion.mode.automatic"),
        };
        ui.label(mode_label);

        let mut manual = matches!(diffusion.mode, DiffusionCurveCreationMode::Manual);
        if dtool_design::toggle(ui, &mut manual, text("fill.diffusion.mode.manual")).changed() {
            if let WellState::Single(paint) = &mut self.fill_state {
                if let Paint::Diffusion(d) = paint {
                    d.mode = if manual {
                        DiffusionCurveCreationMode::Manual
                    } else {
                        DiffusionCurveCreationMode::Automatic
                    };
                }
            }
        }

        let mut automatic = matches!(diffusion.mode, DiffusionCurveCreationMode::Automatic);
        if dtool_design::toggle(ui, &mut automatic, text("fill.diffusion.mode.automatic")).changed() {
            if let WellState::Single(paint) = &mut self.fill_state {
                if let Paint::Diffusion(d) = paint {
                    d.mode = if automatic {
                        DiffusionCurveCreationMode::Automatic
                    } else {
                        DiffusionCurveCreationMode::Manual
                    };
                }
            }
        }

        if dtool_design::button(ui, text("fill.diffusion.reseed")).clicked() {
            if let WellState::Single(paint) = &mut self.fill_state {
                if let Paint::Diffusion(d) = paint {
                    d.seed = Some(rand_seed());
                }
            }
        }
    }

    fn show_stroke_editor(&mut self, ui: &mut egui::Ui) {
        match &self.stroke_state {
            WellState::Mixed => {
                mixed_well(ui, "stroke.well", text("stroke.mixed"));
            }
            WellState::Single(paint) => {
                if matches!(paint, Paint::Solid { .. } | Paint::Gradient { .. }) {
                    edit_well_colours(ui, &mut self.stroke_state, &self.display, self.display_icc.as_ref());
                } else {
                    ui.label(text("paint.type.other"));
                }
            }
            WellState::None => {
                ui.label(text("paint.none"));
            }
        }
        dtool_design::panel_header(ui, text("fill.kind.header"));
        show_paint_kinds(ui, &mut self.stroke_state, &FILL_MENU_KINDS[..2]);
    }

    pub fn close_popup(&mut self) {
        self.active_popup = None;
    }

    pub fn role_state(&self, role: PaintRole) -> &WellState {
        match role {
            PaintRole::Edge => &self.stroke_state,
            _ => &self.fill_state,
        }
    }

    fn set_role_state(&mut self, role: PaintRole, state: WellState) {
        match role {
            PaintRole::Edge => self.stroke_state = state,
            _ => self.fill_state = state,
        }
    }

    pub fn read_owner(&mut self, owners: &PaintOwners<'_>, role: PaintRole) {
        let state = match role {
            PaintRole::Edge if owners.edge.is_none() => WellState::None,
            PaintRole::SourceObject if owners.source.is_none() => WellState::None,
            PaintRole::PictureFrame if owners.frame.is_none() => WellState::None,
            PaintRole::Cell if owners.cell.is_none() => WellState::None,
            PaintRole::Glyph if owners.glyph.is_none() => WellState::None,
            _ => WellState::Single(owners.display(role)),
        };
        self.set_role_state(role, state);
    }

    /// Opens the popup for `role`. A Mixed well (multi-selection with differing paints) stays
    /// Mixed: no paint is preselected, and OK without picking one commits nothing.
    pub fn open_popup_for(&mut self, owners: &PaintOwners<'_>, role: PaintRole) {
        if *self.role_state(role) == WellState::Mixed {
            self.popup_original = None;
        } else {
            self.read_owner(owners, role);
            self.popup_original = Some(owners.display(role));
        }
        self.active_popup = Some(role);
    }

    pub fn write_owner(&self, owners: &mut PaintOwners<'_>, role: PaintRole) -> bool {
        let paint = match self.role_state(role) {
            WellState::Single(paint) => paint.clone(),
            WellState::None => Paint::None,
            WellState::Mixed => return false,
        };
        match owners.paint_mut(role) {
            Some(target) if *target != paint => {
                *target = paint;
                true
            }
            _ => false,
        }
    }

    pub fn preview_popup(&mut self, owners: &mut PaintOwners<'_>, paint: Paint) -> bool {
        let Some(role) = self.active_popup else {
            return false;
        };
        self.set_role_state(role, WellState::Single(paint));
        self.write_owner(owners, role)
    }

    pub fn cancel_popup(&mut self, owners: &mut PaintOwners<'_>) {
        if let (Some(role), Some(original)) = (self.active_popup, self.popup_original.take()) {
            if let Some(target) = owners.paint_mut(role) {
                *target = original;
            }
            self.read_owner(owners, role);
        }
        self.active_popup = None;
    }

    pub fn commit_popup(&mut self, owners: &mut PaintOwners<'_>) -> bool {
        let Some(role) = self.active_popup else {
            return false;
        };
        self.write_owner(owners, role);
        let changed = self.popup_original.take().map_or(false, |original| owners.display(role) != original);
        self.active_popup = None;
        changed
    }

    pub fn preview_state(&mut self, paint: Paint) -> bool {
        let Some(role) = self.active_popup else {
            return false;
        };
        self.set_role_state(role, WellState::Single(paint));
        true
    }

    pub fn cancel_state(&mut self) {
        if let Some(role) = self.active_popup {
            match self.popup_original.take() {
                Some(original) => self.set_role_state(role, WellState::Single(original)),
                None => self.set_role_state(role, WellState::Mixed),
            }
        }
        self.active_popup = None;
    }

    pub fn commit_command(&self, role: PaintRole, document: &dtool_core::document::Document, ids: &[dtool_core::object::ObjectId]) -> Option<dtool_protocol::Command> {
        let paint = match self.role_state(role) {
            WellState::Single(paint) => paint.clone(),
            WellState::None => Paint::None,
            WellState::Mixed => return None,
        };
        let mut updates = Vec::new();
        for id in ids {
            let object = document.get_object(*id)?;
            if object.locked { continue; }
            let original = if role == PaintRole::Edge { &object.stroke.paint } else { &object.fill };
            let mut paint = paint.clone();
            if let Paint::Gradient { geom, .. } = &mut paint
                && *geom == dtool_core::paint::GradientGeom::default()
                && !matches!(original, Paint::Gradient { .. })
            {
                let [width, height] = object.size.map(f64::from);
                let angle = if width.abs() > 0.001 { 0.0 } else { 270.0 };
                *geom = dtool_core::paint::GradientGeom::from_legacy_angle(angle, [0.0, 0.0, width.max(0.001), height.max(0.001)]);
            }
            let update = match role {
                PaintRole::SourceObject if object.fill != paint => serde_json::json!({ "id": id.0, "fill": paint }),
                PaintRole::Edge if object.stroke.paint != paint => {
                    let mut stroke = object.stroke.clone();
                    if matches!(stroke.paint, Paint::None) && !matches!(paint, Paint::None) && stroke.width == 0.0 {
                        stroke.width = dtool_core::units::units_per_point(document.units, document.dpi);
                    }
                    stroke.paint = paint.clone();
                    serde_json::json!({ "id": id.0, "stroke": stroke })
                }
                PaintRole::SourceObject | PaintRole::Edge => continue,
                _ => return None,
            };
            updates.push(update);
        }
        match updates.len() {
            0 => None,
            1 => Some(dtool_protocol::Command::with_params("object.update", updates.remove(0))),
            _ => {
                let commands: Vec<serde_json::Value> = updates.into_iter().map(|params| serde_json::json!({ "name": "object.update", "params": params })).collect();
                Some(dtool_protocol::Command::with_params("edit.batch", serde_json::json!({ "commands": commands })))
            }
        }
    }

    pub fn commit_state(&mut self, document: &dtool_core::document::Document, ids: &[dtool_core::object::ObjectId]) -> Option<dtool_protocol::Command> {
        let role = self.active_popup?;
        self.active_popup = None;
        self.popup_original = None;
        self.commit_command(role, document, ids)
    }

    pub fn swap_paints(&mut self) {
        let fill = match &self.fill_state {
            WellState::Single(p) => p.clone(),
            _ => Paint::None,
        };
        let stroke = match &self.stroke_state {
            WellState::Single(p) => p.clone(),
            _ => Paint::None,
        };
        self.fill_state = WellState::Single(stroke);
        self.stroke_state = WellState::Single(fill);
    }

    pub fn set_none(&mut self) {
        if let Some(role) = self.active_popup {
            match role {
                PaintRole::SourceObject => self.fill_state = WellState::None,
                PaintRole::Edge => self.stroke_state = WellState::None,
                _ => {}
            }
        } else {
            self.fill_state = WellState::None;
            self.stroke_state = WellState::None;
        }
    }

    pub fn set_default(&mut self) {
        let defaults = dtool_core::commands::defaults::current();
        self.fill_state = WellState::Single(defaults.fill);
        self.stroke_state = WellState::Single(defaults.stroke.paint);
    }
}

fn shared_state(mut paints: impl Iterator<Item = Paint>) -> WellState {
    let Some(first) = paints.next() else {
        return WellState::None;
    };
    if paints.all(|paint| paint == first) { WellState::Single(first) } else { WellState::Mixed }
}

fn rand_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
}

fn show_paint_kinds(ui: &mut egui::Ui, state: &mut WellState, kinds: &[&str]) {
    ui.horizontal_wrapped(|ui| {
        for &key in kinds {
            let current = matches!((&*state, key),
                (WellState::Single(Paint::Solid { .. }), "fill.kind.solid")
                | (WellState::Single(Paint::Gradient { .. }), "fill.kind.gradient")
                | (WellState::Single(Paint::Hatch(_)), "fill.kind.hatch")
            );
            if ui.selectable_label(current, text(key)).clicked() && !current
                && let Some(mut paint) = default_fill_for_kind(key)
            {
                if let (WellState::Single(previous), Paint::Solid { colour }) = (&*state, &mut paint)
                    && let Some(previous) = dtool_core::scene::representative_colour(previous)
                {
                    *colour = previous;
                }
                if let (WellState::Single(Paint::Solid { colour }), Paint::Gradient { stops, .. }) = (&*state, &mut paint) {
                    stops[0].colour = *colour;
                    if stops[1].colour == *colour {
                        stops[1].colour = dtool_core::paint::Colour::new(255, 255, 255, 255);
                    }
                }
                *state = WellState::Single(paint);
            }
        }
        let none = matches!(state, WellState::None | WellState::Single(Paint::None));
        if ui.selectable_label(none, text("paint.none")).clicked() {
            *state = WellState::None;
        }
    });
}

fn edit_well_colours(
    ui: &mut egui::Ui,
    state: &mut WellState,
    display: &dtool_core::paint::DisplayColourCtx,
    icc: Option<&dtool_core::colour::model::ObjectIcc>,
) {
    let WellState::Single(paint) = state else { return };
    let edit = |ui: &mut egui::Ui, colour: &mut dtool_core::paint::Colour| {
        let solid = Paint::Solid { colour: *colour };
        let mut rgba = paint_well_colour_in(&solid, display, icc).unwrap_or_else(|| colour_to_egui(colour));
        let button = dtool_design::colour_well(ui, rgba, "edit-paint-colour");
        egui::Popup::menu(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if egui::color_picker::color_picker_color32(ui, &mut rgba, egui::color_picker::Alpha::BlendOrAdditive) {
                    let [r, g, b, a] = rgba.to_srgba_unmultiplied();
                    *colour = dtool_core::paint::Colour::new(r, g, b, a);
                }
            });
    };
    match paint {
        Paint::Solid { colour } => edit(ui, colour),
        Paint::Gradient { stops, .. } => {
            for (index, stop) in stops.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        edit(ui, &mut stop.colour);
                        ui.label(format!("{} {}  ·  {:.0}%", text("ctx.gradient.stop"), index + 1, stop.offset * 100.0));
                    });
                });
            }
        }
        _ => {}
    }
}

fn colour_to_egui(colour: &dtool_core::paint::Colour) -> egui::Color32 {
    let hex = format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        colour.r, colour.g, colour.b, colour.a
    );
    dtool_design::tokens::parse_hex(&hex).unwrap_or_else(|| dtool_design::color("accent"))
}

pub fn paint_well_colour(paint: &Paint) -> Option<egui::Color32> {
    paint_well_colour_in(paint, &dtool_core::paint::DisplayColourCtx::app_default(), None)
}

pub fn paint_well_colour_in(paint: &Paint, display: &dtool_core::paint::DisplayColourCtx, icc: Option<&dtool_core::colour::model::ObjectIcc>) -> Option<egui::Color32> {
    match &dtool_core::paint::display_paint(paint, icc, display) {
        Paint::Solid { colour } => Some(colour_to_egui(colour)),
        Paint::Gradient { stops, .. } => stops.first().map(|stop| colour_to_egui(&stop.colour)),
        Paint::Hatch(hatch) => Some(colour_to_egui(&hatch.colour)),
        _ => None,
    }
}

pub(crate) fn paint_gradient_well(painter: &egui::Painter, rect: egui::Rect, paint: &Paint, display: &dtool_core::paint::DisplayColourCtx) -> bool {
    if !matches!(paint, Paint::Gradient { .. }) { return false; }
    let Paint::Gradient { stops, .. } = dtool_core::paint::display_paint(paint, None, display) else { return false };
    if stops.is_empty() { return false; }
    let mut mesh = egui::Mesh::default();
    for (index, t) in std::iter::once(0.0)
        .chain(stops.iter().map(|stop| stop.offset.clamp(0.0, 1.0)))
        .chain(std::iter::once(1.0))
        .enumerate()
    {
        let x = egui::lerp(rect.left()..=rect.right(), t);
        let colour = colour_to_egui(&dtool_core::paint::interpolate_stops(&stops, t));
        mesh.colored_vertex(egui::pos2(x, rect.top()), colour);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), colour);
        if index > 0 {
            let current = index as u32 * 2;
            mesh.add_triangle(current - 2, current - 1, current);
            mesh.add_triangle(current, current - 1, current + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    true
}

pub fn well_value(state: &WellState) -> dtool_design::WellValue {
    match state {
        WellState::Mixed => dtool_design::WellValue::Mixed,
        WellState::None | WellState::Single(Paint::None) => dtool_design::WellValue::None,
        WellState::Single(paint) => paint_well_colour(paint).map_or(dtool_design::WellValue::None, dtool_design::WellValue::Colour),
    }
}

pub fn mixed_well(ui: &mut egui::Ui, id: &str, tooltip: impl Into<String>) -> egui::Response {
    let state = dtool_design::ControlState::default().mixed(true).tooltip(tooltip);
    dtool_design::colour_well_with(ui, dtool_design::WellValue::Mixed, id, &state)
}

pub fn paint_well(ui: &mut egui::Ui, paint: &Paint, id: &str) -> egui::Response {
    paint_well_in(ui, paint, id, &dtool_core::paint::DisplayColourCtx::app_default(), None)
}

pub fn paint_well_in(ui: &mut egui::Ui, paint: &Paint, id: &str, display: &dtool_core::paint::DisplayColourCtx, icc: Option<&dtool_core::colour::model::ObjectIcc>) -> egui::Response {
    let fill = paint_well_colour_in(paint, display, icc).unwrap_or_else(|| dtool_design::color("accent"));
    dtool_design::colour_well(ui, fill, id)
}
fn solid_paint(r: u8, g: u8, b: u8, a: u8) -> Paint {
    Paint::Solid { colour: dtool_core::paint::Colour::new(r, g, b, a) }
}

#[cfg(test)]
mod owner_snapshot_contracts {
    use super::*;

    fn red() -> Paint {
        solid_paint(255, 0, 0, 255)
    }

    #[test]
    fn popup_cancel_restores_owner_paint() {
        let mut cell = red();
        let mut owners = PaintOwners { source: None, frame: None, cell: Some(&mut cell), edge: None, glyph: None };
        let mut wells = FillStrokeWells::default();
        wells.open_popup_for(&owners, PaintRole::Cell);
        assert!(wells.preview_popup(&mut owners, solid_paint(0, 0, 255, 255)));
        assert_eq!(owners.display(PaintRole::Cell), solid_paint(0, 0, 255, 255));
        wells.cancel_popup(&mut owners);
        assert_eq!(owners.display(PaintRole::Cell), red());
        assert!(wells.active_popup.is_none());
        drop(owners);
        assert_eq!(cell, red());
    }

    #[test]
    fn paint_well_colour_reads_typed_paint() {
        assert_eq!(paint_well_colour(&red()), Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255)));
        assert_eq!(paint_well_colour(&Paint::None), None);
    }

    #[test]
    fn popup_commit_writes_same_owner() {
        let mut edge = StrokeStyle::default();
        let mut owners = PaintOwners { source: None, frame: None, cell: None, edge: Some(&mut edge), glyph: None };
        let mut wells = FillStrokeWells::default();
        wells.open_popup_for(&owners, PaintRole::Edge);
        wells.preview_popup(&mut owners, red());
        assert!(wells.commit_popup(&mut owners));
        drop(owners);
        assert_eq!(edge.paint, red());
    }
}
