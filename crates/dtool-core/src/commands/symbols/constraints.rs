use super::{linked_node, live_instance, object_link, original_object};
use crate::constraints::{Constraint, Edit, target_parent};
use crate::{CommandError, Document, ObjectId, Session};
use std::collections::HashSet;

pub fn has_override(session: &Session, root: ObjectId, node: ObjectId) -> bool {
    session.document.symbols.instances.iter().any(|entry| entry.root == root && entry.constraint_overrides.contains(&node))
}

fn write(doc: &mut Document, id: ObjectId, value: Option<Constraint>) -> Result<bool, CommandError> {
    let object = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    let changed = object.constraint != value;
    object.constraint = value;
    Ok(changed)
}

pub(super) fn shared(doc: &mut Document, definition: u64, node: ObjectId, edit: &Edit) -> Result<(bool, bool), CommandError> {
    let current = original_object(doc, definition, node).ok_or_else(CommandError::not_found)?.constraint;
    let next = edit.apply(current);
    let mut changed = next != current;
    let mut kept_override = false;
    let targets: Vec<_> = doc.symbols.instances.iter()
        .filter(|instance| instance.definition == definition && live_instance(doc, instance))
        .filter_map(|instance| {
            let actual = linked_node(doc, instance, node)?;
            let object = doc.get_object(actual)?;
            doc.get_object(object.parent_id?).filter(|parent| parent.is_group())?;
            if instance.constraint_overrides.contains(&node) {
                kept_override = true;
                None
            } else {
                Some(actual)
            }
        }).collect();
    let original = doc.symbols.definitions.iter_mut().find(|entry| entry.id == definition)
        .and_then(|entry| entry.bundle.objects.iter_mut().find(|object| object.id == node))
        .ok_or_else(CommandError::not_found)?;
    original.constraint = next;
    for id in targets {
        changed |= write(doc, id, next)?;
    }
    Ok((changed, kept_override))
}

pub(crate) fn update(doc: &mut Document, ids: &[ObjectId], edit: &Edit, sync: bool) -> Result<(bool, bool), CommandError> {
    let mut addressed = HashSet::new();
    let mut changed = false;
    let mut kept_override = false;
    for &id in ids {
        let current = doc.get_object(id).ok_or_else(CommandError::not_found)?.constraint;
        let Some((root, definition, node)) = object_link(doc, id) else {
            changed |= write(doc, id, edit.apply(current))?;
            continue;
        };
        let index = doc.symbols.instances.iter().position(|entry| entry.root == root).ok_or_else(CommandError::not_found)?;
        let local = doc.symbols.instances[index].constraint_overrides.contains(&node);
        if sync && !local {
            if addressed.insert((definition, node)) {
                let (did_change, kept) = shared(doc, definition, node, edit)?;
                changed |= did_change;
                kept_override |= kept;
            }
        } else if write(doc, id, edit.apply(current))? {
            if !local {
                doc.symbols.instances[index].constraint_overrides.push(node);
            }
            changed = true;
            kept_override |= sync;
        }
    }
    Ok((changed, kept_override))
}

pub(super) fn revert(doc: &mut Document, definition: u64, index: usize, node: ObjectId) -> Result<bool, CommandError> {
    let instance = &doc.symbols.instances[index];
    if !instance.constraint_overrides.contains(&node) {
        return Ok(false);
    }
    let actual = linked_node(doc, instance, node).ok_or_else(CommandError::not_found)?;
    if target_parent(doc, actual).is_none() {
        return Err(CommandError::invalid_params());
    }
    let value = original_object(doc, definition, node).ok_or_else(CommandError::not_found)?.constraint;
    write(doc, actual, value)?;
    doc.symbols.instances[index].constraint_overrides.retain(|&id| id != node);
    Ok(true)
}
