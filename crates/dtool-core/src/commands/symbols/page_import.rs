use std::collections::HashMap;
use std::path::Path;

use super::{Definition, live_instance};
use crate::document::PageId;
use crate::{CommandError, Document, ObjectId, ObjectKind};

pub(crate) fn import(
    source: &Document,
    destination: &mut Document,
    objects: &HashMap<ObjectId, ObjectId>,
    pages: &HashMap<PageId, PageId>,
    directory: &Path,
    factor: f32,
) -> Result<usize, CommandError> {
    let mut count = 0;
    for definition in &source.symbols.definitions {
        let instances: Vec<_> = source.symbols.instances.iter().filter_map(|instance| {
            (instance.definition == definition.id && live_instance(source, instance))
                .then(|| objects.get(&instance.root).map(|&root| (instance, root))).flatten()
        }).collect();
        let Some(&(_, first_root)) = instances.first() else { continue; };
        let origin = destination.get_object(first_root).ok_or_else(CommandError::not_found)?.pos;
        let shift = std::array::from_fn(|axis| origin[axis] - definition.origin[axis] * factor);
        let names: Vec<_> = destination.symbols.definitions.iter().map(|entry| entry.name.clone()).collect();
        let mut copy = definition.clone();
        copy.id = destination.symbols.definitions.iter().map(|entry| entry.id).max().unwrap_or(0)
            .checked_add(1).ok_or_else(CommandError::invalid_params)?;
        copy.name = crate::longdoc::helpers::unique_name(&names, &definition.name);
        copy.origin = origin;
        copy.size = copy.size.map(|value| value * factor);

        let (nodes, stories) = remap_definition(&mut copy, destination, objects, pages)?;
        crate::longdoc::page_import::resolve_bundle_links(&mut copy.bundle, directory);
        crate::longdoc::page_import::import_swatches(destination, &copy.bundle);
        crate::doc_setup::transform_object_dimensions(&mut copy.bundle.objects, factor, [factor; 2], |pos, size, axis| {
            (pos * factor + shift[axis], size * factor)
        })?;

        for (instance, root) in instances {
            let mut new = instance.clone();
            new.root = root;
            new.definition = copy.id;
            new.links.retain_mut(|(canonical, actual)| {
                let (Some(&node), Some(&object)) = (nodes.get(canonical), objects.get(actual)) else { return false; };
                (*canonical, *actual) = (node, object);
                true
            });
            for overrides in [&mut new.fill_overrides, &mut new.stroke_overrides, &mut new.corner_overrides, &mut new.constraint_overrides] {
                remap_nodes(overrides, &nodes);
            }
            new.text_overrides.retain_mut(|id| {
                let Some(&mapped) = stories.get(id) else { return false; };
                *id = mapped;
                true
            });
            let previous = source.get_object(instance.root).ok_or_else(CommandError::not_found)?.pos;
            let placed = destination.find_object_mut(root.0).ok_or_else(CommandError::not_found)?;
            let instance_shift = std::array::from_fn(|axis| placed.pos[axis] - previous[axis]);
            new.curve_overrides.retain_mut(|entry| {
                let Some(&node) = nodes.get(&entry.node) else { return false; };
                entry.node = node;
                entry.relocate(factor, shift, instance_shift);
                true
            });
            if placed.name == definition.name {
                placed.name.clone_from(&copy.name);
            }
            destination.symbols.instances.push(new);
        }
        destination.symbols.definitions.push(copy);
        count += 1;
    }
    Ok(count)
}

type Nodes = HashMap<ObjectId, ObjectId>;
type Stories = HashMap<crate::StoryId, crate::StoryId>;

fn remap_nodes(ids: &mut Vec<ObjectId>, nodes: &Nodes) {
    ids.retain_mut(|id| {
        let Some(&node) = nodes.get(id) else { return false; };
        *id = node;
        true
    });
}

fn remap_definition(
    definition: &mut Definition,
    destination: &mut Document,
    objects: &Nodes,
    pages: &HashMap<PageId, PageId>,
) -> Result<(Nodes, Stories), CommandError> {
    destination.raise_id_high_water();
    let bundle = &mut definition.bundle;
    let nodes: Nodes = bundle.objects.iter().map(|object| {
        let id = ObjectId(destination.next_object_id);
        destination.next_object_id += 1;
        (object.id, id)
    }).collect();
    let stories: Stories = bundle.stories.iter_mut().map(|story| {
        let original = story.id;
        story.id = destination.alloc_story_id();
        (original, story.id)
    }).collect();
    let resources: HashMap<_, _> = bundle.resources.iter_mut().map(|resource| {
        let original = resource.id;
        resource.id = destination.alloc_resource_id();
        (original, resource.id)
    }).collect();
    let node = |id: ObjectId| nodes.get(&id).copied().ok_or_else(CommandError::invalid_params);
    let story = |id: crate::StoryId| stories.get(&id).copied().ok_or_else(CommandError::invalid_params);
    for object in &mut bundle.objects {
        let original = object.id;
        object.id = node(original)?;
        object.parent_id = object.parent_id.and_then(|id| nodes.get(&id).copied());
        object.page_id = object.page_id.and_then(|id| pages.get(&id).copied());
        remap_nodes(&mut object.baked_warps, &nodes);
        match &mut object.kind {
            ObjectKind::ArtText { story: id } | ObjectKind::FrameText { story: id } => *id = story(*id)?,
            ObjectKind::PathText(path) => {
                path.story = story(path.story)?;
                path.path = if definition.external_path_nodes.contains(&original) {
                    *objects.get(&path.path).ok_or_else(|| CommandError::new("PAGE_OBJECT_REFERENCED"))?
                } else {
                    node(path.path)?
                };
            }
            ObjectKind::Table(table) => {
                for cell in &mut table.cells { cell.story = story(cell.story)?; }
            }
            ObjectKind::Curve(curve) => {
                curve.source_object = curve.source_object.and_then(|id| nodes.get(&ObjectId(id)).map(|node| node.0));
            }
            _ => {}
        }
    }
    remap_nodes(&mut definition.external_path_nodes, &nodes);
    remap_nodes(&mut bundle.roots, &nodes);
    for image in &mut bundle.placed_images {
        image.object_id = node(image.object_id)?;
        image.resource = *resources.get(&image.resource).ok_or_else(CommandError::invalid_params)?;
    }
    for mask in &mut bundle.layer_masks { mask.object = node(mask.object)?; }
    for layer in &mut bundle.pixel_layers { layer.object = node(layer.object)?; }
    for chain in &mut bundle.frame_chains {
        chain.story = story(chain.story)?;
        remap_nodes(&mut chain.frames, &nodes);
    }
    for binding in &mut bundle.swatch_bindings {
        binding.object = node(binding.object)?;
        binding.story = binding.story.map(story).transpose()?;
    }
    for link in &mut bundle.text_links { link.story = story(link.story)?; }
    Ok((nodes, stories))
}
