use dtool_core::layer_style::{BandPass, BevelType, EffectParams, LayerEffect, OutlineAlign};
use dtool_core::paint::{Colour, GradientKind, GradientStop, Paint};
use dtool_design::{NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;

fn scalar(ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>) -> bool {
    scalar_with_rail(ui, key, value, range, None)
}

fn scalar_with_rail(
    ui: &mut egui::Ui,
    key: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    rail: Option<std::ops::RangeInclusive<f32>>,
) -> bool {
    let before = *value;
    dtool_design::studio_row(ui, text(key), |ui, label| {
        let id = ui.make_persistent_id(key);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let spec = NumericSpec::new("").range(range).width(66.0);
            let (response, event) = dtool_design::numeric_field(ui, id, NumericValue::Value(*value), &spec);
            response.labelled_by(label);
            if let NumericEvent::Commit(next) = event {
                *value = next;
            }
            let width = ui.available_width();
            if key == "fx.light_direction" && width >= 46.0 {
                direction_dial(ui, label, value);
            }
            if let Some(rail) = rail.filter(|_| width >= 32.0) {
                let mut slider = (*value as f32).clamp(*rail.start(), *rail.end());
                let enabled = ui.is_enabled();
                if dtool_design::slider_rail(ui, &mut slider, rail, width, text(key), enabled).labelled_by(label).changed() {
                    *value = f64::from(slider);
                }
            }
        });
    });
    *value != before
}
fn direction_dial(ui: &mut egui::Ui, caption: egui::Id, degrees: &mut f64) {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(46.0, 30.0), egui::Sense::click_and_drag());
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let vector = pointer - rect.center();
        if vector.length_sq() >= 4.0 {
            // Positive angles place the light above the object, opposite the rendered shadow.
            let next = f64::from((-vector.y).atan2(vector.x).to_degrees()).round().rem_euclid(360.0);
            if next != *degrees {
                *degrees = next;
                response.mark_changed();
            }
        }
    }

    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    let center = rect.center();
    painter.circle(center, 12.0, visuals.bg_fill, visuals.bg_stroke);
    let radians = (*degrees as f32).to_radians();
    let direction = egui::vec2(radians.cos(), -radians.sin());
    let ink = visuals.fg_stroke.color;
    painter.line_segment([center, center + direction * 10.0], egui::Stroke::new(1.5, ink));
    painter.circle_filled(center + direction * 9.5, 2.0, ink);
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), *degrees, text("fx.light_direction")));
    response.labelled_by(caption).on_hover_text(text("fx.light_direction"));
}

fn number(ui: &mut egui::Ui, key: &str, value: &mut f32, range: std::ops::RangeInclusive<f64>) -> bool {
    let rail = match key {
        "fx.radius" | "fx.soften" | "fx.width" => Some(0.0..=200.0),
        "fx.offset" | "fx.depth" => Some(-200.0..=200.0),
        _ => None,
    };
    let mut next = f64::from(*value);
    if scalar_with_rail(ui, key, &mut next, range, rail) { *value = next as f32; true } else { false }
}
fn percent(ui: &mut egui::Ui, key: &str, value: &mut f32) -> bool {
    let mut next = f64::from(*value) * 100.0;
    if scalar_with_rail(ui, key, &mut next, 0.0..=100.0, Some(0.0..=100.0)) { *value = (next / 100.0) as f32; true } else { false }
}
fn colour(ui: &mut egui::Ui, key: &str, colour: &mut Colour) -> bool {
    let before = *colour;
    dtool_design::studio_row(ui, text(key), |ui, label| {
        crate::panels::colour::picker_button(ui, colour).labelled_by(label);
    });
    *colour != before
}
fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, key: &str, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let label = options.iter().find(|(item, _)| item == value).map(|(_, key)| text(key)).unwrap_or_default();
    dtool_design::studio_row(ui, text(key), |ui, caption| {
        egui::ComboBox::from_id_salt(key).selected_text(label).show_ui(ui, |ui| {
            for (item, key) in options {
                if ui.selectable_label(*item == *value, text(key)).clicked() { changed |= *item != *value; *value = *item; }
            }
        }).response.labelled_by(caption);
    });
    changed
}
fn paint(ui: &mut egui::Ui, fill: &mut Paint, size: [f32; 2]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.selectable_label(matches!(fill, Paint::Solid { .. }), text("fx.fill.solid")).clicked() && !matches!(fill, Paint::Solid { .. }) { *fill = Paint::Solid { colour: Colour::new(0, 0, 0, 255) }; changed = true; }
        if ui.selectable_label(matches!(fill, Paint::Gradient { .. }), text("fx.fill.gradient")).clicked() && !matches!(fill, Paint::Gradient { .. }) { *fill = dtool_core::layer_style::overlay_gradient(size); changed = true; }
    });
    match fill {
        Paint::Solid { colour: value } => { changed |= colour(ui, "fx.colour", value); }
        Paint::Gradient { kind, stops, geom } => {
            changed |= choice(ui, "fx.gradient.kind", kind, &[(GradientKind::Linear, "fx.gradient.linear"), (GradientKind::Radial, "fx.gradient.radial"), (GradientKind::Elliptical, "fx.gradient.elliptical"), (GradientKind::Conical, "fx.gradient.conical")]);
            changed |= scalar(ui, "fx.gradient.start_x", &mut geom.start[0], -1000000.0..=1000000.0);
            changed |= scalar(ui, "fx.gradient.start_y", &mut geom.start[1], -1000000.0..=1000000.0);
            changed |= scalar(ui, "fx.gradient.end_x", &mut geom.end[0], -1000000.0..=1000000.0);
            changed |= scalar(ui, "fx.gradient.end_y", &mut geom.end[1], -1000000.0..=1000000.0);
            changed |= scalar(ui, "fx.gradient.ratio", &mut geom.ratio, 0.0001..=10000.0);
            let mut remove = None;
            let count = stops.len();
            for (index, stop) in stops.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    changed |= percent(ui, "fx.gradient.stop", &mut stop.offset);
                    changed |= colour(ui, "fx.colour", &mut stop.colour);
                    if count > 2 && dtool_design::button(ui, text("fx.gradient.remove")).clicked() { remove = Some(index); }
                });
            }
            if let Some(index) = remove { stops.remove(index); changed = true; }
            if dtool_design::button(ui, text("fx.gradient.add")).clicked() { stops.push(GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) }); changed = true; }
            if changed { stops.sort_by(|a, b| a.offset.total_cmp(&b.offset)); }
        }
        _ => { ui.label(text("fx.fill.existing")); }
    }
    changed
}

pub fn show_effect(ui: &mut egui::Ui, effect: &mut LayerEffect, size: [f32; 2]) -> bool {
    let mut changed = percent(ui, "fx.opacity", &mut effect.opacity);
    dtool_design::studio_row(ui, text("layers.blend"), |ui, caption| {
        egui::ComboBox::from_id_salt("effect.blend").selected_text(crate::dialogs::blend_options::mode_label(&format!("{:?}", effect.blend))).show_ui(ui, |ui| {
            for mode in dtool_core::blend::BlendMode::ALL {
                if mode == dtool_core::blend::BlendMode::PassThrough { continue; }
                if ui.selectable_label(effect.blend == mode, crate::dialogs::blend_options::mode_label(&format!("{mode:?}"))).clicked() { changed |= effect.blend != mode; effect.blend = mode; }
            }
        }).response.labelled_by(caption);
    });
    match &mut effect.params {
        EffectParams::Blur { radius, preserve_alpha } => { changed |= number(ui, "fx.radius", radius, 0.0..=100000.0); changed |= dtool_design::toggle(ui, preserve_alpha, text("fx.preserve_alpha")).changed(); }
        EffectParams::Shadow { radius, offset, angle, intensity, colour: value, knock_out } => {
            changed |= number(ui, "fx.radius", radius, 0.0..=100000.0); changed |= number(ui, "fx.offset", offset, -100000.0..=100000.0);
            changed |= number(ui, "fx.light_direction", angle, 0.0..=360.0); changed |= percent(ui, "fx.intensity", intensity); changed |= colour(ui, "fx.colour", value);
            changed |= dtool_design::toggle(ui, knock_out, text("fx.knock_out")).changed();
        }
        EffectParams::Glow { radius, intensity, colour: value, from_centre } => { changed |= number(ui, "fx.radius", radius, 0.0..=100000.0); changed |= percent(ui, "fx.intensity", intensity); changed |= colour(ui, "fx.colour", value); changed |= dtool_design::toggle(ui, from_centre, text("fx.from_centre")).changed(); }
        EffectParams::Outline { width, align, fill } => { changed |= number(ui, "fx.width", width, 0.0..=100000.0); changed |= choice(ui, "fx.align", align, &[(OutlineAlign::Outside, "fx.outside"), (OutlineAlign::Centre, "fx.centre"), (OutlineAlign::Inside, "fx.inside")]); changed |= paint(ui, fill, size); }
        EffectParams::Light { radius, depth, soften, azimuth, elevation, bevel, highlight, shadow } => {
            changed |= number(ui, "fx.radius", radius, 0.0..=100000.0); changed |= number(ui, "fx.depth", depth, -100000.0..=100000.0); changed |= number(ui, "fx.soften", soften, 0.0..=100000.0);
            changed |= number(ui, "fx.light_direction", azimuth, 0.0..=360.0); changed |= number(ui, "fx.elevation", elevation, -90.0..=90.0);
            changed |= choice(ui, "fx.bevel", bevel, &[(BevelType::Inner, "fx.inside"), (BevelType::Outer, "fx.outside"), (BevelType::Emboss, "fx.emboss"), (BevelType::Pillow, "fx.pillow")]);
            changed |= colour(ui, "fx.highlight", highlight); changed |= colour(ui, "fx.shadow", shadow);
        }
        EffectParams::Overlay { fill } => { changed |= paint(ui, fill, size); }
    }
    let mut band = effect.band_pass.is_some();
    if dtool_design::toggle(ui, &mut band, text("fx.band_pass")).changed() { effect.band_pass = band.then_some(BandPass { hue: [0.0, 1.0], intensity: [0.0, 1.0] }); changed = true; }
    if let Some(band) = &mut effect.band_pass {
        changed |= percent(ui, "fx.hue_min", &mut band.hue[0]); changed |= percent(ui, "fx.hue_max", &mut band.hue[1]);
        changed |= percent(ui, "fx.intensity_min", &mut band.intensity[0]); changed |= percent(ui, "fx.intensity_max", &mut band.intensity[1]);
    }
    changed
}
