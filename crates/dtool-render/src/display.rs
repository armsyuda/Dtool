use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use dtool_design::color;
use egui::Ui;

pub fn draw_appearance(ui: &mut Ui, fills: &[serde_json::Value], strokes: &[serde_json::Value]) {
    for fill in fills.iter() {
        if let Ok(paint)=serde_json::from_value::<Paint>(fill.clone()) {draw_paint(ui,&paint);continue;}
        let color_key = fill.get("color").and_then(|v| v.as_str()).unwrap_or("panel.bg");
        let c = color(color_key);
        ui.painter().rect_filled(ui.min_rect(), 0.0, c);
    }

    for stroke in strokes.iter() {
        let color_key = stroke.get("color").and_then(|v| v.as_str()).unwrap_or("panel.border");
        let c = color(color_key);
        ui.painter().rect_stroke(ui.min_rect(), 0.0, (dtool_design::stroke_width("thin"), c), egui::StrokeKind::Middle);
    }
}

pub use dtool_core::paint::{gradient_parameter, interpolate_stops};

pub fn rasterise_gradient_geom(
    kind: GradientKind,
    stops: &[GradientStop],
    geom: &GradientGeom,
    width: u32,
    height: u32,
) -> Vec<(u32, u32, Colour)> {
    let mut pixels = Vec::new();
    if width == 0 || height == 0 {
        return pixels;
    }
    for y in 0..height {
        for x in 0..width {
            let t = gradient_parameter(kind, x as f64 + 0.5, y as f64 + 0.5, geom);
            pixels.push((x, y, interpolate_stops(stops, t)));
        }
    }
    pixels
}

pub fn draw_gradient_geom(ui: &mut Ui, kind: GradientKind, stops: &[GradientStop], geom: &GradientGeom) {
    let rect = ui.min_rect();
    let width = rect.width().max(1.0).round() as u32;
    let height = rect.height().max(1.0).round() as u32;
    let pixels = rasterise_gradient_geom(kind, stops, geom, width, height);
    let painter = ui.painter();
    for (x, y, colour) in pixels {
        let pos = egui::pos2(rect.min.x + x as f32, rect.min.y + y as f32);
        painter.rect_filled(
            egui::Rect::from_min_size(pos, egui::vec2(1.0, 1.0)),
            0.0,
            colour_as_egui(colour),
        );
    }
}

pub fn draw_paint(ui: &mut Ui, paint: &Paint) {
    draw_paint_in(ui, paint, &dtool_core::paint::DisplayColourCtx::app_default());
}

pub fn draw_paint_in(ui: &mut Ui, paint: &Paint, display: &dtool_core::paint::DisplayColourCtx) {
    match &dtool_core::paint::display_paint(paint, None, display) {
        Paint::Gradient { kind, stops, geom } => draw_gradient_geom(ui, *kind, stops, geom),
        Paint::Solid { colour } => {
            ui.painter().rect_filled(
                ui.min_rect(),
                0.0,
                colour_as_egui(*colour),
            );
        }
        _ => {}
    }
}

fn colour_as_egui(colour: Colour) -> egui::Color32 {
    dtool_design::tokens::parse_hex(&format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        colour.r, colour.g, colour.b, colour.a
    ))
    .unwrap_or_else(|| color("text"))
}

#[cfg(test)]
mod gradient_display_contracts {
    use super::*;
    #[test]
    fn promoted_linear_points_sample_endpoints_and_center() {
        for angle in [0.0,45.0,90.0] {
            let geom=GradientGeom::from_legacy_angle(angle,[0.0,0.0,100.0,50.0]);
            for (point,expected) in [(geom.start,0.0),(geom.end,1.0),([50.0,25.0],0.5)] {
                assert!((gradient_parameter(GradientKind::Linear,point[0],point[1],&geom)-expected).abs()<1e-6);
            }
        }
    }
    #[test]
    fn all_gradient_kinds_use_the_same_source_points() {
        let geom=GradientGeom{start:[10.0,10.0],end:[10.0,30.0],ratio:0.5};
        assert_eq!(gradient_parameter(GradientKind::Radial,10.0,20.0,&geom),0.5);
        assert_eq!(gradient_parameter(GradientKind::Elliptical,0.0,10.0,&geom),1.0);
        assert_eq!(gradient_parameter(GradientKind::Conical,-10.0,10.0,&geom),0.25);
        let seam=GradientGeom{start:[0.0,0.0],end:[-1.0,-0.0],ratio:1.0};
        assert_eq!(gradient_parameter(GradientKind::Conical,-2.0,0.0,&seam),0.0);
        let collapsed=GradientGeom{start:[2.0,3.0],end:[2.0,3.0],ratio:1.0};
        for kind in [GradientKind::Linear,GradientKind::Elliptical,GradientKind::Radial,GradientKind::Conical] {assert_eq!(gradient_parameter(kind,5.0,9.0,&collapsed),0.0);}
    }
    #[test]
    fn legacy_and_point_rasters_match_after_roundtrip() {
        let value=serde_json::json!({"type":"gradient","gradient_kind":"Linear","angle":45,"bounds":[0,0,100,50],"stops":[{"offset":0.0,"colour":{"r":0,"g":0,"b":0,"a":255}},{"offset":1.0,"colour":{"r":255,"g":255,"b":255,"a":255}}]});
        let paint:Paint=serde_json::from_value(value).unwrap();
        let stored=serde_json::to_value(&paint).unwrap();assert!(stored.get("angle").is_none());
        let restored:Paint=serde_json::from_value(stored).unwrap();
        let Paint::Gradient{kind,stops,geom}=paint else {panic!("not gradient");};
        let Paint::Gradient{kind:restored_kind,stops:restored_stops,geom:restored_geom}=restored else {panic!("not gradient");};
        assert_eq!(rasterise_gradient_geom(kind,&stops,&geom,100,50),rasterise_gradient_geom(restored_kind,&restored_stops,&restored_geom,100,50));
    }
}
