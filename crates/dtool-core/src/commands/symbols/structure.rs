use super::{Instance, live_instance};
use crate::{Document, Object, ObjectId};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
struct LayerSlot {
    node: ObjectId,
    parent: Option<ObjectId>,
}

fn children_of(objects: &[Object], root: ObjectId) -> HashSet<ObjectId> {
    let mut parents = HashSet::from([root]);
    loop {
        let len = parents.len();
        for object in objects {
            if object.parent_id.is_some_and(|parent| parents.contains(&parent)) {
                parents.insert(object.id);
            }
        }
        if len == parents.len() {
            break;
        }
    }
    parents.remove(&root);
    parents
}

fn linked_layout(
    objects: &[Object], instance: &Instance, members: &HashSet<ObjectId>,
) -> (Vec<LayerSlot>, bool) {
    let links: HashMap<_, _> = instance.links.iter().map(|&(node, actual)| (actual, node)).collect();
    let mut complete = true;
    let mut layout = Vec::new();
    for object in objects.iter().filter(|object| members.contains(&object.id)) {
        let Some(&node) = links.get(&object.id) else {
            complete = false;
            continue;
        };
        let parent = if object.parent_id == Some(instance.root) {
            None
        } else {
            let linked = object.parent_id.and_then(|parent| links.get(&parent)).copied();
            if linked.is_none() {
                complete = false;
            }
            linked
        };
        layout.push(LayerSlot { node, parent });
    }
    (layout, complete)
}

#[derive(Default)]
struct LayerChange {
    removed: HashSet<ObjectId>,
    layouts: Vec<Vec<LayerSlot>>,
}

fn target_map(doc: &mut Document, members: &[ObjectId], from: &Object, to: &Object) -> Result<(), crate::CommandError> {
    let root_center = crate::geometry::box_centre;
    let old = root_center(from);
    let new = root_center(to);
    let scale = std::array::from_fn(|i| if from.size[i] == 0.0 || to.size[i] == 0.0 { 1.0 } else { to.size[i] / from.size[i] });
    let inverse = crate::geometry::invert_matrix(&crate::geometry::object_matrix(from)).ok_or_else(crate::CommandError::invalid_params)?;
    let matrix = crate::geometry::object_matrix(to);
    let original: Vec<_> = members.iter().filter_map(|id| doc.get_object(*id).map(|object| (*id, root_center(object)))).collect();
    let args = crate::command::TransformSelectionBoxArgs {
        ids: None, origin: old, translate: [new[0] - old[0], new[1] - old[1]], scale: scale.map(f32::abs),
        rotation_deg: to.rotation_deg - from.rotation_deg, shear_deg: (to.shear_deg - from.shear_deg).clamp(-89.0, 89.0), ..Default::default()
    };
    crate::command::execute_transform_layer_box(doc, members, &args)?;
    for (id, center) in original {
        let center = crate::geometry::apply_matrix(&inverse, center);
        let center = std::array::from_fn(|i| new[i] + (center[i] - old[i]) * scale[i].abs());
        let center = crate::geometry::apply_matrix(&matrix, center);
        let object = doc.find_object_mut(id.0).ok_or_else(crate::CommandError::not_found)?;
        for i in 0..2 {
            object.pos[i] = center[i] - object.size[i].abs() * 0.5;
            if scale[i] < 0.0 {
                object.size[i] = -object.size[i];
            }
        }
        object.page_id = to.page_id;
    }
    Ok(())
}

pub(super) fn restore_external_guides(
    doc: &mut Document, definition: &super::Definition, added: &HashMap<ObjectId, ObjectId>,
) {
    for node in &definition.external_path_nodes {
        if let Some(actual) = added.get(node)
            && let Some(guide) = definition.bundle.objects.iter().find(|object| object.id == *node)
                .and_then(|object| object.path_text_data()).map(|path| path.path)
            && let Some(object) = doc.find_object_mut(actual.0)
            && let crate::ObjectKind::PathText(path) = &mut object.kind
        {
            path.path = guide;
        }
    }
}

fn insert_into_definition(doc: &mut Document, source: &Instance, added: &HashSet<ObjectId>) -> Result<(), crate::CommandError> {
    use crate::clipboard::{collect_bundle_members, paste_bundle_mapped};
    use crate::CommandError;
    let root = doc.get_object(source.root).ok_or_else(CommandError::not_found)?.clone();
    let definition_index = doc.symbols.definitions.iter().position(|entry| entry.id == source.definition)
        .ok_or_else(CommandError::invalid_params)?;
    let definition = &doc.symbols.definitions[definition_index];
    if definition.bundle.objects.iter().any(|object| added.contains(&object.id)) {
        return Err(CommandError::invalid_params());
    }
    let template = super::make_group(String::new(), definition.origin, definition.size, None, root.page_id);
    let ids: Vec<_> = doc.objects.iter().filter(|object| added.contains(&object.id)).map(|object| object.id).collect();
    let mut stage = doc.clone();
    target_map(&mut stage, &ids, &root, &template)?;
    let mut bundle = collect_bundle_members(&stage, added);
    let source_links: HashMap<_, _> = source.links.iter().map(|&(node, actual)| (actual, node))
        .chain(ids.iter().map(|id| (*id, *id))).collect();
    let mut external_path_nodes = Vec::new();
    for object in &mut bundle.objects {
        object.parent_id = object.parent_id.and_then(|parent| source_links.get(&parent).copied());
        object.baked_warps.retain_mut(|warp| {
            let Some(canonical) = source_links.get(warp) else { return false };
            *warp = *canonical;
            true
        });
        if let crate::ObjectKind::PathText(path) = &mut object.kind {
            if let Some(canonical) = source_links.get(&path.path) {
                path.path = *canonical;
            } else if doc.get_object(path.path).is_some() && !super::is_descendant(doc, path.path, source.root) {
                external_path_nodes.push(object.id);
            } else {
                return Err(CommandError::invalid_params());
            }
        }
    }
    let peers: Vec<_> = doc.symbols.instances.iter().filter(|entry| entry.definition == source.definition && entry.root != source.root
        && live_instance(doc, entry) && !entry.structure_override).cloned().collect();
    for peer in peers {
        let target = doc.get_object(peer.root).ok_or_else(CommandError::not_found)?.clone();
        let pasted = paste_bundle_mapped(doc, &bundle, Some(peer.root), [0.0, 0.0]);
        let new_ids: Vec<_> = ids.iter().filter_map(|id| pasted.get(id).copied()).collect();
        target_map(doc, &new_ids, &template, &target)?;
        let links: HashMap<_, _> = peer.links.iter().copied().chain(pasted.iter().map(|(key, val)| (*key, *val))).collect();
        for original in &bundle.objects {
            let Some(actual) = pasted.get(&original.id) else { continue };
            let object = doc.find_object_mut(actual.0).ok_or_else(CommandError::not_found)?;
            object.parent_id = original.parent_id.and_then(|parent| links.get(&parent).copied()).or(Some(peer.root));
            if let (crate::ObjectKind::PathText(path), Some(canonical)) = (&mut object.kind, original.path_text_data()) {
                path.path = if external_path_nodes.contains(&original.id) {
                    canonical.path
                } else {
                    *links.get(&canonical.path).ok_or_else(CommandError::invalid_params)?
                };
            }
            object.baked_warps = original.baked_warps.iter().filter_map(|id| links.get(id).copied()).collect();
        }
        if let Some(instance) = doc.symbols.instances.iter_mut().find(|entry| entry.root == peer.root) {
            instance.links.extend(ids.iter().filter_map(|id| pasted.get(id).map(|actual| (*id, *actual))));
        }
    }
    let definition = &mut doc.symbols.definitions[definition_index];
    definition.external_path_nodes.extend(external_path_nodes);
    let target = &mut definition.bundle;
    target.objects.extend(bundle.objects);
    for story in bundle.stories {
        if !target.stories.iter().any(|entry| entry.id == story.id) { target.stories.push(story); }
    }
    for resource in bundle.resources {
        if !target.resources.iter().any(|entry| entry.id == resource.id) { target.resources.push(resource); }
    }
    target.brush_sources.extend(bundle.brush_sources);
    target.placed_images.extend(bundle.placed_images);
    target.layer_masks.extend(bundle.layer_masks);
    target.pixel_layers.extend(bundle.pixel_layers);
    for chain in bundle.frame_chains {
        if let Some(existing) = target.frame_chains.iter_mut().find(|entry| entry.story == chain.story) {
            existing.frames.extend(chain.frames);
        } else {
            target.frame_chains.push(chain);
        }
    }
    for swatch in bundle.swatches {
        if !target.swatches.iter().any(|entry| entry.id == swatch.id) { target.swatches.push(swatch); }
    }
    target.swatch_bindings.extend(bundle.swatch_bindings);
    for link in bundle.text_links {
        if !target.text_links.iter().any(|entry| entry.story == link.story) { target.text_links.push(link); }
    }
    if let Some(instance) = doc.symbols.instances.iter_mut().find(|entry| entry.root == source.root) {
        instance.links.extend(ids.into_iter().map(|id| (id, id)));
    }
    Ok(())
}

pub(crate) fn sync(doc: &mut Document, before: &[Object], shared: bool) -> bool {
    let mut touched = Vec::new();
    let previous_ids: HashSet<_> = before.iter().map(|object| object.id).collect();
    // Newly placed or duplicated instances already have their own link state.
    for instance in doc.symbols.instances.iter().filter(|entry| previous_ids.contains(&entry.root) && live_instance(doc, entry)) {
        let old = children_of(before, instance.root);
        let mut current = doc.subtree_ids(instance.root);
        current.remove(&instance.root);
        let old_order: Vec<_> = before.iter().filter(|object| old.contains(&object.id)).map(|object| (object.id, object.parent_id)).collect();
        let new_order: Vec<_> = doc.objects().iter().filter(|object| current.contains(&object.id)).map(|object| (object.id, object.parent_id)).collect();
        if old_order != new_order {
            let existing: HashSet<_> = instance.links.iter().map(|(_, actual)| *actual).collect();
            let added = current.difference(&old).filter(|id| !existing.contains(id)).copied().collect::<HashSet<_>>();
            touched.push((instance.root, added));
        }
    }
    let mut divergent = HashSet::new();
    if shared {
        for (root, added) in &touched {
            let Some(source) = doc.symbols.instances.iter().find(|entry| entry.root == *root && !entry.structure_override).cloned() else { continue };
            if !added.is_empty() {
                let mut staged = doc.clone();
                if insert_into_definition(&mut staged, &source, added).is_ok() {
                    *doc = staged;
                } else {
                    divergent.insert(*root);
                }
            }
        }
    }
    let mut changes = BTreeMap::<u64, LayerChange>::new();
    let mut local_overrides = Vec::new();
    for instance in &doc.symbols.instances {
        if !touched.iter().any(|(root, _)| *root == instance.root) || !live_instance(doc, instance) {
            continue;
        }
        let old_members = children_of(before, instance.root);
        let members = doc.subtree_ids(instance.root);
        let (old, _) = linked_layout(before, instance, &old_members);
        let mut members = members;
        members.remove(&instance.root);
        let (current, complete) = linked_layout(doc.objects(), instance, &members);
        if old == current && old_members == members {
            continue;
        }
        if !shared || instance.structure_override || divergent.contains(&instance.root) {
            local_overrides.push(instance.root);
            continue;
        }
        let change = changes.entry(instance.definition).or_default();
        let known: HashSet<_> = current.iter().map(|slot| slot.node).collect();
        change.removed.extend(old.iter().map(|slot| slot.node).filter(|node| !known.contains(node)));
        if complete {
            change.layouts.push(current);
        }
    }

    let alive: HashSet<_> = doc.symbols.instances.iter().filter(|instance| live_instance(doc, instance))
        .map(|instance| instance.root).collect();
    doc.symbols.instances.retain(|instance| alive.contains(&instance.root));
    for instance in &mut doc.symbols.instances {
        if local_overrides.contains(&instance.root) {
            instance.structure_override = true;
        }
    }

    let mut kept_override = !local_overrides.is_empty() && shared;
    let mut removed_objects = HashSet::new();
    for (id, change) in changes {
        let Some(definition) = doc.symbols.definitions.iter_mut().find(|entry| entry.id == id) else { continue };
        let before: Vec<_> = definition.bundle.objects.iter().map(|object| {
            LayerSlot { node: object.id, parent: object.parent_id }
        }).collect();
        let mut layout = before.clone();
        for source in &change.layouts {
            let source_nodes: HashSet<_> = source.iter().map(|slot| slot.node).collect();
            let mut ordered = source.iter().copied().filter(|slot| !change.removed.contains(&slot.node));
            for slot in layout.iter_mut().filter(|slot| source_nodes.contains(&slot.node) && !change.removed.contains(&slot.node)) {
                if let Some(next) = ordered.next() {
                    *slot = next;
                }
            }
        }
        layout.retain(|slot| !change.removed.contains(&slot.node));
        apply_layout(&mut definition.bundle.objects, &layout);
        definition.bundle.remove_members(&change.removed);
        definition.external_path_nodes.retain(|node| !change.removed.contains(node));
        definition.bundle.roots = layout.iter().filter(|slot| slot.parent.is_none()).map(|slot| slot.node).collect();
        let retained_stories: HashSet<_> = definition.bundle.stories.iter().map(|story| story.id).collect();

        let instances: Vec<_> = doc.symbols.instances.iter().filter(|instance| instance.definition == id).cloned().collect();
        for instance in instances {
            if instance.structure_override {
                kept_override = true;
                continue;
            }
            let members = doc.subtree_ids(instance.root);
            let links: HashMap<_, _> = instance.links.iter().copied().filter(|(_, actual)| members.contains(actual)).collect();
            let doomed: Vec<_> = change.removed.iter().filter_map(|node| links.get(node)).copied().collect();
            let restored = layout.iter().filter_map(|slot| {
                let node = *links.get(&slot.node)?;
                let parent = match slot.parent {
                    Some(parent) => Some(*links.get(&parent)?),
                    None => Some(instance.root),
                };
                Some(LayerSlot { node, parent })
            }).collect::<Vec<_>>();
            apply_layout(&mut doc.objects, &restored);
            for root in doomed {
                removed_objects.extend(doc.subtree_ids(root));
            }
            if let Some(live) = doc.symbols.instances.iter_mut().find(|entry| entry.root == instance.root) {
                live.links.retain(|(node, _)| !change.removed.contains(node));
                live.fill_overrides.retain(|node| !change.removed.contains(node));
                live.stroke_overrides.retain(|node| !change.removed.contains(node));
                live.corner_overrides.retain(|node| !change.removed.contains(node));
                live.constraint_overrides.retain(|node| !change.removed.contains(node));
                live.curve_overrides.retain(|entry| !change.removed.contains(&entry.node));
                live.text_overrides.retain(|story| retained_stories.contains(story));
            }
        }
    }
    if !removed_objects.is_empty() {
        super::super::arrange::delete_objects(doc, &removed_objects);
    }
    kept_override
}

pub(super) fn restore(doc: &mut Document, root: ObjectId) -> Result<bool, crate::CommandError> {
    use crate::CommandError;
    let index = doc.symbols.instances.iter().position(|entry| entry.root == root && live_instance(doc, entry))
        .ok_or_else(CommandError::invalid_params)?;
    let instance = doc.symbols.instances[index].clone();
    let definition = doc.symbols.definitions.iter().find(|entry| entry.id == instance.definition)
        .ok_or_else(CommandError::invalid_params)?.clone();
    let group = doc.get_object(root).ok_or_else(CommandError::not_found)?.clone();
    let members = children_of(doc.objects(), root);
    let (local, complete) = linked_layout(doc.objects(), &instance, &members);
    let canonical = template_layout(&definition.bundle.objects);
    if !instance.structure_override && complete && canonical == local {
        return Ok(false);
    }

    let old_links: HashMap<_, _> = instance.links.iter().copied().collect();
    let existing: HashMap<_, _> = canonical.iter().filter_map(|slot| {
        let actual = *old_links.get(&slot.node)?;
        members.contains(&actual).then_some((slot.node, actual))
    }).collect();
    let retained: HashSet<_> = existing.values().copied().collect();
    let discarded: HashSet<_> = members.difference(&retained).copied().collect();
    let links = if existing.len() == canonical.len() {
        existing.clone()
    } else {
        crate::clipboard::paste_bundle_with_reuse(doc, &definition.bundle, Some(root), [0.0, 0.0], &existing)
    };
    let additions: Vec<_> = canonical.iter().filter(|slot| !existing.contains_key(&slot.node))
        .filter_map(|slot| links.get(&slot.node).copied()).collect();
    if !additions.is_empty() {
        let added = links.iter().filter(|(node, _)| !existing.contains_key(node)).map(|(&node, &actual)| (node, actual)).collect();
        restore_external_guides(doc, &definition, &added);
        let template = super::make_group(String::new(), definition.origin, definition.size, None, group.page_id);
        target_map(doc, &additions, &template, &group)?;
        let stories: HashSet<_> = additions.iter().filter_map(|id| doc.get_object(*id)?.story_id()).collect();
        for story in stories {
            crate::text_cmd::refresh_art_bounds(doc, story)?;
        }
    }

    let remapped: HashMap<_, _> = old_links.iter().filter_map(|(node, old)| {
        links.get(node).filter(|new| *new != old).map(|new| (*old, *new))
    }).collect();
    for template in &definition.bundle.objects {
        let Some(actual) = existing.get(&template.id) else { continue };
        if let Some(object) = doc.find_object_mut(actual.0) {
            if let crate::ObjectKind::PathText(path) = &mut object.kind {
                if let Some(replacement) = remapped.get(&path.path) {
                    path.path = *replacement;
                } else if discarded.contains(&path.path)
                    && let Some(original) = template.path_text_data()
                {
                    path.path = if definition.external_path_nodes.contains(&template.id) {
                        original.path
                    } else {
                        links.get(&original.path).copied().unwrap_or(original.path)
                    };
                }
            }
            for warp in &mut object.baked_warps {
                if let Some(replacement) = remapped.get(warp) {
                    *warp = *replacement;
                }
            }
            object.baked_warps.retain(|id| !discarded.contains(id));
        }
    }
    let order: Vec<_> = canonical.iter().map(|slot| {
        let node = *links.get(&slot.node).ok_or_else(CommandError::invalid_params)?;
        let parent = match slot.parent {
            Some(parent) => *links.get(&parent).ok_or_else(CommandError::invalid_params)?,
            None => root,
        };
        Ok(LayerSlot { node, parent: Some(parent) })
    }).collect::<Result<_, CommandError>>()?;
    apply_layout(&mut doc.objects, &order);
    if !discarded.is_empty() {
        super::super::arrange::delete_objects(doc, &discarded);
    }
    let keep = |node: &ObjectId| existing.contains_key(node);
    let keep_stories: HashSet<_> = definition.bundle.objects.iter().filter(|node| keep(&node.id))
        .flat_map(crate::clipboard::object_stories).collect();
    let instance = &mut doc.symbols.instances[index];
    instance.links = canonical.iter().map(|slot| (slot.node, links[&slot.node])).collect();
    instance.fill_overrides.retain(keep);
    instance.stroke_overrides.retain(keep);
    instance.corner_overrides.retain(keep);
    instance.constraint_overrides.retain(keep);
    instance.curve_overrides.retain(|entry| keep(&entry.node));
    instance.text_overrides.retain(|story| keep_stories.contains(story));
    instance.structure_override = false;
    doc.symbols.instances.retain(|entry| !discarded.contains(&entry.root));
    Ok(true)
}

fn template_layout(objects: &[Object]) -> Vec<LayerSlot> {
    let members: HashSet<_> = objects.iter().map(|object| object.id).collect();
    objects.iter().map(|object| LayerSlot {
        node: object.id,
        parent: object.parent_id.filter(|parent| members.contains(parent)),
    }).collect()
}

fn apply_layout(objects: &mut [Object], order: &[LayerSlot]) {
    let ids: HashMap<_, _> = order.iter().map(|slot| (slot.node, slot.parent)).collect();
    let mut indexes = Vec::new();
    let mut staged = HashMap::new();
    for (index, object) in objects.iter().enumerate() {
        if let Some(parent) = ids.get(&object.id) {
            let mut object = object.clone();
            object.parent_id = *parent;
            staged.insert(object.id, object);
            indexes.push(index);
        }
    }
    for (index, object) in indexes.into_iter().zip(order.iter().filter_map(|slot| staged.remove(&slot.node))) {
        objects[index] = object;
    }
}
