use dtool_core::shape::ShapeKind;
use dtool_strings::text;

const P0: [ShapeKind; 4] = [ShapeKind::Rectangle, ShapeKind::Triangle, ShapeKind::Ellipse, ShapeKind::RoundedRectangle];

pub fn is_p0(kind: ShapeKind) -> bool {
    P0.contains(&kind)
}

pub fn render(ui: &mut egui::Ui, selected: ShapeKind) -> Option<ShapeKind> {
    let mut result = None;
    let kinds = [
        (ShapeKind::Rectangle, "tool.shape.rectangle"),
        (ShapeKind::Triangle, "tool.shape.triangle"),
        (ShapeKind::Ellipse, "tool.shape.ellipse"),
        (ShapeKind::RoundedRectangle, "tool.shape.rounded_rectangle"),
        (ShapeKind::Polygon, "tool.shape.polygon"),
        (ShapeKind::Pie, "tool.shape.pie"),
        (ShapeKind::Arrow, "tool.shape.arrow"),
        (ShapeKind::Star, "tool.shape.star"),
        (ShapeKind::Donut, "tool.shape.donut"),
        (ShapeKind::CalloutEllipse, "tool.shape.callout_ellipse"),
        (ShapeKind::CalloutRectangle, "tool.shape.callout_rectangle"),
        (ShapeKind::Diamond, "tool.shape.diamond"),
        (ShapeKind::DoubleStar, "tool.shape.doublestar"),
        (ShapeKind::Segment, "tool.shape.segment"),
        (ShapeKind::Cog, "tool.shape.cog"),
        (ShapeKind::Trapezoid, "tool.shape.trapezoid"),
        (ShapeKind::SquareStar, "tool.shape.squarestar"),
        (ShapeKind::Crescent, "tool.shape.crescent"),
        (ShapeKind::Heart, "tool.shape.heart"),
        (ShapeKind::Tear, "tool.shape.tear"),
        (ShapeKind::Cloud, "tool.shape.cloud"),
        (ShapeKind::Cat, "tool.shape.cat"),
        (ShapeKind::Spiral, "tool.shape.spiral"),
    ];
    for (kind, key) in kinds {
        let label = text(key);
        if is_p0(kind) {
            if ui.selectable_label(selected == kind, label).clicked() {
                result = Some(kind);
            }
        } else {
            dtool_design::button_with(ui, label, &dtool_design::ControlState::enabled(false));
        }
    }
    result
}
