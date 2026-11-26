use super::{Instance, Symbols, is_descendant, live_instance};
use crate::clipboard::ClipboardBundle;
use crate::workspace::ContextId;
use crate::{CommandError, Document, ObjectId};
use std::collections::{HashMap, HashSet};

pub(crate) fn capture(doc: &Document, bundle: &ClipboardBundle) -> Symbols {
    let members: HashSet<_> = bundle.objects.iter().map(|object| object.id).collect();
    let instances: Vec<_> = doc.symbols.instances.iter()
        .filter(|instance| members.contains(&instance.root) && live_instance(doc, instance))
        .cloned().collect();
    let definitions = doc.symbols.definitions.iter()
        .filter(|definition| instances.iter().any(|instance| instance.definition == definition.id))
        .cloned().collect();
    Symbols { definitions, instances, ..Symbols::default() }
}

pub(crate) fn external_guides(symbols: &Symbols) -> impl Iterator<Item = ObjectId> + '_ {
    symbols.definitions.iter().flat_map(|definition| {
        definition.bundle.objects.iter()
            .filter(|object| definition.external_path_nodes.contains(&object.id))
            .filter_map(|object| object.path_text_data().map(|path| path.path))
    })
}

pub(crate) fn can_link_under(doc: &Document, parent: Option<ObjectId>) -> bool {
    parent.is_none_or(|parent| !doc.symbols.instances.iter().any(|instance| {
        live_instance(doc, instance) && (instance.root == parent || is_descendant(doc, parent, instance.root))
    }))
}

pub(crate) fn has_external_guide(doc: &Document, object: ObjectId) -> bool {
    let Some((_, definition, node)) = super::object_link(doc, object) else { return false };
    doc.symbols.definitions.iter().any(|entry| entry.id == definition && entry.external_path_nodes.contains(&node))
}

fn remap_instance(instance: &Instance, mapping: &HashMap<ObjectId, ObjectId>, offset: [f32; 2]) -> Option<Instance> {
    let mut copy = instance.clone();
    copy.root = *mapping.get(&instance.root)?;
    copy.links.retain_mut(|(_, actual)| {
        let Some(&mapped) = mapping.get(actual) else { return false };
        *actual = mapped;
        true
    });
    for entry in &mut copy.curve_overrides {
        entry.relocate(1.0, [0.0, 0.0], offset);
    }
    Some(copy)
}

/// Same-document duplication can refer directly to the original definition.
pub(crate) fn duplicate(doc: &mut Document, mapping: &HashMap<ObjectId, ObjectId>, offset: [f32; 2]) {
    let copies: Vec<_> = doc.symbols.instances.iter()
        .filter(|instance| mapping.contains_key(&instance.root) && live_instance(doc, instance))
        .filter_map(|instance| remap_instance(instance, mapping, offset))
        .collect();
    doc.symbols.instances.extend(copies);
}

pub(crate) fn paste(
    doc: &mut Document,
    bundle: &ClipboardBundle,
    mapping: &HashMap<ObjectId, ObjectId>,
    destination: Option<ContextId>,
    offset: [f32; 2],
    factor: f32,
) -> Result<(), CommandError> {
    let same_context = factor == 1.0 && destination.is_some() && bundle.symbol_source == destination;
    let mut imported = bundle.symbols.clone();
    if factor != 1.0 {
        imported.convert_units(factor)?;
    }
    if same_context {
        let matching: HashSet<_> = imported.definitions.iter().filter(|source| {
            source.external_path_nodes.is_empty() && doc.symbols.definitions.iter().any(|current| {
                current.id == source.id
                    && serde_json::to_vec(current).ok().zip(serde_json::to_vec(source).ok())
                        .is_some_and(|(current, source)| current == source)
            })
        }).map(|definition| definition.id).collect();
        doc.symbols.instances.extend(imported.instances.iter()
            .filter(|instance| matching.contains(&instance.definition))
            .filter_map(|instance| remap_instance(instance, mapping, offset)));
        imported.instances.retain(|instance| !matching.contains(&instance.definition));
    }
    if !imported.instances.is_empty() {
        let mut source = Document::default();
        source.objects.clone_from(&bundle.objects);
        source.symbols = imported;
        super::page_import::import(&source, doc, mapping, &HashMap::new(), None)?;
    }
    Ok(())
}
