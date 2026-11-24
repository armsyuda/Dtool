use crate::shape::{ShapeKind, ShapeRecipe, SpiralStyle};
use crate::{CommandError, CommandResult, ObjectId, Session};
use serde_json::{Value, json};

const PREFIX: &str = "shape.recipe.";

#[derive(Clone, Copy)]
enum Control {
    Number { min: f64, max: f64, step: f64, integer: bool },
    Toggle,
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy)]
struct Field {
    name: &'static str,
    label: &'static str,
    control: Control,
}

const fn number(name: &'static str, label: &'static str, min: f64, max: f64, step: f64) -> Field {
    Field { name, label, control: Control::Number { min, max, step, integer: false } }
}

const fn integer(name: &'static str, label: &'static str, max: f64) -> Field {
    Field { name, label, control: Control::Number { min: 3.0, max, step: 1.0, integer: true } }
}

const fn toggle(name: &'static str, label: &'static str) -> Field {
    Field { name, label, control: Control::Toggle }
}

const INDENT: Field = number("indent", "shape.recipe.indent", 0.0, 100.0, 1.0);
const POINTINESS: Field = number("pointiness", "shape.recipe.pointiness", 0.0, 100.0, 1.0);
const INNER: Field = number("inner_radius", "shape.recipe.inner_radius", 0.0, 100.0, 1.0);
const OUTER: Field = number("outer_radius", "shape.recipe.outer_radius", 1.0, 100.0, 1.0);
const POINTER: [Field; 3] = [
    number("pointer_angle", "shape.recipe.pointer_angle", -180.0, 180.0, 1.0),
    number("pointer_length", "shape.recipe.pointer_length", 0.0, 100.0, 1.0),
    number("pointer_width", "shape.recipe.pointer_width", 0.0, 100.0, 1.0),
];
const SPIRAL_STYLES: &[&str] = &["linear", "decaying", "semi_circular", "counter_semi_circular", "fibonacci", "plotted"];

fn fields(recipe: &ShapeRecipe) -> Vec<Field> {
    use ShapeKind::*;
    let fields: &[Field] = match recipe.kind {
        Star => &[
            integer("number_of_points", "shape.star.points", 1024.0), INNER, OUTER,
            number("curve_left", "shape.star.curve_left", -100.0, 100.0, 1.0),
            number("curve_right", "shape.star.curve_right", -100.0, 100.0, 1.0),
        ],
        Arrow => &[
            number("head_length", "shape.arrow.head_length", 0.0, 100.0, 1.0),
            number("head_width", "shape.arrow.head_width", 0.0, 50.0, 1.0),
            number("shaft_width", "shape.arrow.shaft_width", 0.0, 50.0, 1.0),
            number("tail_width", "shape.arrow.tail_width", 0.0, 50.0, 1.0),
        ],
        CalloutRectangle => &POINTER,
        CalloutEllipse => &[
            number("radius_x", "shape.recipe.radius_x", 1.0, 50.0, 1.0),
            number("radius_y", "shape.recipe.radius_y", 1.0, 50.0, 1.0),
            POINTER[0], POINTER[1], POINTER[2],
        ],
        Donut => &[INNER, OUTER],
        Pie => &[
            number("start_angle", "shape.recipe.start_angle", -360.0, 360.0, 1.0),
            number("end_angle", "shape.recipe.end_angle", -360.0, 360.0, 1.0), INNER, OUTER,
        ],
        Cog => &[
            integer("number_of_teeth", "shape.recipe.teeth", 256.0),
            number("tooth_width", "shape.recipe.tooth_width", 0.0, 100.0, 1.0),
            number("inner_radius", "shape.recipe.inner_radius", 0.0, 50.0, 1.0),
            number("outer_radius", "shape.recipe.outer_radius", 1.0, 50.0, 1.0),
        ],
        Heart | Tear | Cloud | Segment | Cat => &[INDENT, POINTINESS],
        Crescent | Diamond => &[INDENT],
        Trapezoid => &[number("indent", "shape.recipe.indent", 0.0, 50.0, 1.0)],
        SquareStar => &[
            integer("number_of_sides", "shape.squarestar.sides", 1024.0),
            number("cutout", "shape.squarestar.cutout", 0.0, 100.0, 1.0),
        ],
        DoubleStar => &[
            integer("number_of_points", "shape.doublestar.points", 1024.0),
            INNER, number("point_radius", "shape.doublestar.point_radius", 0.0, 100.0, 1.0),
        ],
        Polygon => &[
            integer("number_of_sides", "ctx.polygon.sides", 1024.0),
            number("curve", "ctx.polygon.curve", -100.0, 100.0, 1.0),
            toggle("smooth_points", "ctx.polygon.smooth_points"),
        ],
        Spiral => return recipe.spiral.as_ref().map(|spiral| spiral_fields(spiral.style)).unwrap_or_default(),
        _ => &[],
    };
    fields.to_vec()
}

fn spiral_fields(style: SpiralStyle) -> Vec<Field> {
    let mut fields = vec![
        Field { name: "style", label: "ctx.spiral.style", control: Control::Choice(SPIRAL_STYLES) },
        number("turns", "ctx.spiral.turns", 0.1, 32.0, 0.1),
    ];
    let specific: &[Field] = match style {
        SpiralStyle::Linear => &[number("part_turn", "ctx.spiral.part_turn", 0.0, 1.0, 0.05)],
        SpiralStyle::Decaying => &[number("decay_per_turn", "ctx.spiral.decay", 0.05, 20.0, 0.05)],
        SpiralStyle::SemiCircular | SpiralStyle::CounterSemiCircular => &[
            number("segment_angle", "ctx.spiral.segment_angle", 15.0, 180.0, 1.0),
            Field { name: "divisions", label: "ctx.spiral.divisions", control: Control::Number { min: 1.0, max: 256.0, step: 1.0, integer: true } },
            number("decay_per_segment", "shape.recipe.decay_per_segment", 0.05, 20.0, 0.05),
        ],
        SpiralStyle::Fibonacci => &[INNER, number("choke", "ctx.spiral.choke", 0.0, 100.0, 1.0)],
        SpiralStyle::Plotted => &[
            number("bias", "ctx.spiral.bias", -100.0, 100.0, 1.0),
            number("cap_choke", "ctx.spiral.cap_choke", 0.0, 100.0, 1.0),
        ],
    };
    fields.extend_from_slice(specific);
    fields.extend([toggle("cusped", "ctx.spiral.cusped"), toggle("flip", "ctx.spiral.flip")]);
    fields
}

fn selected<'a>(session: &'a Session, tool: &str) -> Option<(ObjectId, &'static str, &'a ShapeRecipe, bool)> {
    let [id] = *session.selection.ids.as_slice() else { return None; };
    let object = session.document.get_object(id)?;
    let recipe = object.shape.as_ref()?;
    let key = super::super::shape::B19_RECIPE_KEYS.iter().find(|(_, kind)| *kind == recipe.kind)?.0;
    if !matches!(tool, "tool.move" | "tool.node" | "tool.shapes") && tool.strip_prefix("tool.shape.") != Some(key) {
        return None;
    }
    Some((id, key, recipe, object.locked))
}

pub(super) fn matches(id: &str) -> bool { id.starts_with(PREFIX) }

pub(super) fn options(session: &Session, tool: &str) -> Vec<Value> {
    let Some((id, key, recipe, locked)) = selected(session, tool) else { return Vec::new(); };
    let Ok(values) = serde_json::to_value(recipe) else { return Vec::new(); };
    fields(recipe).into_iter().filter_map(|field| {
        let value = values[key].get(field.name)?;
        let mut option = json!({
            "id": format!("{PREFIX}{}.{}", id.0, field.name),
            "key": field.label,
            "value": value,
            "state": if locked { "disabled" } else { "enabled" },
        });
        match field.control {
            Control::Number { min, max, step, .. } => {
                option["kind"] = json!("number");
                option["min"] = json!(min);
                option["max"] = json!(max);
                option["step"] = json!(step);
            }
            Control::Toggle => option["kind"] = json!("toggle"),
            Control::Choice(values) => {
                option["kind"] = json!("choice");
                option["values"] = json!(values);
            }
        }
        Some(option)
    }).collect()
}

pub(super) struct Edit {
    target: ObjectId,
    recipe: ShapeRecipe,
    value: Value,
}

pub(super) fn plan(session: &Session, tool: &str, id: &str, value: &Value) -> Result<Edit, CommandError> {
    let (target, key, recipe, locked) = selected(session, tool).ok_or_else(CommandError::invalid_params)?;
    let (object, name) = id.strip_prefix(PREFIX).and_then(|id| id.split_once('.')).ok_or_else(CommandError::invalid_params)?;
    if locked || object.parse::<u64>().ok() != Some(target.0) {
        return Err(CommandError::invalid_params());
    }
    let field = fields(recipe).into_iter().find(|field| field.name == name).ok_or_else(CommandError::invalid_params)?;
    let value = match field.control {
        Control::Number { min, max, integer, .. } => {
            let number = value.as_f64().filter(|number| (min..=max).contains(number)).ok_or_else(CommandError::invalid_params)?;
            if integer { json!(number.round() as u32) } else { json!(number) }
        }
        Control::Toggle if value.is_boolean() => value.clone(),
        Control::Choice(values) if value.as_str().is_some_and(|value| values.contains(&value)) => value.clone(),
        _ => return Err(CommandError::invalid_params()),
    };
    let mut whole = serde_json::to_value(recipe).map_err(|_| CommandError::invalid_params())?;
    if !whole[key].is_object() {
        return Err(CommandError::invalid_params());
    }
    whole[key][field.name] = value.clone();
    let recipe: ShapeRecipe = serde_json::from_value(whole).map_err(|_| CommandError::invalid_params())?;
    recipe.validate()?;
    Ok(Edit { target, recipe, value })
}

impl Edit {
    pub(super) fn apply(self, session: &mut Session, tool: &str, id: &str, record: bool) -> CommandResult {
        let reply = json!({ "tool": tool, "id": id, "value": self.value });
        if session.document.get_object(self.target).and_then(|object| object.shape.as_ref()) == Some(&self.recipe) {
            return Ok(reply);
        }
        if record {
            let mut document = session.document.clone();
            document.find_object_mut(self.target.0).ok_or_else(CommandError::not_found)?.shape = Some(self.recipe);
            let selection = session.selection.ids.clone();
            super::super::document_change::commit(session, "shape.params.set", document, selection, reply)
        } else {
            session.document.find_object_mut(self.target.0).ok_or_else(CommandError::not_found)?.shape = Some(self.recipe);
            Ok(reply)
        }
    }
}
