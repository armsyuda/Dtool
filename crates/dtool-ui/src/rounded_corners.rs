use dtool_core::document::Units;
use dtool_core::shape::{CornerRecipe, CornerStyle};
use dtool_core::{Command, Object, Session};
use dtool_design::{NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use serde_json::json;

const STYLES: [(CornerStyle, &str); 5] = [
    (CornerStyle::None, "corner.style.none"),
    (CornerStyle::Rounded, "corner.style.rounded"),
    (CornerStyle::Straight, "corner.style.straight"),
    (CornerStyle::Concave, "corner.style.concave"),
    (CornerStyle::Cutout, "corner.style.cutout"),
];
const CORNERS: [(usize, &str); 4] = [
    (0, "corner.top_left"), (1, "corner.top_right"),
    (3, "corner.bottom_left"), (2, "corner.bottom_right"),
];

pub(crate) fn selected(session: &Session) -> Option<&Object> {
    if !matches!(session.active_tool.as_str(), "tool.move" | "tool.shape.rounded_rectangle") {
        return None;
    }
    dtool_tools::shape_tool::selected_parametric_shape(session)
        .filter(|object| object.shape.as_ref().is_some_and(|shape| shape.corner.is_some()))
}

pub(crate) fn show(ui: &mut egui::Ui, session: &Session, object: &Object) -> Option<Command> {
    let original = object.shape.as_ref()?.corner.as_ref()?;
    let mut corner = original.clone();
    let half = object.size[0].abs().min(object.size[1].abs()) * 0.5;
    if !half.is_finite() || half <= 0.0 {
        return None;
    }
    let compact = ui.available_width() < 860.0;
    ui.push_id(("rounded_corners", object.id), |ui| {
        if compact {
            ui.menu_button(text("corner.options"), |ui| {
                ui.horizontal(|ui| switches(ui, &mut corner, half));
                for (index, key) in CORNERS {
                    ui.horizontal(|ui| fields(ui, session, &mut corner, half, index, key));
                }
            });
        } else {
            ui.label(text("corner.options"));
            switches(ui, &mut corner, half);
            ui.separator();
            for (index, key) in CORNERS {
                fields(ui, session, &mut corner, half, index, key);
            }
        }
    });
    (corner != *original).then(|| Command::with_params("shape.params.set", json!({
        "id": object.id, "corner": corner,
    })))
}

fn switches(ui: &mut egui::Ui, corner: &mut CornerRecipe, half: f32) {
    if ui.checkbox(&mut corner.lock, text("corner.single_radius")).changed() && corner.lock {
        let radius = corner.radii[0];
        corner.radii.fill(radius);
    }
    if ui.checkbox(&mut corner.absolute, text("corner.absolute")).changed() {
        corner.radii = corner.radii.map(|radius| {
            if corner.absolute { radius.clamp(0.0, 50.0) * half / 50.0 }
            else { radius.clamp(0.0, half) / half * 50.0 }
        });
    }
}

fn fields(ui: &mut egui::Ui, session: &Session, corner: &mut CornerRecipe, half: f32, index: usize, key: &str) {
    let label = ui.label(text(key));
    let style = &mut corner.styles[index];
    let name = STYLES.iter().find(|(candidate, _)| *candidate == *style)
        .map_or("corner.style.rounded", |(_, key)| *key);
    egui::ComboBox::from_id_salt(("style", index)).width(54.0).selected_text(text(name))
        .show_ui(ui, |ui| {
            for (value, key) in STYLES {
                ui.selectable_value(style, value, text(key));
            }
        }).response.labelled_by(label.id);

    let suffix = if corner.absolute {
        match session.document.units {
            Units::Px => " px", Units::Mm => " mm", Units::Cm => " cm",
            Units::In => " in", Units::Pt => " pt",
        }
    } else { "%" };
    let limit = if corner.absolute { f64::from(half) } else { 50.0 };
    let spec = NumericSpec::new("").range(0.0..=limit).suffix(suffix).width(64.0).decimals(2);
    let id = ui.make_persistent_id(("radius", index));
    let shown = NumericValue::Value(f64::from(corner.radii[index]));
    let event = ui.add_enabled_ui(*style != CornerStyle::None, |ui| {
        let (field, event) = dtool_design::numeric_field(ui, id, shown, &spec);
        field.labelled_by(label.id).on_hover_text(text(key));
        event
    }).inner;
    if let NumericEvent::Commit(value) = event {
        let radius = value as f32;
        if corner.lock { corner.radii.fill(radius); }
        else { corner.radii[index] = radius; }
    }
}
