use crate::commands::appearance::{self, AppearanceSlots, PaintSlot};
use crate::paint::{BitmapPaint, Colour, GradientGeom, GradientKind, GradientStop, Paint, Transparency};
use crate::{Object, ObjectKind, Session};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;
use std::path::Path;

pub const TYPE: &str = "gradient.type";
pub const TARGET: &str = "gradient.target";

pub(super) fn apply_bitmap(session: &mut Session, tool: &str, path: &Path, record: bool) -> CommandResult {
    let (object, target) = Target::selected_bitmap_target(session, tool).ok_or_else(CommandError::invalid_params)?;
    let id = object.id;
    let decoded = crate::commands::image_place::decode_path(path)?;
    let image = crate::commands::image_place::embedded_bitmap(decoded)?;
    let [width, height] = image.pixel_size.map(f64::from);
    let [w, h] = object.size.map(|value| f64::from(value.abs()));
    let scale = (w / width).max(h / height).max(0.01);
    let matrix = [scale, 0.0, 0.0, scale, (w - width * scale) * 0.5, (h - height * scale) * 0.5];

    let mut document = session.document.clone();
    let resource = crate::path_brush::import_snapshot(&mut document, &image);
    let object = document.objects.iter_mut().find(|object| object.id == id).ok_or_else(CommandError::not_found)?;
    let paint = Paint::Bitmap(BitmapPaint { resource_id: resource.0.to_string(), matrix, seed: None, single_image: false });
    match target {
        Target::Fill => object.fill = paint,
        Target::Stroke => object.stroke.paint = paint,
        Target::Appearance(slot) => {
            let mut list = object.appearance();
            appearance::edit_list(&slot.paint_command(id, paint), &mut list)?;
            object.set_appearance(&list)?;
        }
        Target::Transparency => return Err(CommandError::invalid_params()),
    }
    let result = json!({ "tool": tool, "id": TYPE, "value": "bitmap", "resource": resource.0 });
    if !record {
        session.document = document;
        return Ok(result);
    }
    let selection = session.selection.ids.clone();
    let result = crate::commands::document_change::commit(session, "gradient.bitmap", document, selection, result)?;
    if let Target::Appearance(slot) = target {
        let _ = appearance::run(session, &slot.focus_command(id));
    }
    Ok(result)
}

pub fn parse_kind(value: &str) -> Option<GradientKind> {
    Some(match value {
        "linear" => GradientKind::Linear,
        "radial" => GradientKind::Radial,
        "elliptical" => GradientKind::Elliptical,
        "conical" => GradientKind::Conical,
        _ => return None,
    })
}

pub(super) fn kind_value(kind: GradientKind) -> &'static str {
    match kind {
        GradientKind::Linear => "linear",
        GradientKind::Radial => "radial",
        GradientKind::Elliptical => "elliptical",
        GradientKind::Conical => "conical",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Fill,
    Stroke,
    Appearance(PaintSlot),
    Transparency,
}

impl Target {
    pub fn selected<'a>(session: &'a Session, tool: &str) -> Option<(&'a Object, Self)> {
        let [id] = session.selection.ids.as_slice() else { return None };
        let object = session.document.get_object(*id)?;
        Some((object, Self::for_object(session, tool, object)?))
    }

    pub fn for_object(session: &Session, tool: &str, object: &Object) -> Option<Self> {
        if object.locked || session.quick_mask.active {
            return None;
        }
        if tool == "tool.transparency" {
            return Some(Self::Transparency);
        }
        if tool != "tool.gradient" && !tool.starts_with("tool.gradient.") {
            return None;
        }
        if let Some(focus) = session.appearance_focus.filter(|focus| focus.node == object.id) {
            return focus.active(session).then_some(Self::Appearance(focus.slot));
        }
        Some(if session.tool_options.choice(tool, TARGET).as_deref() == Some("stroke") {
            Self::Stroke
        } else {
            Self::Fill
        })
    }

    pub fn selected_bitmap_target<'a>(session: &'a Session, tool: &str) -> Option<(&'a Object, Self)> {
        let (object, target) = Self::selected(session, tool)?;
        if !appearance::supports_multiple(object) {
            return None;
        }
        let has_fill = !matches!(&object.kind, ObjectKind::Curve(curve) if !curve.contour.closed)
            && !object.shape.as_ref().is_some_and(|recipe| recipe.kind == crate::shape::ShapeKind::Spiral);
        let enabled = match target {
            Self::Fill => has_fill,
            Self::Stroke => object.stroke.width > 0.0,
            Self::Appearance(slot) => match slot.kind {
                appearance::Kind::Fill => has_fill && slot.paint(object).is_some(),
                appearance::Kind::Stroke => slot.stroke(object).is_some_and(|stroke| stroke.width > 0.0),
            },
            Self::Transparency => false,
        };
        enabled.then_some((object, target))
    }

    pub fn paint(self, object: &Object) -> Option<&Paint> {
        match self {
            Self::Fill => Some(&object.fill),
            Self::Stroke => Some(&object.stroke.paint),
            Self::Appearance(slot) => slot.paint(object),
            Self::Transparency => None,
        }
    }

    pub fn gradient(self, object: &Object) -> Option<(GradientKind, &[GradientStop], &GradientGeom)> {
        if self == Self::Transparency {
            let Transparency::Gradient { kind, stops, geom } = &object.transparency else { return None };
            return Some((*kind, stops, geom));
        }
        let Paint::Gradient { kind, stops, geom } = self.paint(object)? else { return None };
        Some((*kind, stops, geom))
    }

    pub fn command(self, object: &Object, kind: GradientKind, stops: Vec<GradientStop>, geom: GradientGeom) -> Command {
        if self == Self::Transparency {
            let transparency = Transparency::Gradient { kind, stops, geom };
            return Command::with_params("object.update", json!({"id": object.id.0, "transparency": transparency}));
        }
        self.paint_command(object, Paint::Gradient { kind, stops, geom })
    }

    pub(super) fn from_bitmap(self, object: &Object, kind: GradientKind) -> Command {
        let [w, h] = object.size.map(f64::from);
        let geom = GradientGeom { start: [0.0, h * 0.5], end: [w.abs().max(0.1), h * 0.5], ratio: 1.0, ..GradientGeom::default() };
        let stops = vec![
            GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) },
            GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) },
        ];
        self.command(object, kind, stops, geom)
    }

    pub fn paint_command(self, object: &Object, paint: Paint) -> Command {
        match self {
            Self::Appearance(slot) => slot.paint_command(object.id, paint),
            Self::Stroke => {
                let mut stroke = object.stroke.clone();
                stroke.paint = paint;
                Command::with_params("object.update", json!({"id": object.id.0, "stroke": stroke}))
            }
            Self::Fill => Command::with_params("object.update", json!({"id": object.id.0, "fill": paint})),
            Self::Transparency => unreachable!(),
        }
    }
}
