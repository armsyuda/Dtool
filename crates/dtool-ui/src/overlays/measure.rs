use dtool_design::{color, font_size, space};
use dtool_tools::measure::DimensionLine;

pub fn render_measure_overlay(
    ui: &mut egui::Ui,
    line: Option<&DimensionLine>,
) {
    if let Some(dim) = line {
        let len = dim.length();
        let ang = dim.angle_deg();
        let text = format!("{:.2} pt {:.2} deg", len, ang);

        let mid_x = (dim.a[0] + dim.b[0]) * 0.5;
        let mid_y = (dim.a[1] + dim.b[1]) * 0.5;

        let pos = egui::pos2(mid_x as f32, mid_y as f32);

        ui.painter().text(
            pos,
            egui::Align2::CENTER_CENTER,
            &text,
            dtool_design::font_size("md"),
            color("text.primary"),
        );
    }
}
