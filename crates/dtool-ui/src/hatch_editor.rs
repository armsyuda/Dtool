use crate::fill_stroke::PaintOwners;
use dtool_core::colour::swatch::{self, SwatchTarget};
use dtool_core::commands::hatch::{FILL_HATCH_SET, SWATCH_HATCH_ADD};
use dtool_core::paint::{Colour, Hatch, HatchLine, HatchUnits, Paint, PaintRole};
use dtool_core::{Command, CommandError, ObjectId, Session};
use serde_json::{json, Value};
use dtool_core::colour::{BitDepth, ColourFormat, ColourModel, ColourPrefs};
use dtool_strings::text;

pub const UNITS: [HatchUnits; 10] = [
    HatchUnits::Document,
    HatchUnits::Px,
    HatchUnits::Pt,
    HatchUnits::Pc,
    HatchUnits::In,
    HatchUnits::Ft,
    HatchUnits::Yd,
    HatchUnits::Mm,
    HatchUnits::Cm,
    HatchUnits::M,
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HatchEditorState {
    pub hatch: Option<Hatch>,
    pub original: Option<Hatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HatchRole {
    Fill,
    Stroke,
}

impl HatchRole {
    pub fn name(self) -> &'static str {
        match self {
            HatchRole::Fill => "fill",
            HatchRole::Stroke => "stroke",
        }
    }

    fn target(self) -> SwatchTarget {
        match self {
            HatchRole::Fill => SwatchTarget::Fill,
            HatchRole::Stroke => SwatchTarget::Stroke,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineEdit {
    Add,
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
    ToggleDash(usize),
}

impl HatchEditorState {
    pub fn new() -> Self {
        Self { hatch: None, original: None }
    }

    pub fn set_hatch(&mut self, hatch: Option<Hatch>) {
        self.original = hatch.clone();
        self.hatch = hatch;
    }

    pub fn get_hatch(&self) -> Option<&Hatch> {
        self.hatch.as_ref()
    }

    pub fn load_from_owner(&mut self, owners: &PaintOwners<'_>, role: PaintRole) -> bool {
        self.set_hatch(match owners.display(role) {
            Paint::Hatch(hatch) => Some(hatch),
            _ => None,
        });
        self.hatch.is_some()
    }

    pub fn load_from_session(&mut self, session: &Session, ids: &[ObjectId], role: HatchRole) -> bool {
        let found = ids
            .iter()
            .find_map(|id| session.document.objects.iter().find(|object| object.id == *id))
            .and_then(|object| match swatch::target_paint(object, role.target()) {
                Paint::Hatch(hatch) => Some(hatch.clone()),
                _ => None,
            });
        self.set_hatch(found);
        self.hatch.is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.hatch != self.original
    }

    pub fn cancel(&mut self) {
        self.hatch = self.original.clone();
    }

    pub fn set_command(&self, ids: Option<&[ObjectId]>, role: HatchRole) -> Option<Command> {
        let hatch = self.hatch.as_ref()?;
        let mut params = json!({ "role": role.name(), "hatch": hatch });
        if let Some(ids) = ids {
            params["ids"] = Value::from(ids.iter().map(|id| id.0).collect::<Vec<u64>>());
        }
        Some(Command::with_params(FILL_HATCH_SET, params))
    }

    pub fn swatch_command(&self, name: &str, palette: Option<&str>) -> Option<Command> {
        let hatch = self.hatch.as_ref()?;
        let mut params = json!({ "name": name, "hatch": hatch });
        if let Some(palette) = palette {
            params["palette"] = Value::from(palette);
        }
        Some(Command::with_params(SWATCH_HATCH_ADD, params))
    }

    pub fn commit(&mut self, session: &mut Session, ids: Option<&[ObjectId]>, role: HatchRole) -> Result<bool, CommandError> {
        if !self.is_dirty() {
            return Ok(false);
        }
        let Some(command) = self.set_command(ids, role) else { return Ok(false) };
        let result = session.run(&command)?;
        self.original = self.hatch.clone();
        Ok(result["changes"][0]["ids"].as_array().is_some_and(|ids| !ids.is_empty()))
    }

    pub fn add_swatch(&self, session: &mut Session, name: &str, palette: Option<&str>) -> Result<Option<u64>, CommandError> {
        let Some(command) = self.swatch_command(name, palette) else { return Ok(None) };
        let result = session.run(&command)?;
        Ok(result["id"].as_u64())
    }

    pub fn apply(&mut self, edit: LineEdit) -> bool {
        let Some(hatch) = &mut self.hatch else { return false };
        let lines = &mut hatch.lines;
        match edit {
            LineEdit::Add => {
                let spacing = lines.last().map(|line| line.spacing).unwrap_or(4.0);
                lines.push(HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing, dash: Vec::new() });
                true
            }
            LineEdit::Remove(index) if index < lines.len() && lines.len() > 1 => {
                lines.remove(index);
                true
            }
            LineEdit::MoveUp(index) if index > 0 && index < lines.len() => {
                lines.swap(index, index - 1);
                true
            }
            LineEdit::MoveDown(index) if index + 1 < lines.len() => {
                lines.swap(index, index + 1);
                true
            }
            LineEdit::ToggleDash(index) if index < lines.len() => {
                let dash = &mut lines[index].dash;
                if dash.is_empty() {
                    dash.extend([1.0, 1.0]);
                } else {
                    dash.clear();
                }
                true
            }
            _ => false,
        }
    }
}

fn to_color32(colour: Colour) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a)
}

fn colour_edit(ui: &mut egui::Ui, label: &str, colour: &mut Colour) -> bool {
    let mut shown = to_color32(*colour);
    let changed = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.color_edit_button_srgba(&mut shown).changed()
        })
        .inner;
    if changed {
        let [r, g, b, a] = shown.to_srgba_unmultiplied();
        *colour = edited_colour(colour, [r, g, b, a]);
    }
    changed
}

pub fn edited_colour(old: &Colour, rgba: [u8; 4]) -> Colour {
    let [r, g, b, a] = rgba;
    if [r, g, b] == [old.r, old.g, old.b] {
        let mut kept = *old;
        kept.a = a;
        return kept;
    }
    let rgb = Colour::new(r, g, b, a);
    if old.model == ColourModel::Rgb {
        return rgb;
    }
    rgb.write_for_document(ColourFormat { model: old.model, depth: BitDepth::U8 }, &ColourPrefs::default())
}

fn unit_label(units: HatchUnits) -> String {
    match units {
        HatchUnits::Document => text("unit.document").to_string(),
        HatchUnits::Px => "px".into(),
        HatchUnits::Pt => "pt".into(),
        HatchUnits::Pc => "pc".into(),
        HatchUnits::In => "in".into(),
        HatchUnits::Ft => "ft".into(),
        HatchUnits::Yd => "yd".into(),
        HatchUnits::Mm => "mm".into(),
        HatchUnits::Cm => "cm".into(),
        HatchUnits::M => "m".into(),
    }
}

fn hatch_number(ui: &mut egui::Ui, salt: (&str, usize, usize), value: &mut f64, label: &str) -> egui::Response {
    let id = egui::Id::NULL.with(("dtool.hatch_editor.number", salt));
    dtool_design::numeric_value(ui, id, value, &dtool_design::NumericSpec::new(label).decimals(6))
}

fn show_line(ui: &mut egui::Ui, index: usize, count: usize, line: &mut HatchLine) -> Option<LineEdit> {
    let mut edit = None;
    ui.horizontal(|ui| {
        let mut degrees = line.angle_rad.to_degrees();
        if hatch_number(ui, ("angle", index, 0), &mut degrees, &text("hatch.angle")).changed() {
            line.angle_rad = degrees.to_radians();
        }
        hatch_number(ui, ("origin_x", index, 0), &mut line.origin[0], &text("hatch.origin_x"));
        hatch_number(ui, ("origin_y", index, 0), &mut line.origin[1], &text("hatch.origin_y"));
        hatch_number(ui, ("shift", index, 0), &mut line.shift, &text("hatch.shift"));
        let mut spacing = line.spacing;
        if hatch_number(ui, ("spacing", index, 0), &mut spacing, &text("hatch.spacing")).changed() && spacing > 0.0 {
            line.spacing = spacing;
        }
        if dtool_design::button(ui, text("hatch.dash")).clicked() {
            edit = Some(LineEdit::ToggleDash(index));
        }
        if index > 0 && dtool_design::button(ui, text("hatch.move_up")).clicked() {
            edit = Some(LineEdit::MoveUp(index));
        }
        if index + 1 < count && dtool_design::button(ui, text("hatch.move_down")).clicked() {
            edit = Some(LineEdit::MoveDown(index));
        }
        if count > 1 && dtool_design::button(ui, text("hatch.remove_line")).clicked() {
            edit = Some(LineEdit::Remove(index));
        }
    });
    if !line.dash.is_empty() {
        ui.horizontal(|ui| {
            let label = text("hatch.dash_value");
            for (slot, value) in line.dash.iter_mut().enumerate() {
                let mut next = *value;
                if hatch_number(ui, ("dash", index, slot), &mut next, &label).changed() && next >= 0.0 {
                    *value = next;
                }
            }
        });
    }
    edit
}

pub fn show(ui: &mut egui::Ui, state: &mut HatchEditorState) -> bool {
    ui.label(text("fill.hatch"));
    let Some(current) = state.hatch.clone() else {
        ui.label(text("hatch.none"));
        return false;
    };
    let mut next = current.clone();
    let mut edit = None;
    colour_edit(ui, &text("hatch.colour"), &mut next.colour);
    let mut has_background = next.background.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut has_background, text("hatch.background")).changed() {
            next.background = if has_background { Some(Colour::new(255, 255, 255, 255)) } else { None };
        }
    });
    if let Some(background) = next.background.as_mut() {
        colour_edit(ui, &text("hatch.background"), background);
    }
    ui.horizontal(|ui| {
        let mut weight = next.weight;
        if hatch_number(ui, ("weight", 0, 0), &mut weight, &text("hatch.weight")).changed() && weight >= 0.0 {
            next.weight = weight;
        }
        let labels: Vec<String> = UNITS.iter().map(|units| unit_label(*units)).collect();
        let options: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut selected = UNITS.iter().position(|units| *units == next.units).unwrap_or(0);
        ui.label(text("hatch.units"));
        if dtool_design::dropdown(ui, "hatch.units", &mut selected, &options).changed() {
            next.units = UNITS[selected];
        }
    });
    ui.horizontal(|ui| {
        ui.label(text("hatch.matrix"));
        for (slot, value) in next.matrix.iter_mut().enumerate() {
            hatch_number(ui, ("matrix", 0, slot), value, "");
        }
    });
    let count = next.lines.len();
    for (index, line) in next.lines.iter_mut().enumerate() {
        if let Some(found) = show_line(ui, index, count, line) {
            edit = Some(found);
        }
    }
    if dtool_design::button(ui, text("hatch.add_line")).clicked() {
        edit = Some(LineEdit::Add);
    }
    let mut changed = next != current;
    state.hatch = Some(next);
    if let Some(edit) = edit {
        changed |= state.apply(edit);
    }
    changed
}

pub fn show_for_session(ui: &mut egui::Ui, state: &mut HatchEditorState, session: &mut Session, role: HatchRole) -> Result<bool, CommandError> {
    let ids = session.selection.ids.clone();
    if !state.is_dirty() && !state.load_from_session(session, &ids, role) {
        ui.label(text("hatch.none"));
        return Ok(false);
    }
    show(ui, state);
    if !state.is_dirty() {
        return Ok(false);
    }
    let mut committed = Ok(false);
    ui.horizontal(|ui| {
        if dtool_design::button(ui, text("button.apply")).clicked() {
            committed = state.commit(session, Some(&ids), role);
        }
        if dtool_design::button(ui, text("button.cancel")).clicked() {
            state.cancel();
        }
    });
    committed
}
