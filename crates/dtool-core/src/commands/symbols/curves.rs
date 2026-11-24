use super::{Instance, linked_node, live_instance, object_link, original_object};
use crate::curve::{Contour, Curve};
use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::{CommandError, CommandResult, Document, Object, ObjectId, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct CurveFrame {
    pos: [f32; 2],
    size: [f32; 2],
    rotation: f32,
    shear: f32,
}

impl CurveFrame {
    fn of(object: &Object) -> Self {
        Self { pos: object.pos, size: object.size, rotation: object.rotation_deg, shear: object.shear_deg }
    }

    fn center(self) -> [f32; 2] {
        std::array::from_fn(|i| self.pos[i] + self.size[i].abs() * 0.5)
    }

    fn matrix(self) -> [f32; 6] {
        object_matrix(&Object {
            pos: self.pos, size: self.size, rotation_deg: self.rotation, shear_deg: self.shear,
            ..Object::default()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CurveOverride {
    pub(super) node: ObjectId,
    template: CurveFrame,
    reference: CurveFrame,
    edited: CurveFrame,
}

impl CurveOverride {
    pub(super) fn relocate(&mut self, factor: f32, template_shift: [f32; 2], instance_shift: [f32; 2]) {
        for (frame, shift) in [(&mut self.template, template_shift), (&mut self.reference, instance_shift), (&mut self.edited, instance_shift)] {
            frame.pos = std::array::from_fn(|axis| frame.pos[axis] * factor + shift[axis]);
            frame.size = frame.size.map(|value| value * factor);
        }
    }
}

fn project(from: CurveFrame, to: CurveFrame, target: CurveFrame) -> Result<(CurveFrame, [f32; 2]), CommandError> {
    let extent = |length: f32| {
        if length == 0.0 { super::super::curve_edit::MIN_CURVE_EXTENT } else { length.abs() }
    };
    let scale = std::array::from_fn(|i| extent(target.size[i]) / extent(from.size[i]));
    let inverse = invert_matrix(&from.matrix()).ok_or_else(CommandError::invalid_params)?;
    let untransformed = apply_matrix(&inverse, to.center());
    let origin = from.center();
    let center = target.center();
    let moved = std::array::from_fn(|i| center[i] + (untransformed[i] - origin[i]) * scale[i]);
    let moved = apply_matrix(&target.matrix(), moved);
    let size = std::array::from_fn(|i| (to.size[i].abs() * scale[i]).copysign(target.size[i]));
    let pos = std::array::from_fn(|i| moved[i] - size[i].abs() * 0.5);
    if pos.iter().chain(&size).any(|value| !value.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    Ok((CurveFrame { pos, size, ..target }, scale))
}

fn assign(object: &mut Object, frame: CurveFrame, curve: &Curve, scale: [f32; 2]) -> Result<(), CommandError> {
    let target = object.curve_mut().ok_or_else(CommandError::invalid_params)?;
    let rescale = |contour: &Contour| {
        let mut contour = contour.clone();
        let scale = scale.map(f64::from);
        let point = |point: [f64; 2]| [point[0] * scale[0], point[1] * scale[1]];
        for node in &mut contour.points {
            node.pos = point(node.pos);
            node.handle_in = node.handle_in.map(point);
            node.handle_out = node.handle_out.map(point);
            if let Some(corner) = &mut node.corner {
                corner.radius *= (scale[0] * scale[1]).sqrt();
            }
        }
        contour
    };
    target.contour = rescale(&curve.contour);
    target.holes = curve.holes.iter().map(rescale).collect();
    object.pos = frame.pos;
    object.size = frame.size;
    Ok(())
}

fn override_entry(instance: &Instance, node: ObjectId) -> Option<&CurveOverride> {
    instance.curve_overrides.iter().find(|entry| entry.node == node)
}

pub(crate) fn apply_edit(
    session: &mut Session, before: &Object, after: &Object, name: &str, mut reply: Value,
) -> Option<CommandResult> {
    let doc = &session.document;
    let (root, definition, node) = object_link(doc, before.id)?;
    let template = original_object(doc, definition, node).filter(|object| object.curve().is_some())?;
    let instance = doc.symbols.instances.iter().find(|entry| entry.root == root)?;
    let old_frame = CurveFrame::of(before);
    let new_frame = CurveFrame::of(after);
    let shape = after.curve()?;
    let changed = (|| {
        let mut updated = doc.clone();
        let template_frame = CurveFrame::of(template);
        let existing = override_entry(instance, node);
        let mut kept_override = false;
        if session.symbol_sync && existing.is_none() {
            let canonical = updated.symbols.definitions.iter_mut().find(|entry| entry.id == definition)
                .and_then(|entry| entry.bundle.objects.iter_mut().find(|object| object.id == node))
                .ok_or_else(CommandError::invalid_params)?;
            let (frame, scale) = project(old_frame, new_frame, template_frame)?;
            assign(canonical, frame, shape, scale)?;
            for other in doc.symbols.instances.iter().filter(|entry| entry.definition == definition && entry.root != root && live_instance(doc, entry)) {
                let Some(actual) = linked_node(doc, other, node) else { continue };
                if override_entry(other, node).is_some() {
                    kept_override = true;
                } else if let Some(object) = updated.find_object_mut(actual.0).filter(|object| object.curve().is_some()) {
                    let (frame, scale) = project(old_frame, new_frame, CurveFrame::of(object))?;
                    assign(object, frame, shape, scale)?;
                }
            }
        } else {
            let memory = if let Some(previous) = existing {
                let (reference, _) = project(previous.edited, previous.reference, old_frame)?;
                CurveOverride { reference, edited: new_frame, ..previous.clone() }
            } else {
                CurveOverride { node, template: template_frame, reference: old_frame, edited: new_frame }
            };
            if let Some(entry) = updated.symbols.instances.iter_mut().find(|entry| entry.root == root) {
                entry.curve_overrides.retain(|entry| entry.node != node);
                entry.curve_overrides.push(memory);
            }
        }
        *updated.find_object_mut(before.id.0).ok_or_else(CommandError::not_found)? = after.clone();
        Ok((updated, kept_override))
    })();
    Some(changed.and_then(|(updated, kept_override)| {
        reply["warnings"] = json!(super::warnings(kept_override));
        super::super::document_change::commit(session, name, updated, session.selection.ids.clone(), reply)
    }))
}

pub(super) fn revert(doc: &mut Document, definition: u64, root: ObjectId, node: ObjectId) -> Result<bool, CommandError> {
    let instance = doc.symbols.instances.iter().find(|entry| entry.root == root && live_instance(doc, entry))
        .ok_or_else(CommandError::invalid_params)?;
    let Some(memory) = override_entry(instance, node).cloned() else { return Ok(false) };
    let actual = linked_node(doc, instance, node).ok_or_else(CommandError::invalid_params)?;
    let template = original_object(doc, definition, node).ok_or_else(CommandError::invalid_params)?;
    let template_frame = CurveFrame::of(template);
    let shape = template.curve().ok_or_else(CommandError::invalid_params)?.clone();
    let target = doc.find_object_mut(actual.0).ok_or_else(CommandError::not_found)?;
    let (reference, _) = project(memory.edited, memory.reference, CurveFrame::of(target))?;
    let (frame, scale) = project(memory.template, template_frame, reference)?;
    assign(target, frame, &shape, scale)?;
    if let Some(entry) = doc.symbols.instances.iter_mut().find(|entry| entry.root == root) {
        entry.curve_overrides.retain(|entry| entry.node != node);
    }
    Ok(true)
}
