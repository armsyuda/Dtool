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
    let mut scrub = None;
    let compact = ui.available_width() < 860.0;
    ui.push_id(("rounded_corners", object.id), |ui| {
        if compact {
            ui.menu_button(text("corner.options"), |ui| {
                ui.horizontal(|ui| switches(ui, &mut corner, half));
                for &(index, key) in visible_fields(&corner) {
                    let value = ui.horizontal(|ui| fields(ui, session, &mut corner, half, index, key)).inner;
                    scrub = value.or(scrub);
                }
            });
        } else {
            switches(ui, &mut corner, half);
            ui.separator();
            for &(index, key) in visible_fields(&corner) {
                scrub = fields(ui, session, &mut corner, half, index, key).or(scrub);
            }
        }
    });
    let command = |corner| Command::with_params("shape.params.set", json!({
        "id": object.id, "corner": corner,
    }));
    if corner != *original {
        return Some(command(corner));
    }
    if let Some((index, value)) = scrub {
        set_radius(&mut corner, index, value);
        crate::app_canvas::stage_geometry_preview(ui, session, command(corner));
    }
    None
}

fn visible_fields(corner: &CornerRecipe) -> &'static [(usize, &'static str)] {
    if corner.lock {
        &[(0, "context.shape.corner_radius")]
    } else {
        &CORNERS
    }
}

fn set_radius(corner: &mut CornerRecipe, index: usize, radius: f32) {
    if corner.lock {
        corner.radii.fill(radius);
    } else {
        corner.radii[index] = radius;
    }
}

fn switches(ui: &mut egui::Ui, corner: &mut CornerRecipe, half: f32) {
    if dtool_design::toggle(ui, &mut corner.lock, text("corner.single_radius")).changed() && corner.lock {
        let radius = corner.radii[0];
        corner.radii.fill(radius);
    }
    if dtool_design::toggle(ui, &mut corner.absolute, text("corner.absolute")).changed() {
        corner.radii = corner.radii.map(|radius| {
            if corner.absolute { radius.clamp(0.0, 50.0) * half / 50.0 }
            else { radius.clamp(0.0, half) / half * 50.0 }
        });
    }
}

fn fields(ui: &mut egui::Ui, session: &Session, corner: &mut CornerRecipe, half: f32, index: usize, key: &str) -> Option<(usize, f32)> {
    let label = ui.add(egui::Label::new(text(key)).sense(egui::Sense::click()));
    let mut style = corner.styles[index];
    let name = STYLES.iter().find(|(candidate, _)| *candidate == style)
        .map_or("corner.style.rounded", |(_, key)| *key);
    egui::ComboBox::from_id_salt(("style", index)).width(54.0).selected_text(text(name))
        .show_ui(ui, |ui| {
            for (value, key) in STYLES {
                ui.selectable_value(&mut style, value, text(key));
            }
        }).response.labelled_by(label.id);
    if style != corner.styles[index] {
        if corner.lock {
            corner.styles.fill(style);
        } else {
            corner.styles[index] = style;
        }
    }

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
    let event = ui.add_enabled_ui(style != CornerStyle::None, |ui| {
        let (field, event) = dtool_design::numeric_field(ui, id, shown, &spec);
        field.labelled_by(label.id).on_hover_text(text(key));
        event
    }).inner;
    let enabled = ui.is_enabled() && style != CornerStyle::None;
    if enabled && label.double_clicked() {
        use dtool_core::context_toolbar::{SHAPE_CORNER_RADIUS, find_option};
        if let Some(percent) = find_option("tool.shape.rounded_rectangle", SHAPE_CORNER_RADIUS)
            .and_then(|spec| spec.default_value().as_f64())
        {
            dtool_design::numeric_cancel_edit(ui.ctx(), id);
            let radius = if corner.absolute { percent * f64::from(half) / 50.0 } else { percent };
            set_radius(corner, index, spec.clamp(radius) as f32);
        }
        return None;
    }
    if let NumericEvent::Commit(value) = event {
        set_radius(corner, index, value as f32);
        return None;
    }
    let state = dtool_design::numeric_state(ui.ctx(), id);
    (enabled && state.scrubbing && state.previewed).then_some((index, state.scrub as f32))
}
