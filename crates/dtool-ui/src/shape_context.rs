use dtool_core::shape::{ShapeKind, StarRecipe, TriangleRecipe, CornerRecipe, ArrowRecipe, CalloutEllipseRecipe, CalloutRectangleRecipe, DonutRecipe, PieRecipe, HeartRecipe, CogRecipe, TearRecipe, CrescentRecipe, CloudRecipe, SegmentRecipe, CatRecipe, SquareStarRecipe, DoubleStarRecipe, DiamondRecipe, TrapezoidRecipe, PolygonRecipe, SpiralRecipe, SpiralStyle};
use dtool_design::{number_field, toggle, context_bar_section, text_field, dropdown};
use dtool_strings::text;

pub fn render_star(ui: &mut egui::Ui, recipe: &mut StarRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut inner = recipe.inner_radius;
            number_field(ui, text("shape.star.inner_radius"), &mut inner);
            recipe.inner_radius = inner;

            let mut outer = recipe.outer_radius;
            number_field(ui, text("shape.star.outer_radius"), &mut outer);
            recipe.outer_radius = outer;
        });

        ui.horizontal(|ui| {
            let mut points = recipe.number_of_points as i64;
            number_field(ui, text("shape.star.points"), &mut points);
            recipe.number_of_points = points.max(3) as u32;
        });

        ui.horizontal(|ui| {
            let mut left = recipe.curve_left;
            number_field(ui, text("shape.star.curve_left"), &mut left);
            recipe.curve_left = left;

            let mut right = recipe.curve_right;
            number_field(ui, text("shape.star.curve_right"), &mut right);
            recipe.curve_right = right;
        });

        ui.horizontal(|ui| {
            toggle(ui, text("shape.star.legacy"), &mut recipe.legacy);
        });
    });
}

pub fn render_triangle(ui: &mut egui::Ui, recipe: &mut TriangleRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut p0 = recipe.points[0];
            number_field(ui, text("shape.triangle.point_1"), &mut p0);
            recipe.points[0] = p0;

            let mut p1 = recipe.points[1];
            number_field(ui, text("shape.triangle.point_2"), &mut p1);
            recipe.points[1] = p1;

            let mut p2 = recipe.points[2];
            number_field(ui, text("shape.triangle.point_3"), &mut p2);
            recipe.points[2] = p2;
        });
    });
}

pub fn render_rectangle(ui: &mut egui::Ui) {
    context_bar_section(ui, |ui| {
        ui.label(text("shape.rectangle.info"));
    });
}

pub fn render_ellipse(ui: &mut egui::Ui) {
    context_bar_section(ui, |ui| {
        ui.label(text("shape.ellipse.info"));
    });
}

pub fn render_rounded_rectangle(ui: &mut egui::Ui, recipe: &mut CornerRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut r0 = recipe.radii[0];
            number_field(ui, text("shape.rounded_rect.radius_1"), &mut r0);
            recipe.radii[0] = r0;

            let mut r1 = recipe.radii[1];
            number_field(ui, text("shape.rounded_rect.radius_2"), &mut r1);
            recipe.radii[1] = r1;

            let mut r2 = recipe.radii[2];
            number_field(ui, text("shape.rounded_rect.radius_3"), &mut r2);
            recipe.radii[2] = r2;

            let mut r3 = recipe.radii[3];
            number_field(ui, text("shape.rounded_rect.radius_4"), &mut r3);
            recipe.radii[3] = r3;
        });

        ui.horizontal(|ui| {
            toggle(ui, text("shape.rounded_rect.absolute"), &mut recipe.absolute);
            toggle(ui, text("shape.rounded_rect.lock"), &mut recipe.lock);
        });
    });
}

pub fn render_arrow(ui: &mut egui::Ui, recipe: &mut ArrowRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut head_width = recipe.head_width;
            number_field(ui, text("shape.arrow.head_width"), &mut head_width);
            recipe.head_width = head_width;

            let mut head_length = recipe.head_length;
            number_field(ui, text("shape.arrow.head_length"), &mut head_length);
            recipe.head_length = head_length;
        });

        ui.horizontal(|ui| {
            let mut shaft_width = recipe.shaft_width;
            number_field(ui, text("shape.arrow.shaft_width"), &mut shaft_width);
            recipe.shaft_width = shaft_width;

            let mut shaft_length = recipe.shaft_length;
            number_field(ui, text("shape.arrow.shaft_length"), &mut shaft_length);
            recipe.shaft_length = shaft_length;
        });

        ui.horizontal(|ui| {
            let mut tail_width = recipe.tail_width;
            number_field(ui, text("shape.arrow.tail_width"), &mut tail_width);
            recipe.tail_width = tail_width;
        });
    });
}

pub fn render_callout_ellipse(ui: &mut egui::Ui, recipe: &mut CalloutEllipseRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut rx = recipe.radius_x;
            number_field(ui, text("shape.callout_ellipse.radius_x"), &mut rx);
            recipe.radius_x = rx;

            let mut ry = recipe.radius_y;
            number_field(ui, text("shape.callout_ellipse.radius_y"), &mut ry);
            recipe.radius_y = ry;
        });

        ui.horizontal(|ui| {
            let mut angle = recipe.pointer_angle;
            number_field(ui, text("shape.callout_ellipse.pointer_angle"), &mut angle);
            recipe.pointer_angle = angle;

            let mut length = recipe.pointer_length;
            number_field(ui, text("shape.callout_ellipse.pointer_length"), &mut length);
            recipe.pointer_length = length;

            let mut width = recipe.pointer_width;
            number_field(ui, text("shape.callout_ellipse.pointer_width"), &mut width);
            recipe.pointer_width = width;
        });
    });
}

pub fn render_callout_rectangle(ui: &mut egui::Ui, recipe: &mut CalloutRectangleRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut radius = recipe.corner_radius;
            number_field(ui, text("shape.callout_rect.corner_radius"), &mut radius);
            recipe.corner_radius = radius;

            toggle(ui, text("shape.callout_rect.absolute_radii"), &mut recipe.absolute_radii);
            toggle(ui, text("shape.callout_rect.lock_radii"), &mut recipe.lock_radii);
        });

        ui.horizontal(|ui| {
            let mut angle = recipe.pointer_angle;
            number_field(ui, text("shape.callout_rect.pointer_angle"), &mut angle);
            recipe.pointer_angle = angle;

            let mut length = recipe.pointer_length;
            number_field(ui, text("shape.callout_rect.pointer_length"), &mut length);
            recipe.pointer_length = length;

            let mut width = recipe.pointer_width;
            number_field(ui, text("shape.callout_rect.pointer_width"), &mut width);
            recipe.pointer_width = width;
        });
    });
}

pub fn render_donut(ui: &mut egui::Ui, recipe: &mut DonutRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut inner = recipe.inner_radius;
            number_field(ui, text("shape.donut.inner_radius"), &mut inner);
            recipe.inner_radius = inner;

            let mut outer = recipe.outer_radius;
            number_field(ui, text("shape.donut.outer_radius"), &mut outer);
            recipe.outer_radius = outer;
        });
    });
}

pub fn render_pie(ui: &mut egui::Ui, recipe: &mut PieRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut start = recipe.start_angle;
            number_field(ui, text("shape.pie.start_angle"), &mut start);
            recipe.start_angle = start;

            let mut end = recipe.end_angle;
            number_field(ui, text("shape.pie.end_angle"), &mut end);
            recipe.end_angle = end;
        });

        ui.horizontal(|ui| {
            let mut inner = recipe.inner_radius;
            number_field(ui, text("shape.pie.inner_radius"), &mut inner);
            recipe.inner_radius = inner;

            let mut outer = recipe.outer_radius;
            number_field(ui, text("shape.pie.outer_radius"), &mut outer);
            recipe.outer_radius = outer;
        });
    });
}

pub fn render_heart(ui: &mut egui::Ui, recipe: &mut HeartRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.heart.indent"), &mut indent);
            recipe.indent = indent;

            let mut pointiness = recipe.pointiness;
            number_field(ui, text("shape.heart.pointiness"), &mut pointiness);
            recipe.pointiness = pointiness;
        });
    });
}

pub fn render_cog(ui: &mut egui::Ui, recipe: &mut CogRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut teeth = recipe.number_of_teeth as i64;
            number_field(ui, text("shape.cog.teeth"), &mut teeth);
            recipe.number_of_teeth = teeth.max(3) as u32;

            let mut depth = recipe.tooth_depth;
            number_field(ui, text("shape.cog.depth"), &mut depth);
            recipe.tooth_depth = depth;
        });

        ui.horizontal(|ui| {
            let mut width = recipe.tooth_width;
            number_field(ui, text("shape.cog.width"), &mut width);
            recipe.tooth_width = width;

            let mut inner = recipe.inner_radius;
            number_field(ui, text("shape.cog.inner_radius"), &mut inner);
            recipe.inner_radius = inner;

            let mut outer = recipe.outer_radius;
            number_field(ui, text("shape.cog.outer_radius"), &mut outer);
            recipe.outer_radius = outer;
        });
    });
}

pub fn render_tear(ui: &mut egui::Ui, recipe: &mut TearRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.tear.indent"), &mut indent);
            recipe.indent = indent;

            let mut pointiness = recipe.pointiness;
            number_field(ui, text("shape.tear.pointiness"), &mut pointiness);
            recipe.pointiness = pointiness;
        });
    });
}

pub fn render_crescent(ui: &mut egui::Ui, recipe: &mut CrescentRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.crescent.indent"), &mut indent);
            recipe.indent = indent;
        });
    });
}

pub fn render_cloud(ui: &mut egui::Ui, recipe: &mut CloudRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.cloud.indent"), &mut indent);
            recipe.indent = indent;

            let mut pointiness = recipe.pointiness;
            number_field(ui, text("shape.cloud.pointiness"), &mut pointiness);
            recipe.pointiness = pointiness;
        });
    });
}

pub fn render_segment(ui: &mut egui::Ui, recipe: &mut SegmentRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.segment.indent"), &mut indent);
            recipe.indent = indent;

            let mut pointiness = recipe.pointiness;
            number_field(ui, text("shape.segment.pointiness"), &mut pointiness);
            recipe.pointiness = pointiness;
        });
    });
}

pub fn render_cat(ui: &mut egui::Ui, recipe: &mut CatRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.cat.indent"), &mut indent);
            recipe.indent = indent;

            let mut pointiness = recipe.pointiness;
            number_field(ui, text("shape.cat.pointiness"), &mut pointiness);
            recipe.pointiness = pointiness;
        });
    });
}

pub fn render_square_star(ui: &mut egui::Ui, recipe: &mut SquareStarRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut sides = recipe.number_of_sides as i64;
            number_field(ui, text("shape.squarestar.sides"), &mut sides);
            recipe.number_of_sides = sides.max(3) as u32;

            let mut cutout = recipe.cutout;
            number_field(ui, text("shape.squarestar.cutout"), &mut cutout);
            recipe.cutout = cutout;
        });
    });
}

pub fn render_double_star(ui: &mut egui::Ui, recipe: &mut DoubleStarRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut inner = recipe.inner_radius;
            number_field(ui, text("shape.doublestar.inner_radius"), &mut inner);
            recipe.inner_radius = inner;

            let mut point = recipe.point_radius;
            number_field(ui, text("shape.doublestar.point_radius"), &mut point);
            recipe.point_radius = point;
        });

        ui.horizontal(|ui| {
            let mut points = recipe.number_of_points as i64;
            number_field(ui, text("shape.doublestar.points"), &mut points);
            recipe.number_of_points = points.max(3) as u32;
        });
    });
}

pub fn render_diamond(ui: &mut egui::Ui, recipe: &mut DiamondRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.diamond.indent"), &mut indent);
            recipe.indent = indent;
        });
    });
}

pub fn render_trapezoid(ui: &mut egui::Ui, recipe: &mut TrapezoidRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut indent = recipe.indent;
            number_field(ui, text("shape.trapezoid.indent"), &mut indent);
            recipe.indent = indent;
        });
    });
}

pub fn render_polygon(ui: &mut egui::Ui, recipe: &mut PolygonRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let mut sides = recipe.number_of_sides as i64;
            number_field(ui, text("ctx.polygon.sides"), &mut sides);
            recipe.number_of_sides = sides.max(3) as u32;

            let mut curve = recipe.curve;
            number_field(ui, text("ctx.polygon.curve"), &mut curve);
            recipe.curve = curve;
        });

        ui.horizontal(|ui| {
            toggle(ui, text("ctx.polygon.smooth_points"), &mut recipe.smooth_points);
        });
    });
}

pub fn render_spiral(ui: &mut egui::Ui, recipe: &mut SpiralRecipe) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            let style_options = [
                (SpiralStyle::Linear, "ctx.spiral.style.linear"),
                (SpiralStyle::Decaying, "ctx.spiral.style.decaying"),
                (SpiralStyle::SemiCircular, "ctx.spiral.style.semi_circular"),
                (SpiralStyle::CounterSemiCircular, "ctx.spiral.style.counter_semi_circular"),
                (SpiralStyle::Fibonacci, "ctx.spiral.style.fibonacci"),
                (SpiralStyle::Plotted, "ctx.spiral.style.plotted"),
            ];
            
            let current_idx = style_options.iter().position(|(s, _)| *s == recipe.style).unwrap_or(0);
            if let Some(new_style) = dropdown(ui, text("ctx.spiral.style"), &style_options, current_idx) {
                recipe.style = new_style;
            }
        });

        ui.horizontal(|ui| {
            let mut turns = recipe.turns;
            number_field(ui, text("ctx.spiral.turns"), &mut turns);
            recipe.turns = turns;
        });

        match recipe.style {
            SpiralStyle::Linear => {
                ui.horizontal(|ui| {
                    let mut part_turn = recipe.part_turn;
                    number_field(ui, text("ctx.spiral.part_turn"), &mut part_turn);
                    recipe.part_turn = part_turn;
                });
            }
            SpiralStyle::Decaying => {
                ui.horizontal(|ui| {
                    let mut decay_per_turn = recipe.decay_per_turn;
                    number_field(ui, text("ctx.spiral.decay"), &mut decay_per_turn);
                    recipe.decay_per_turn = decay_per_turn;
                });
            }
            SpiralStyle::SemiCircular | SpiralStyle::CounterSemiCircular => {
                ui.horizontal(|ui| {
                    let mut segment_angle = recipe.segment_angle;
                    number_field(ui, text("ctx.spiral.segment_angle"), &mut segment_angle);
                    recipe.segment_angle = segment_angle;

                    let mut divisions = recipe.divisions as i64;
                    number_field(ui, text("ctx.spiral.divisions"), &mut divisions);
                    recipe.divisions = divisions.max(1) as u32;
                });
            }
            SpiralStyle::Fibonacci => {
                ui.horizontal(|ui| {
                    let mut inner_radius = recipe.inner_radius;
                    number_field(ui, text("ctx.spiral.inner_radius"), &mut inner_radius);
                    recipe.inner_radius = inner_radius;

                    let mut choke = recipe.choke;
                    number_field(ui, text("ctx.spiral.choke"), &mut choke);
                    recipe.choke = choke;
                });
            }
            SpiralStyle::Plotted => {
                ui.horizontal(|ui| {
                    let mut cap_choke = recipe.cap_choke;
                    number_field(ui, text("ctx.spiral.cap_choke"), &mut cap_choke);
                    recipe.cap_choke = cap_choke;

                    let mut bias = recipe.bias;
                    number_field(ui, text("ctx.spiral.bias"), &mut bias);
                    recipe.bias = bias;
                });
            }
        }

        ui.horizontal(|ui| {
            toggle(ui, text("ctx.spiral.cusped"), &mut recipe.cusped);
            toggle(ui, text("ctx.spiral.flip"), &mut recipe.flip);
        });
    });
}

pub fn render_qr(ui: &mut egui::Ui, payload: &mut String) {
    context_bar_section(ui, |ui| {
        ui.horizontal(|ui| {
            text_field(ui, text("qr.payload"), payload);
        });
    });
}
