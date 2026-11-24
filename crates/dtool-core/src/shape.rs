use serde::{Serialize, Deserialize};
use crate::{CommandError, ObjectId, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ShapeKind { Rectangle, Triangle, Ellipse, RoundedRectangle, Star, Arrow, CalloutEllipse, CalloutRectangle, Donut, Pie, Heart, Cog, Tear, Crescent, Cloud, Segment, Cat, SquareStar, DoubleStar, Diamond, Trapezoid, Polygon, Spiral }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum SpiralStyle { Linear, Decaying, SemiCircular, CounterSemiCircular, Fibonacci, Plotted }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CornerRecipe { pub radii: [f32;4], pub absolute: bool, pub lock: bool }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriangleRecipe { pub points: [f32;3] }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StarRecipe {
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub number_of_points: u32,
    pub curve_left: f32,
    pub curve_right: f32,
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrowRecipe {
    pub head_width: f32,
    pub head_length: f32,
    pub shaft_width: f32,
    pub shaft_length: f32,
    pub tail_width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalloutEllipseRecipe {
    pub radius_x: f32,
    pub radius_y: f32,
    pub pointer_angle: f32,
    pub pointer_length: f32,
    pub pointer_width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalloutRectangleRecipe {
    pub corner_radius: f32,
    pub absolute_radii: bool,
    pub lock_radii: bool,
    pub pointer_angle: f32,
    pub pointer_length: f32,
    pub pointer_width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DonutRecipe {
    pub inner_radius: f32,
    pub outer_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PieRecipe {
    pub start_angle: f32,
    pub end_angle: f32,
    pub inner_radius: f32,
    pub outer_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartRecipe {
    pub indent: f32,
    pub pointiness: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CogRecipe {
    pub number_of_teeth: u32,
    pub tooth_depth: f32,
    pub tooth_width: f32,
    pub inner_radius: f32,
    pub outer_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TearRecipe {
    pub indent: f32,
    pub pointiness: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrescentRecipe {
    pub indent: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudRecipe {
    pub indent: f32,
    pub pointiness: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentRecipe {
    pub indent: f32,
    pub pointiness: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatRecipe {
    pub indent: f32,
    pub pointiness: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SquareStarRecipe {
    pub number_of_sides: u32,
    pub cutout: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoubleStarRecipe {
    pub inner_radius: f32,
    pub point_radius: f32,
    pub number_of_points: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiamondRecipe {
    pub indent: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrapezoidRecipe {
    pub indent: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolygonRecipe {
    pub number_of_sides: u32,
    pub curve: f32,
    pub smooth_points: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpiralRecipe {
    pub style: SpiralStyle,
    pub turns: f32,
    pub part_turn: f32,
    pub segment_angle: f32,
    pub divisions: u32,
    pub decay_per_turn: f32,
    pub decay_per_segment: f32,
    pub inner_radius: f32,
    pub choke: f32,
    pub cap_choke: f32,
    pub bias: f32,
    pub cusped: bool,
    pub flip: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeRecipe {
    pub kind: ShapeKind,
    pub size: [f32;2],
    pub corner: Option<CornerRecipe>,
    pub triangle: Option<TriangleRecipe>,
    pub star: Option<StarRecipe>,
    pub arrow: Option<ArrowRecipe>,
    pub callout_ellipse: Option<CalloutEllipseRecipe>,
    pub callout_rectangle: Option<CalloutRectangleRecipe>,
    pub donut: Option<DonutRecipe>,
    pub pie: Option<PieRecipe>,
    pub heart: Option<HeartRecipe>,
    pub cog: Option<CogRecipe>,
    pub tear: Option<TearRecipe>,
    pub crescent: Option<CrescentRecipe>,
    pub cloud: Option<CloudRecipe>,
    pub segment: Option<SegmentRecipe>,
    pub cat: Option<CatRecipe>,
    pub square_star: Option<SquareStarRecipe>,
    pub double_star: Option<DoubleStarRecipe>,
    pub diamond: Option<DiamondRecipe>,
    pub trapezoid: Option<TrapezoidRecipe>,
    pub polygon: Option<PolygonRecipe>,
    pub spiral: Option<SpiralRecipe>,
}

impl ShapeRecipe {
    pub fn validate(&self) -> Result<(), CommandError> {
        if self.size.iter().any(|v|!v.is_finite() || *v<=0.0) { return Err(CommandError::invalid_params()); }
        match self.kind {
            ShapeKind::Rectangle if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Triangle if self.corner.is_none() && self.triangle.as_ref().is_some_and(|t|t.points.iter().all(|v|v.is_finite() && (0.0..=1.0).contains(v))) && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Ellipse if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::RoundedRectangle if self.corner.as_ref().is_some_and(|c|c.radii.iter().all(|v|v.is_finite() && *v>=0.0)) && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Star if self.corner.is_none() && self.triangle.is_none() && self.star.as_ref().is_some_and(|s|{
                s.inner_radius.is_finite() && s.outer_radius.is_finite() && s.curve_left.is_finite() && s.curve_right.is_finite() && s.number_of_points >= 3
            }) && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Arrow if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.as_ref().is_some_and(|a|{
                a.head_width.is_finite() && a.head_length.is_finite() && a.shaft_width.is_finite() && a.shaft_length.is_finite() && a.tail_width.is_finite()
            }) && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::CalloutEllipse if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.as_ref().is_some_and(|c|{
                c.radius_x.is_finite() && c.radius_y.is_finite() && c.pointer_angle.is_finite() && c.pointer_length.is_finite() && c.pointer_width.is_finite()
            }) && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::CalloutRectangle if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.as_ref().is_some_and(|c|{
                c.corner_radius.is_finite() && c.pointer_angle.is_finite() && c.pointer_length.is_finite() && c.pointer_width.is_finite()
            }) && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Donut if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.as_ref().is_some_and(|d|{
                d.inner_radius.is_finite() && d.outer_radius.is_finite() && d.inner_radius >= 0.0 && d.outer_radius > 0.0 && d.inner_radius < d.outer_radius
            }) && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Pie if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.as_ref().is_some_and(|p|{
                p.start_angle.is_finite() && p.end_angle.is_finite() && p.inner_radius.is_finite() && p.outer_radius.is_finite() && p.inner_radius >= 0.0 && p.outer_radius > 0.0 && p.inner_radius < p.outer_radius
            }) && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Heart if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.as_ref().is_some_and(|h|{
                h.indent.is_finite() && h.pointiness.is_finite()
            }) && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Cog if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.as_ref().is_some_and(|c|{
                c.number_of_teeth >= 3 && c.tooth_depth.is_finite() && c.tooth_width.is_finite() && c.inner_radius.is_finite() && c.outer_radius.is_finite() && c.inner_radius >= 0.0 && c.outer_radius > 0.0 && c.inner_radius < c.outer_radius
            }) && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Tear if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.as_ref().is_some_and(|t|{
                t.indent.is_finite() && t.pointiness.is_finite()
            }) && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Crescent if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.as_ref().is_some_and(|c|{
                c.indent.is_finite()
            }) && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Cloud if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.as_ref().is_some_and(|c|{
                c.indent.is_finite() && c.pointiness.is_finite()
            }) && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Segment if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.as_ref().is_some_and(|s|{
                s.indent.is_finite() && s.pointiness.is_finite()
            }) && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Cat if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.as_ref().is_some_and(|c|{
                c.indent.is_finite() && c.pointiness.is_finite()
            }) && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::SquareStar if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.as_ref().is_some_and(|s|{
                s.number_of_sides >= 3 && s.cutout.is_finite()
            }) && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::DoubleStar if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.as_ref().is_some_and(|d|{
                d.inner_radius.is_finite() && d.point_radius.is_finite() && d.number_of_points >= 3
            }) && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Diamond if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.as_ref().is_some_and(|d|{
                d.indent.is_finite()
            }) && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Trapezoid if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.as_ref().is_some_and(|t|{
                t.indent.is_finite()
            }) && self.polygon.is_none() && self.spiral.is_none() => Ok(()),
            ShapeKind::Polygon if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.as_ref().is_some_and(|p|{
                p.number_of_sides >= 3 && p.curve.is_finite()
            }) && self.spiral.is_none() => Ok(()),
            ShapeKind::Spiral if self.corner.is_none() && self.triangle.is_none() && self.star.is_none() && self.arrow.is_none() && self.callout_ellipse.is_none() && self.callout_rectangle.is_none() && self.donut.is_none() && self.pie.is_none() && self.heart.is_none() && self.cog.is_none() && self.tear.is_none() && self.crescent.is_none() && self.cloud.is_none() && self.segment.is_none() && self.cat.is_none() && self.square_star.is_none() && self.double_star.is_none() && self.diamond.is_none() && self.trapezoid.is_none() && self.polygon.is_none() && self.spiral.as_ref().is_some_and(|s|{
                s.turns.is_finite() && s.part_turn.is_finite() && s.segment_angle.is_finite() && s.divisions >= 1 && s.decay_per_turn.is_finite() && s.decay_per_segment.is_finite() && s.inner_radius.is_finite() && s.choke.is_finite() && s.cap_choke.is_finite() && s.bias.is_finite()
            }) => Ok(()),
            _ => Err(CommandError::invalid_params()),
        }
    }
}

pub fn query(session: &Session, id: ObjectId) -> Result<&ShapeRecipe, CommandError> {
    session.document.get_object(id).ok_or_else(CommandError::not_found)?.shape.as_ref().ok_or_else(CommandError::invalid_params)
}

pub fn shape_pct_radius(size:[f32;2], pct:f32)->[f32;4]{
    let w = size[0];
    let h = size[1];
    let r = pct.max(0.0).min(50.0);
    [w*r/100.0, h*r/100.0, w*r/100.0, h*r/100.0]
}
