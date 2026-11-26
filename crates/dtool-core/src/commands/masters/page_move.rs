use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{InheritedMaster, TargetSurface, application_sources, source_ids};
use crate::{CommandError, Document, ObjectId, ObjectKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageMoveMasterPolicy {
    #[default]
    Keep,
    MoveContent,
    Reapply,
}

impl PageMoveMasterPolicy {
    pub(crate) fn is_keep(&self) -> bool {
        *self == Self::Keep
    }
}

pub(in crate::commands) fn update_after_move(before: &Document, doc: &mut Document) -> Result<(), CommandError> {
    let policy = doc.page_move_master_policy;
    if policy.is_keep() {
        return Ok(());
    }
    let old_indices: HashMap<_, _> = before.pages.iter().enumerate().map(|(index, page)| (page.id, index)).collect();
    let new_indices: HashMap<_, _> = doc.pages.iter().enumerate().map(|(index, page)| (page.id, index)).collect();
    let mut assignments = std::mem::take(&mut doc.master_pages.assignments);
    for assignment in &mut assignments {
        let page = assignment.page;
        let (Some(&old_index), Some(&new_index)) = (old_indices.get(&page), new_indices.get(&page)) else { continue };
        let previous = assignment.master;
        let root = doc.master_pages.spread_id(previous);
        if doc.master_pages.spread(root).is_none() {
            continue;
        }
        let old_side = application_sources(before, root, &[(old_index, page)], None)?[0];
        let new_side = application_sources(doc, root, &[(new_index, page)], None)?[0];
        if old_side != new_side && previous != new_side {
            retarget(before, doc, &mut assignment.inherited, TargetSurface::Page(page), new_side, policy)?;
        }
    }
    doc.master_pages.assignments = assignments;
    doc.validate_refs().map_err(CommandError::from)
}

#[derive(Clone, Copy, Default)]
struct FrameEdit {
    text: bool,
    picture: bool,
}

impl FrameEdit {
    fn record(self, fields: &mut BTreeSet<String>) {
        if self.text {
            fields.insert(super::STORY_TEXT_OVERRIDE.into());
        }
        if self.picture {
            fields.extend([
                "image_resource", "picture_frame.fit", "picture_frame.anchor", "picture_frame.manual_layout",
                "picture_frame.content.offset", "picture_frame.content.scale", "picture_frame.content.rotation",
            ].map(str::to_owned));
        }
    }
}

fn retarget(
    before: &Document, doc: &mut Document, binding: &mut InheritedMaster, surface: TargetSurface, master: ObjectId,
    policy: PageMoveMasterPolicy,
) -> Result<(), CommandError> {
    let move_content = policy == PageMoveMasterPolicy::MoveContent;
    let previous = binding.clone();
    let old_sources = source_ids(before, previous.master);
    let new_sources = source_ids(doc, master);
    let pairings = match_sources(before, doc, previous.master, master, &old_sources, &new_sources);
    let previous_links: HashMap<_, _> = previous.links.iter().copied().collect();
    let old_instances: HashSet<_> = previous.links.iter().map(|link| link.1).collect();
    let edited: HashMap<_, _> = previous.links.iter().filter_map(|&(source, local)| {
        let edit = frame_edit(before, &previous, surface, &previous_links, source, local);
        (edit.text || edit.picture).then_some((local, edit))
    }).collect();
    let field_edits: HashMap<_, _> = if move_content {
        let projection = surface.projection(before, &previous);
        previous.links.iter().filter_map(|&(source, local)| {
            let fields = object_edits(before, &previous, surface, projection, &previous_links, source, local);
            (!fields.is_empty()).then_some((local, fields))
        }).collect()
    } else {
        HashMap::new()
    };
    let mut keep: HashSet<_> = edited.keys().copied().collect();
    keep.extend(field_edits.keys().copied());
    // Local path text can reference an inherited path even when it belongs to another layer.
    loop {
        let len = keep.len();
        for object in &doc.objects {
            if move_content && keep.contains(&object.id)
                && let Some(parent) = object.parent_id.filter(|id| old_instances.contains(id))
            {
                keep.insert(parent);
            }
            if (!old_instances.contains(&object.id) || keep.contains(&object.id))
                && let Some(data) = object.path_text_data()
                && old_instances.contains(&data.path)
            {
                keep.insert(data.path);
            }
        }
        if keep.len() == len {
            break;
        }
    }
    let mut removed = InheritedMaster::new(previous.master);
    removed.links = previous.links.iter().copied().filter(|link| !keep.contains(&link.1)).collect();
    super::retire_content(doc, [&removed], false)?;

    binding.master = master;
    binding.links.clear();
    binding.matching_overrides.clear();
    let mut matched = Vec::new();
    for &(old, local) in &previous.links {
        let edit = edited.get(&local).copied().unwrap_or_default();
        if !keep.contains(&local) || (!move_content && !edit.text && !edit.picture) {
            continue;
        }
        // Without an unambiguous counterpart, keep the edited object as local content.
        let Some(&source) = pairings.get(&old) else { continue };
        if !move_content && doc.get_object(local).is_some_and(|object| object.path_text_data().is_some()) {
            continue;
        }
        binding.links.push((source, local));
        let mut fields = field_edits.get(&local).cloned().unwrap_or_default();
        edit.record(&mut fields);
        if move_content {
            fields.insert(super::LOCAL_POSITION_OVERRIDE.into());
        } else {
            matched.push((source, local, edit));
        }
        binding.matching_overrides.insert(source.0, fields);
    }
    let mapped: HashSet<_> = binding.links.iter().map(|link| link.0).collect();
    let suppressed = if move_content {
        suppressed_destination_objects(doc, &new_sources, &pairings, &previous_links)
    } else {
        HashSet::new()
    };
    let pending = new_sources.iter().copied().filter(|source| !mapped.contains(source) && !suppressed.contains(source))
        .collect::<Vec<_>>();
    super::add_instances(doc, binding, surface, &pending);
    for (source, local, edit) in matched {
        refresh_frame(doc, binding, surface, source, local, edit)?;
    }
    if let Some(group) = binding.group
        && let Some(root) = doc.get_object(master)
    {
        let name = root.name.clone();
        let old_name = before.get_object(previous.master).map(|object| object.name.as_str());
        if let Some(layer) = doc.find_object_mut(group.0).filter(|layer| Some(layer.name.as_str()) == old_name) {
            layer.name = name;
        }
    }
    super::inherit_structure(before, doc, binding, surface, &[], &[], &new_sources);
    Ok(())
}

fn suppressed_destination_objects(
    doc: &Document, sources: &[ObjectId], pairings: &HashMap<ObjectId, ObjectId>, old_links: &HashMap<ObjectId, ObjectId>,
) -> HashSet<ObjectId> {
    let mut omitted: HashSet<_> = pairings.iter().filter(|(old, _)| !old_links.contains_key(old))
        .map(|(_, new)| *new).collect();
    // Do not re-create a deleted layer through its template children or leave a path-text reference dangling.
    while !omitted.is_empty() {
        let len = omitted.len();
        for &source in sources {
            if let Some(object) = doc.get_object(source)
                && (object.parent_id.is_some_and(|parent| omitted.contains(&parent))
                    || object.path_text_data().is_some_and(|data| omitted.contains(&data.path)))
            {
                omitted.insert(source);
            }
        }
        if omitted.len() == len {
            break;
        }
    }
    omitted
}

fn object_edits(
    doc: &Document, binding: &InheritedMaster, surface: TargetSurface, projection: Option<super::Projection>,
    links: &HashMap<ObjectId, ObjectId>, source: ObjectId, local: ObjectId,
) -> BTreeSet<String> {
    let mut fields = binding.matching_overrides.get(&source.0).cloned().unwrap_or_default();
    let (Some(original), Some(instance)) = (doc.get_object(source), doc.get_object(local)) else { return fields };
    let mut expected = projection.and_then(|projection| projection.object(original).ok()).unwrap_or_else(|| {
        let mut object = original.clone();
        if let (Some(page), Some(master)) = (surface.origin(doc), doc.get_object(binding.master)) {
            for axis in 0..2 {
                object.pos[axis] += page[axis] - master.pos[axis];
            }
        }
        object
    });
    expected.id = local;
    expected.page_id = instance.page_id;
    expected.parent_id = original.parent_id.and_then(|parent| links.get(&parent).copied())
        .or(binding.group).or_else(|| surface.parent(doc));
    if let Some(story) = instance.story_id() {
        match &mut expected.kind {
            ObjectKind::ArtText { story: id } | ObjectKind::FrameText { story: id } => *id = story,
            ObjectKind::PathText(data) => {
                data.story = story;
                data.path = links.get(&data.path).copied().unwrap_or(data.path);
            }
            _ => {}
        }
    }
    if let (Ok(serde_json::Value::Object(expected)), Ok(serde_json::Value::Object(actual))) =
        (serde_json::to_value(expected), serde_json::to_value(instance))
    {
        for key in expected.keys().chain(actual.keys()) {
            let expected = expected.get(key).unwrap_or(&serde_json::Value::Null);
            let actual = actual.get(key).unwrap_or(&serde_json::Value::Null);
            if !super::same_projected_value(expected, actual) {
                fields.insert(key.clone());
            }
        }
        if fields.contains("kind") && original.curve().is_some() && instance.curve().is_some() {
            fields.insert("curve_geometry".into());
        }
    }
    if doc.pixel_layers.get(source) != doc.pixel_layers.get(local) {
        fields.insert("pixel_layers".into());
    }
    let mask = |id| doc.layer_masks.iter().find(|mask| mask.object == id).cloned().map(|mut mask| {
        mask.object = local;
        mask
    });
    if mask(source) != mask(local) {
        fields.insert("layer_mask".into());
    }
    if doc.placed_resource(source) != doc.placed_resource(local) {
        fields.insert("image_resource".into());
    }
    let picture_shape = |id| crate::commands::picture_frame::get_frame(doc, id).map(|frame| frame.shape);
    if picture_shape(source) != picture_shape(local) {
        fields.insert("picture_frame.shape".into());
    }
    // These records belong to the page instance. The normal master clone does not copy them.
    if crate::commands::layer_meta::meta(doc, local).is_some() {
        fields.insert("layer_meta".into());
    }
    if doc.text_wraps.entries.iter().any(|entry| entry.object == local) {
        fields.insert("text_wrap".into());
    }
    fields
}

fn match_sources(
    before: &Document, doc: &Document, old_root: ObjectId, new_root: ObjectId,
    old: &[ObjectId], new: &[ObjectId],
) -> HashMap<ObjectId, ObjectId> {
    fn index<'a>(doc: &'a Document, root: ObjectId, ids: &[ObjectId]) -> HashMap<Vec<(&'a str, &'static str)>, Option<ObjectId>> {
        let mut index = HashMap::new();
        for &id in ids {
            let mut key = Vec::new();
            let mut cursor = id;
            for _ in 0..=ids.len() {
                if cursor == root {
                    index.entry(key).and_modify(|value| *value = None).or_insert(Some(id));
                    break;
                }
                let Some(object) = doc.get_object(cursor) else { break };
                key.push((object.name.as_str(), object.kind.kind_name()));
                let Some(parent) = object.parent_id else { break };
                cursor = parent;
            }
        }
        index
    }
    let by_key = index(doc, new_root, new);
    index(before, old_root, old).into_iter().filter_map(|(key, old)| {
        Some((old?, by_key.get(&key).copied().flatten()?))
    }).collect()
}

fn frame_edit(
    doc: &Document, binding: &InheritedMaster, surface: TargetSurface, links: &HashMap<ObjectId, ObjectId>,
    source: ObjectId, local: ObjectId,
) -> FrameEdit {
    let (Some(original), Some(instance)) = (doc.get_object(source), doc.get_object(local)) else { return FrameEdit::default() };
    let text = if let (Some(original_story), Some(local_story_id)) = (original.story_id(), instance.story_id()) {
        let original_story = doc.stories.iter().find(|story| story.id == original_story);
        let local_story = doc.stories.iter().find(|story| story.id == local_story_id);
        let content_changed = original_story.zip(local_story).is_some_and(|(old, current)| {
            old.text != current.text || old.ranges != current.ranges || old.paras != current.paras || old.specials != current.specials
        });
        let inherited_flow = super::frame_flow(doc, source).into_iter().map(|frame| links.get(&frame).copied()).collect::<Option<Vec<_>>>();
        let local_flow = super::frame_flow(doc, local);
        content_changed || story_has_local_references(doc, local_story_id)
            || inherited_flow.is_some_and(|inherited| inherited != local_flow)
            || local_flow.iter().any(|frame| !links.values().any(|local| local == frame))
            || binding.matching_overrides.get(&source.0).is_some_and(|keys| keys.contains(super::STORY_TEXT_OVERRIDE))
    } else {
        false
    };
    let picture = if let Some(current) = crate::commands::picture_frame::get_frame(doc, local) {
        let mut expected = crate::commands::picture_frame::get_frame(doc, source);
        if let Some(frame) = &mut expected
            && let Some(projected) = surface.projection(doc, binding).and_then(|projection| projection.object(original).ok())
        {
            super::placement::project_picture_frame(doc, original, projected.size, frame);
        }
        doc.placed_resource(source) != doc.placed_resource(local) || expected.is_none_or(|expected| {
            expected.anchor != current.anchor || expected.fit != current.fit
                || !super::projected_values_match(&expected.content, &current.content)
                || !super::projected_values_match(&expected.manual_layout, &current.manual_layout)
        }) || binding.matching_overrides.get(&source.0)
            .is_some_and(|keys| keys.contains("image_resource") || keys.iter().any(|key| key.starts_with("picture_frame.")))
    } else {
        false
    };
    FrameEdit { text, picture }
}

fn story_has_local_references(doc: &Document, story: crate::StoryId) -> bool {
    let in_story = |position| doc.story_at_pos(position).is_some_and(|(id, _)| id == story);
    doc.text_links.iter().any(|link| link.story == story)
        || doc.anchors.iter().any(|anchor| in_story(anchor.story_pos))
        || doc.notes.notes.iter().any(|note| in_story(note.story_pos))
        || doc.index.marks.iter().any(|mark| in_story(mark.story_pos))
        || doc.hyperlinks.iter().any(|link| in_story(link.range.0))
}

fn refresh_frame(
    doc: &mut Document, binding: &InheritedMaster, surface: TargetSurface,
    source: ObjectId, local: ObjectId, edit: FrameEdit,
) -> Result<(), CommandError> {
    let source_object = doc.get_object(source).ok_or_else(CommandError::not_found)?;
    let instance = doc.get_object(local).ok_or_else(CommandError::not_found)?;
    let mut replacement = if let Some(projection) = surface.projection(doc, binding) {
        projection.object(source_object)?
    } else {
        source_object.clone()
    };
    replacement.id = local;
    replacement.page_id = instance.page_id;
    replacement.parent_id = source_object.parent_id
        .and_then(|parent| binding.links.iter().find(|link| link.0 == parent).map(|link| link.1))
        .or(binding.group).or_else(|| surface.parent(doc));
    let resource = doc.placed_resource(local);
    let picture = crate::commands::picture_frame::get_frame(doc, local);
    if edit.text && let Some(story) = instance.story_id() {
        match &mut replacement.kind {
            ObjectKind::ArtText { story: target } | ObjectKind::FrameText { story: target } => *target = story,
            _ => {}
        }
    }
    *doc.find_object_mut(local.0).ok_or_else(CommandError::not_found)? = replacement;
    super::copy_object_content(doc, source, local);
    if edit.picture && let Some(picture) = picture {
        if let Some(id) = resource {
            doc.place_image(local, id)?;
        } else {
            doc.unplace_image(local);
        }
        if let Some(mut updated) = crate::commands::picture_frame::get_frame(doc, local) {
            updated.image = resource;
            updated.fit = picture.fit;
            updated.anchor = picture.anchor;
            updated.content = picture.content;
            updated.manual_layout = picture.manual_layout;
            crate::commands::picture_frame::put_frame(doc, updated);
        }
    }
    Ok(())
}
