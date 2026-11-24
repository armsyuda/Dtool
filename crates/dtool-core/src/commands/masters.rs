use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::data_merge::layout::{clone_template, page_rect};
use crate::document::PageId;
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, ObjectKind, Session};

const CREATE: &str = "master.create";
const UPDATE: &str = "master.update";
const LIST: &str = "master.list";
const APPLY: &str = "master.apply";
const RESET: &str = "master.object.reset";
const DETACH: &str = "master.detach";
const PREFIX: &str = "마스터_";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MasterPages {
    pub roots: Vec<ObjectId>,
    pub assignments: Vec<Assignment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub page: PageId,
    pub master: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<ObjectId>,
    pub links: Vec<(ObjectId, ObjectId)>,
}

impl MasterPages {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty() && self.assignments.is_empty()
    }

    pub fn source_of(&self, object: ObjectId) -> Option<ObjectId> {
        self.assignments.iter().flat_map(|item| &item.links)
            .find_map(|&(source, instance)| (instance == object).then_some(source))
    }

    pub fn for_page(&self, page: PageId) -> Option<&Assignment> {
        self.assignments.iter().find(|item| item.page == page)
    }
}

pub fn master_name(root: &Object) -> &str {
    root.name.strip_prefix(PREFIX).unwrap_or(&root.name)
}

pub fn owns_source(doc: &Document, object: ObjectId) -> bool {
    if doc.master_pages.roots.is_empty() {
        return false;
    }
    doc.master_pages.roots.contains(&object)
        || crate::artboard::owner(doc, object).is_some_and(|board| doc.master_pages.roots.contains(&board.id))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), CREATE | UPDATE | LIST | APPLY | RESET | DETACH)
        .then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command).map(|has_document| {
        if !has_document {
            return Err(CommandError::no_document());
        }
        match command.name.as_str() {
            CREATE => create(session, command),
            UPDATE => update(session, command),
            LIST => Ok(list(&session.document)),
            APPLY => apply(session, command),
            RESET => reset(session, command),
            _ => detach(session, command),
        }
    })
}

fn list(doc: &Document) -> Value {
    let masters: Vec<_> = doc.master_pages.roots.iter().filter_map(|id| doc.get_object(*id)).map(|root| {
        json!({"id": root.id, "name": master_name(root), "size": root.size, "origin": root.pos})
    }).collect();
    json!({"masters": masters, "assignments": doc.master_pages.assignments})
}

fn read_id(params: &Value, key: &str) -> Result<u64, CommandError> {
    crate::text_cmd::param_u64(params, key)
}

fn options(doc: &Document, params: &Value, id: Option<ObjectId>) -> Result<(String, [f32; 2]), CommandError> {
    let name = params["name"].as_str().ok_or_else(CommandError::invalid_params)?.trim();
    if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err(CommandError::invalid_params());
    }
    if doc.master_pages.roots.iter().filter(|root| Some(**root) != id).any(|root| {
        doc.get_object(*root).is_some_and(|object| master_name(object) == name)
    }) {
        return Err(CommandError::invalid_params());
    }
    let size = match params.get("size") {
        Some(size) => serde_json::from_value(size.clone()).map_err(|_| CommandError::invalid_params())?,
        None => crate::data_merge::layout::page_size(doc),
    };
    if !size.iter().all(|value| value.is_finite() && *value > 0.0) {
        return Err(CommandError::invalid_params());
    }
    Ok((format!("{PREFIX}{name}"), size))
}

fn create(session: &mut Session, command: &Command) -> CommandResult {
    let (name, size) = options(&session.document, &command.params, None)?;
    let mut doc = session.document.clone();
    let id = doc.create_object(Object {
        name, size, kind: ObjectKind::Artboard,
        blend_mode: crate::blend::BlendMode::PassThrough, ..Object::default()
    });
    doc.master_pages.roots.push(id);
    arrange_masters(&mut doc);
    super::document_change::commit(session, CREATE, doc, vec![id], json!({"id": id}))
}

fn update(session: &mut Session, command: &Command) -> CommandResult {
    let id = ObjectId(read_id(&command.params, "id")?);
    let mut doc = session.document.clone();
    if !doc.master_pages.roots.contains(&id) {
        return Err(CommandError::not_found());
    }
    let (name, size) = options(&doc, &command.params, Some(id))?;
    let root = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    if root.name == name && root.size == size {
        return Ok(json!({"id": id, "changes": [], "doc_version": session.generation}));
    }
    let before = crate::geometry::object_world_bounds(root);
    root.name.clone_from(&name);
    root.size = size;
    let after = crate::geometry::object_world_bounds(root);
    let resized_fill = super::layer_kinds::follow_surface_resize(&mut doc, Some(id), before, after);
    let groups: Vec<_> = doc.master_pages.assignments.iter()
        .filter(|assignment| assignment.master == id)
        .filter_map(|assignment| assignment.group)
        .collect();
    for group in groups {
        if let Some(folder) = doc.find_object_mut(group.0) {
            folder.name.clone_from(&name);
        }
    }
    arrange_masters(&mut doc);
    if resized_fill {
        inherit_changes(&session.document, &mut doc, command);
    }
    super::document_change::commit(session, UPDATE, doc, session.selection.ids.clone(), json!({"id": id}))
}

pub(super) fn master_row_height(doc: &Document) -> Option<f32> {
    doc.master_pages.roots.iter().filter_map(|id| doc.get_object(*id))
        .map(|root| root.size[1]).reduce(f32::max)
}

pub(crate) fn arrange_masters(doc: &mut Document) {
    let Some(height) = master_row_height(doc) else { return };
    let gap = crate::artboard::edit::default_spacing(doc);
    let page_tops = (0..doc.pages.len()).map(|index| page_rect(doc, index)[1]);
    let board_tops = doc.objects.iter().filter(|object| {
        object.parent_id.is_none() && crate::artboard::is_artboard(object)
            && !doc.master_pages.roots.contains(&object.id)
    }).map(|board| crate::geometry::object_world_bounds(board)[1]);
    let top = page_tops.chain(board_tops).filter(|y| y.is_finite()).reduce(f32::min).unwrap_or(0.0);
    let mut x = 0.0;
    let y = top - height - gap;
    for id in doc.master_pages.roots.clone() {
        let Some(root) = doc.get_object(id) else { continue };
        let delta = [x - root.pos[0], y - root.pos[1]];
        x += root.size[0] + gap;
        let members = doc.subtree_ids(id);
        for object in doc.objects.iter_mut().filter(|object| members.contains(&object.id)) {
            object.pos[0] += delta[0];
            object.pos[1] += delta[1];
        }
    }
}

fn apply(session: &mut Session, command: &Command) -> CommandResult {
    let id = ObjectId(read_id(&command.params, "master_id")?);
    let targets = apply_targets(&session.document, &command.params)?;
    let flag = |key| command.params.get(key).map(|value| value.as_bool().ok_or_else(CommandError::invalid_params))
        .transpose().map(|value| value.unwrap_or(false));
    let replace = flag("replace")?;
    let keep_previous = flag("keep_previous")?;
    if keep_previous && !replace {
        return Err(CommandError::invalid_params());
    }
    let mut doc = session.document.clone();
    if doc.get_object(id).is_none() || !doc.master_pages.roots.contains(&id) {
        return Err(CommandError::not_found());
    }
    let page_ids: Vec<_> = targets.iter().map(|&(_, page)| page).collect();
    let mut extra = json!({"master_id": id, "page_id": page_ids[0], "page_ids": page_ids});
    let mut changed = Vec::new();
    for (index, page) in targets {
        if doc.master_pages.for_page(page).is_some_and(|current| current.master == id) {
            continue;
        }
        let previous = apply_on_page(&mut doc, id, page, index, replace, keep_previous)?;
        if page_ids.len() == 1 && let Some(previous) = previous {
            extra["previous_master_id"] = json!(previous);
            extra["kept_previous"] = json!(keep_previous);
        }
        changed.push(page);
    }
    extra["changed_page_ids"] = json!(changed);
    if changed.is_empty() {
        extra["changes"] = json!([]);
        extra["doc_version"] = json!(session.generation);
        return Ok(extra);
    }
    doc.validate_refs().map_err(|_| CommandError::new("PAGE_OBJECT_REFERENCED"))?;
    let selection = session.selection.ids.iter().copied().filter(|id| doc.get_object(*id).is_some()).collect();
    super::document_change::commit(session, APPLY, doc, selection, extra)
}

fn apply_targets(doc: &Document, params: &Value) -> Result<Vec<(usize, PageId)>, CommandError> {
    let ids = match (params.get("page_id"), params.get("page_ids"), params.get("pages")) {
        (Some(_), None, None) => vec![PageId(read_id(params, "page_id")?)],
        (None, Some(ids), None) => serde_json::from_value(ids.clone()).map_err(|_| CommandError::invalid_params())?,
        (None, None, Some(pages)) => {
            let pages = pages.as_str().ok_or_else(CommandError::invalid_params)?;
            crate::longdoc::helpers::parse_page_ranges(pages, doc.pages.len())?
                .into_iter().map(|number| doc.pages[number - 1].id).collect()
        }
        _ => return Err(CommandError::invalid_params()),
    };
    let unique: HashSet<_> = ids.iter().copied().collect();
    if ids.is_empty() || unique.len() != ids.len() {
        return Err(CommandError::invalid_params());
    }
    let targets: Vec<_> = doc.pages.iter().enumerate()
        .filter_map(|(index, page)| unique.contains(&page.id).then_some((index, page.id))).collect();
    if targets.len() != ids.len() {
        return Err(CommandError::not_found());
    }
    Ok(targets)
}

fn apply_on_page(
    doc: &mut Document, id: ObjectId, page_id: PageId, page_index: usize, replace: bool, keep_previous: bool,
) -> Result<Option<ObjectId>, CommandError> {
    let mut insertion = doc.objects.len();
    let mut previous = None;
    let old_slot = doc.master_pages.assignments.iter().position(|assignment| assignment.page == page_id);
    if let Some(slot) = old_slot {
        let current = doc.master_pages.assignments[slot].clone();
        if !replace {
            return Err(CommandError::new("MASTER_ALREADY_APPLIED"));
        }
        previous = Some(current.master);
        insertion = retire_assignment(doc, &current, keep_previous)?;
    }
    let before_apply = doc.objects.len();
    apply_to_page(doc, id, page_id, page_index);
    if let Some(slot) = old_slot {
        doc.master_pages.assignments[slot..].rotate_right(1);
    }
    let added = doc.objects.len() - before_apply;
    doc.objects[insertion..].rotate_right(added);
    Ok(previous)
}

fn retire_assignment(doc: &mut Document, current: &Assignment, keep: bool) -> Result<usize, CommandError> {
    let owned: HashSet<_> = current.links.iter().map(|&(_, instance)| instance)
        .chain(current.group).collect();
    let at = doc.objects.iter().position(|object| owned.contains(&object.id)).unwrap_or(doc.objects.len());
    doc.master_pages.assignments.retain(|assignment| assignment.page != current.page);
    if keep {
        return Ok(at);
    }
    if doc.objects.iter().any(|object| !owned.contains(&object.id)
        && object.path_text_data().is_some_and(|data| owned.contains(&data.path)))
    {
        return Err(CommandError::new("PAGE_OBJECT_REFERENCED"));
    }
    let parent_by_id: HashMap<_, _> = doc.objects.iter().filter(|object| owned.contains(&object.id))
        .map(|object| (object.id, object.parent_id)).collect();
    for object in &mut doc.objects {
        if owned.contains(&object.id) {
            continue;
        }
        while let Some(parent) = object.parent_id.and_then(|parent| parent_by_id.get(&parent)) {
            object.parent_id = *parent;
        }
    }
    super::pages::remove_layout_objects(doc, &owned);
    Ok(at)
}

pub(super) fn apply_to_page(doc: &mut Document, master: ObjectId, page: PageId, index: usize) {
    let template = source_ids(doc, master);
    let mut assignment = Assignment { page, master, group: None, links: Vec::new() };
    create_assignment_group(doc, &mut assignment);
    add_instances(doc, &mut assignment, index, &template);
    doc.master_pages.assignments.push(assignment);
}

pub(crate) fn materialize_groups(doc: &mut Document) {
    if doc.master_pages.assignments.iter().all(|assignment| assignment.group.is_some()) {
        return;
    }
    let mut assignments = std::mem::take(&mut doc.master_pages.assignments);
    for assignment in assignments.iter_mut().filter(|assignment| assignment.group.is_none()) {
        create_assignment_group(doc, assignment);
    }
    doc.master_pages.assignments = assignments;
}

fn create_assignment_group(doc: &mut Document, assignment: &mut Assignment) {
    let Some(master) = doc.get_object(assignment.master) else { return };
    let name = master.name.clone();
    let Some(index) = doc.pages.iter().position(|page| page.id == assignment.page) else { return };
    let [x, y, _, _] = page_rect(doc, index);
    let parent = doc.linked_artboard(assignment.page).map(|board| board.id);
    let linked: HashSet<_> = assignment.links.iter().map(|&(_, id)| id).collect();
    let roots: HashSet<_> = doc.objects.iter().filter(|object| {
        linked.contains(&object.id) && object.parent_id == parent
    }).map(|object| object.id).collect();
    let at = doc.objects.iter().position(|object| roots.contains(&object.id)).unwrap_or(doc.objects.len());
    let group = doc.create_object(Object {
        name,
        pos: [x, y],
        kind: ObjectKind::Group,
        layer_folder: true,
        parent_id: parent,
        page_id: Some(assignment.page),
        blend_mode: crate::blend::BlendMode::PassThrough,
        ..Object::default()
    });
    doc.objects[at..].rotate_right(1);
    for object in doc.objects.iter_mut().filter(|object| roots.contains(&object.id)) {
        object.parent_id = Some(group);
    }
    assignment.group = Some(group);
}

fn source_ids(doc: &Document, id: ObjectId) -> Vec<ObjectId> {
    let members = doc.subtree_ids(id);
    doc.objects.iter().filter(|object| object.id != id && members.contains(&object.id))
        .map(|object| object.id).collect()
}

fn add_instances(doc: &mut Document, assignment: &mut Assignment, index: usize, template: &[ObjectId]) {
    if template.is_empty() {
        return;
    }
    let Some(root) = doc.get_object(assignment.master) else { return };
    let [x, y, _, _] = page_rect(doc, index);
    let offset = [x - root.pos[0], y - root.pos[1]];
    let parent = assignment.group.or_else(|| doc.linked_artboard(assignment.page).map(|board| board.id));
    doc.raise_id_high_water();
    let links = clone_template(doc, template, offset);
    let mapped: HashMap<_, _> = assignment.links.iter().chain(&links).copied().collect();
    for &(source, instance) in &links {
        let original_parent = doc.get_object(source).and_then(|object| object.parent_id);
        let original_path = doc.get_object(source).and_then(|object| object.path_text_data()).map(|data| data.path);
        let inherited_parent = original_parent.and_then(|id| mapped.get(&id).copied())
            .filter(|id| doc.get_object(*id).is_some()).or(parent);
        let path = original_path.and_then(|id| mapped.get(&id).copied())
            .filter(|id| doc.get_object(*id).is_some());
        copy_object_content(doc, source, instance);
        if let Some(object) = doc.find_object_mut(instance.0) {
            object.page_id = Some(assignment.page);
            object.parent_id = inherited_parent;
            if let (ObjectKind::PathText(data), Some(path)) = (&mut object.kind, path) {
                data.path = path;
            }
        }
    }
    connect_cloned_frames(doc, assignment, &links);
    assignment.links.extend(links);
}

fn connect_cloned_frames(doc: &mut Document, assignment: &Assignment, links: &[(ObjectId, ObjectId)]) {
    let new_frames: HashMap<_, _> = links.iter().copied().collect();
    let existing: HashMap<_, _> = assignment.links.iter().copied().collect();
    let originals = doc.frame_chains.clone();
    let mut abandoned = HashSet::new();
    for chain in originals {
        if !chain.frames.iter().any(|frame| new_frames.contains_key(frame)) {
            continue;
        }
        let previous: Vec<_> = chain.frames.iter().filter_map(|frame| existing.get(frame).copied()).collect();
        let inherited_story = previous.first().and_then(|&id| doc.get_object(id)?.story_id())
            .filter(|&story| crate::frame::flow_frames(doc, story) == previous);
        let frames: Vec<_> = chain.frames.iter().filter_map(|frame| {
            new_frames.get(frame).copied().or_else(|| inherited_story.and_then(|_| existing.get(frame).copied()))
        }).collect();
        if frames.len() < 2 {
            continue;
        }
        let Some(story) = inherited_story.or_else(|| doc.get_object(frames[0])?.story_id()) else { continue };
        for &id in &frames {
            if let Some(object) = doc.find_object_mut(id.0)
                && let ObjectKind::FrameText { story: current } = &mut object.kind
                && *current != story
            {
                abandoned.insert(*current);
                *current = story;
            }
        }
        if let Some(existing) = doc.frame_chains.iter_mut().find(|chain| chain.story == story) {
            existing.frames = frames;
        } else {
            doc.frame_chains.push(crate::frame::FrameChain { story, frames });
        }
    }
    // These stories belonged to newly cloned frames and have no document references.
    doc.stories.retain(|story| !abandoned.contains(&story.id));
}

fn detach(session: &mut Session, command: &Command) -> CommandResult {
    let page = PageId(read_id(&command.params, "page_id")?);
    let mut doc = session.document.clone();
    if doc.master_pages.for_page(page).is_none() {
        return Err(CommandError::not_found());
    }
    doc.master_pages.assignments.retain(|assignment| assignment.page != page);
    super::document_change::commit(session, DETACH, doc, session.selection.ids.clone(), json!({"page_id": page}))
}

fn reset(session: &mut Session, command: &Command) -> CommandResult {
    let id = ObjectId(read_id(&command.params, "id")?);
    let mut doc = session.document.clone();
    let assignment = doc.master_pages.assignments.iter().find(|item| item.links.iter().any(|link| link.1 == id))
        .ok_or_else(CommandError::not_found)?;
    let source_id = doc.master_pages.source_of(id).ok_or_else(CommandError::not_found)?;
    let mut source = doc.get_object(source_id).cloned().ok_or_else(CommandError::not_found)?;
    let current = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let old_story = current.story_id();
    let preserve_flow = if let (
        ObjectKind::FrameText { story: original },
        ObjectKind::FrameText { story: local },
    ) = (&mut source.kind, &current.kind) {
        let original_flow = crate::frame::flow_frames(&doc, *original);
        let local_flow = crate::frame::flow_frames(&doc, *local);
        let mapped = original_flow.iter().map(|frame| {
            assignment.links.iter().find_map(|&(source, instance)| (source == *frame).then_some(instance))
        });
        let differs = !mapped.eq(local_flow.iter().copied().map(Some));
        if differs {
            // A locally rethreaded story can contain text owned by other pages or frames.
            *original = *local;
        }
        differs
    } else {
        false
    };
    let source_story = if preserve_flow {
        None
    } else {
        source.story_id().and_then(|story| doc.stories.iter().find(|item| item.id == story)).cloned()
    };
    let resource = doc.placed_resource(source_id);
    source.id = id;
    source.pos = current.pos;
    source.parent_id = current.parent_id;
    source.page_id = current.page_id;
    if let ObjectKind::PathText(data) = &mut source.kind {
        data.path = assignment.links.iter().find(|link| link.0 == data.path)
            .map_or(data.path, |link| link.1);
    }
    if let Some(mut story) = source_story {
        story.id = old_story.unwrap_or_else(|| doc.alloc_story_id());
        match &mut source.kind {
            ObjectKind::ArtText { story: id } | ObjectKind::FrameText { story: id } => *id = story.id,
            ObjectKind::PathText(data) => data.story = story.id,
            _ => {}
        }
        let position = doc.story_pos(story.id, 0);
        if let Some(current) = doc.stories.iter_mut().find(|item| item.id == story.id) {
            let old_len = current.text.chars().count();
            let new_len = story.text.chars().count();
            *current = story;
            crate::longdoc::marks::shift_after_story_rebuild(&mut doc, position, old_len, new_len);
        } else {
            doc.stories.push(story);
        }
    }
    *doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)? = source;
    copy_object_content(&mut doc, source_id, id);
    if let Some(story) = old_story {
        crate::frame::drop_story_if_unowned(&mut doc, story);
    }
    if let Some(resource) = resource {
        doc.place_image(id, resource)?;
    } else {
        doc.unplace_image(id);
    }
    lock_positions(&mut doc);
    crate::longdoc::xref::refresh_xrefs(&mut doc);
    super::document_change::commit(session, RESET, doc, session.selection.ids.clone(), json!({"id": id, "source_id": source_id}))
}

fn copy_object_content(doc: &mut Document, source: ObjectId, instance: ObjectId) {
    if let Some(pixels) = doc.pixel_layers.get(source).cloned() {
        doc.pixel_layers.insert(instance, pixels);
    } else {
        doc.pixel_layers.remove(instance);
    }
    let mask = doc.layer_masks.iter().find(|mask| mask.object == source).cloned();
    doc.layer_masks.retain(|mask| mask.object != instance);
    if let Some(mut mask) = mask {
        mask.object = instance;
        doc.layer_masks.push(mask);
    }

    if let Some(mut frame) = super::picture_frame::get_frame(doc, source) {
        frame.object = instance;
        super::picture_frame::put_frame(doc, frame);
    } else {
        doc.picture_frames.retain(|frame| frame.object != instance);
    }
}

pub(crate) fn inherit_changes(before: &Document, doc: &mut Document, command: &Command) -> bool {
    let assignments = doc.master_pages.assignments.clone();
    let old_root_count = doc.master_pages.roots.len();
    let roots: Vec<_> = doc.master_pages.roots.iter().copied().filter(|id| doc.get_object(*id).is_some()).collect();
    let mut changed = roots.len() != old_root_count;
    doc.master_pages.roots = roots;

    let mut sources_by_master = HashMap::new();
    let restoring = matches!(command.name.as_str(), "edit.undo" | "edit.redo");
    for mut assignment in assignments {
        let Some(index) = doc.pages.iter().position(|page| page.id == assignment.page) else { continue };
        if !doc.master_pages.roots.contains(&assignment.master)
            || assignment.group.is_some_and(|id| doc.get_object(id).is_none())
        {
            // Removing the template leaves the current page content editable.
            doc.master_pages.assignments.retain(|item| item.page != assignment.page);
            changed = true;
            continue;
        }
        if let Some(group) = assignment.group
            && let Some(master) = doc.get_object(assignment.master)
            && before.get_object(assignment.master).is_some_and(|old| old.name != master.name)
        {
            let name = master.name.clone();
            if let Some(folder) = doc.find_object_mut(group.0) {
                changed |= folder.name != name;
                folder.name = name;
            }
        }
        let (old, current, members) = sources_by_master.entry(assignment.master).or_insert_with(|| {
            let old = source_ids(before, assignment.master);
            let current = source_ids(doc, assignment.master);
            let members: HashSet<_> = current.iter().copied().collect();
            (old, current, members)
        });
        let old_links = assignment.links.clone();
        // A deleted source cannot be restored. Keep the page's last copy and its edits.
        assignment.links.retain(|(source, instance)| members.contains(source) && doc.get_object(*instance).is_some());
        let (flow_changed, flow_sources) = inherit_flow(before, doc, &assignment, old, command);
        changed |= flow_changed;
        for &(source, instance) in &assignment.links {
            let history_flow_change = restoring && frame_flow(before, source) != frame_flow(doc, source);
            let inherit_text = !history_flow_change && !flow_sources.contains(&source);
            changed |= inherit_object(before, doc, source, instance, inherit_text);
        }
        if old != current {
            let previous: HashSet<_> = old.iter().copied().collect();
            let assigned: HashSet<_> = assignment.links.iter().map(|(source, _)| *source).collect();
            let added: Vec<_> = current.iter().copied()
                .filter(|id| !previous.contains(id) && !assigned.contains(id)).collect();
            add_instances(doc, &mut assignment, index, &added);
        }
        changed |= inherit_structure(before, doc, &assignment, &old_links, old, current);
        if old_links != assignment.links {
            if let Some(target) = doc.master_pages.assignments.iter_mut().find(|item| item.page == assignment.page) {
                *target = assignment;
            }
            changed = true;
        }
    }
    if changed {
        crate::longdoc::xref::refresh_xrefs(doc);
    }
    changed
}

fn frame_flow(doc: &Document, frame: ObjectId) -> Vec<ObjectId> {
    doc.get_object(frame)
        .and_then(Object::story_id)
        .map_or_else(Vec::new, |story| crate::frame::flow_frames(doc, story))
}

fn inherit_flow(
    before: &Document, doc: &mut Document, assignment: &Assignment, template: &[ObjectId], command: &Command,
) -> (bool, Vec<ObjectId>) {
    let key = match command.name.as_str() {
        "text.link" => "from",
        "text.unlink" => "id",
        _ => return (false, Vec::new()),
    };
    let Ok(frame) = crate::text_cmd::param_u64(&command.params, key).map(ObjectId) else { return (false, Vec::new()) };
    let chain = frame_flow(before, frame);
    let target = if key == "from" {
        let Ok(target) = crate::text_cmd::param_u64(&command.params, "to").map(ObjectId) else { return (false, Vec::new()) };
        Some(target)
    } else {
        None
    };
    let sources: Vec<_> = chain.iter().copied().chain(target).collect();
    if !sources.iter().all(|source| template.contains(source)) {
        return (false, Vec::new());
    }
    let mapped = |source| assignment.links.iter().find_map(|&(id, instance)| (source == id).then_some(instance));
    let Some(local_chain) = chain.iter().map(|&id| mapped(id)).collect::<Option<Vec<_>>>() else { return (false, sources) };
    let Some(local) = mapped(frame) else { return (false, sources) };
    let Some(story) = doc.get_object(local).and_then(Object::story_id) else { return (false, sources) };
    if crate::frame::flow_frames(doc, story) != local_chain {
        return (false, sources);
    }
    let changed = if let Some(target) = target {
        mapped(target).is_some_and(|to| {
            super::frame_text::apply_link(&Command::with_params("text.link", json!({ "from": local.0, "to": to.0 })), doc).is_ok()
        })
    } else {
        super::frame_text::unlink_inherited(doc, local).is_ok()
    };
    (changed, sources)
}

fn inherit_structure(
    before: &Document, doc: &mut Document, assignment: &Assignment,
    old_links: &[(ObjectId, ObjectId)], old: &[ObjectId], current: &[ObjectId],
) -> bool {
    let parents_changed = current.iter().any(|id| {
        before.get_object(*id).and_then(|object| object.parent_id)
            != doc.get_object(*id).and_then(|object| object.parent_id)
    });
    if old == current && !parents_changed {
        return false;
    }
    let old_map: HashMap<_, _> = old_links.iter().copied().collect();
    let map: HashMap<_, _> = assignment.links.iter().copied().collect();
    let old_board = assignment.group.or_else(|| before.linked_artboard(assignment.page).map(|board| board.id));
    let board = assignment.group.or_else(|| doc.linked_artboard(assignment.page).map(|board| board.id));
    let mut changed = false;
    for &(source, instance) in &assignment.links {
        let Some(original) = before.get_object(source) else { continue };
        let Some(template) = doc.get_object(source) else { continue };
        if original.parent_id == template.parent_id {
            continue;
        }
        let old_parent = original.parent_id.and_then(|id| old_map.get(&id).copied()).or(old_board);
        let parent = template.parent_id.and_then(|id| map.get(&id).copied()).or(board);
        if doc.get_object(instance).is_some_and(|object| object.parent_id == old_parent)
            && old_parent != parent
            && !parent.is_some_and(|id| doc.subtree_ids(instance).contains(&id))
            && let Some(object) = doc.find_object_mut(instance.0)
        {
            object.parent_id = parent;
            changed = true;
        }
    }
    if old != current {
        let mut positions: HashMap<_, _> = doc.objects.iter().enumerate().map(|(index, object)| (object.id, index)).collect();
        let ordered: Vec<_> = current.iter().filter_map(|source| map.get(source).copied())
            .filter(|instance| positions.contains_key(instance)).collect();
        let mut slots: Vec<_> = ordered.iter().map(|instance| positions[instance]).collect();
        slots.sort_unstable();
        for (id, slot) in ordered.into_iter().zip(slots) {
            let from = positions[&id];
            if from != slot {
                let displaced = doc.objects[slot].id;
                doc.objects.swap(from, slot);
                positions.insert(displaced, from);
                positions.insert(id, slot);
                changed = true;
            }
        }
    }
    changed
}

fn inherit_object(before: &Document, doc: &mut Document, source: ObjectId, instance: ObjectId, inherit_text: bool) -> bool {
    let (Some(old), Some(new), Some(local)) = (before.get_object(source), doc.get_object(source), doc.get_object(instance)) else {
        return false;
    };
    let story_ids = (old.story_id(), new.story_id(), local.story_id());
    let updated = inherited_properties(old, new, local);
    let old_frame = super::picture_frame::get_frame(before, source);
    let new_frame = super::picture_frame::get_frame(doc, source);
    let mut inherited_frame = super::picture_frame::get_frame(doc, instance);
    let converted = old.kind != ObjectKind::PictureFrame && new.kind == ObjectKind::PictureFrame
        && local.kind == old.kind;
    let mut frame_changed = false;
    if converted && inherited_frame.is_none() && let Some(mut frame) = new_frame.clone() {
        frame.object = instance;
        frame.image = doc.placed_resource(instance);
        inherited_frame = Some(frame);
        frame_changed = true;
    }
    if let (Some(old), Some(new), Some(local)) = (&old_frame, &new_frame, &mut inherited_frame) {
        macro_rules! follow {
            ($($prop:ident).+) => {
                if local.$($prop).+ == old.$($prop).+ && new.$($prop).+ != old.$($prop).+ {
                    local.$($prop).+ = new.$($prop).+;
                    frame_changed = true;
                }
            };
        }
        follow!(shape);
        follow!(fit);
        follow!(anchor);
        follow!(content.offset);
        follow!(content.scale);
        follow!(content.rotation);
    }
    let old_resource = before.placed_resource(source);
    let new_resource = doc.placed_resource(source);
    let image_changed = old_resource != new_resource && doc.placed_resource(instance) == old_resource;
    let mut changed = false;
    if let Some(updated) = updated {
        if let Some(local) = doc.find_object_mut(instance.0) {
            *local = updated;
            changed = true;
        }
    }
    if converted && let Some(object) = doc.find_object_mut(instance.0) {
        object.kind = ObjectKind::PictureFrame;
        changed = true;
    }
    if frame_changed {
        if let Some(frame) = inherited_frame {
            super::picture_frame::put_frame(doc, frame);
        }
        changed = true;
    }
    if image_changed {
        // Keep each page's frame fitting and transform when refreshing its inherited image.
        let raster = doc.get_object(source).map(|object| (object.raster_data.clone(), object.raster_size));
        let pixels = doc.pixel_layers.get(source).cloned();
        if let Some((raster, size)) = raster {
            if let Some(local) = doc.find_object_mut(instance.0) {
                local.raster_data = raster;
                local.raster_size = size;
            }
        }
        if let Some(pixels) = pixels {
            doc.pixel_layers.insert(instance, pixels);
        } else {
            doc.pixel_layers.remove(instance);
        }
        if let Some(id) = new_resource {
            let _ = doc.place_image(instance, id);
        } else {
            doc.unplace_image(instance);
        }
        if let Some(frame) = doc.picture_frames.iter_mut().find(|frame| frame.object == instance) {
            frame.image = new_resource;
        }
        changed = true;
    }
    let text_changed = inherit_text && inherit_story(before, doc, story_ids);
    if text_changed && let Some(story) = story_ids.2 {
        let _ = crate::text_cmd::refresh_art_bounds(doc, story);
    }
    changed || text_changed
}

fn inherited_properties(old: &Object, new: &Object, local: &Object) -> Option<Object> {
    let mut updated = None;
    macro_rules! follow {
        ($($field:ident),+ $(,)?) => {
            $(
                if let Ok(value) = serde_json::to_value(&old.$field)
                    && serde_json::to_value(&new.$field).is_ok_and(|next| next != value)
                    && serde_json::to_value(&local.$field).is_ok_and(|current| current == value)
                {
                    updated.get_or_insert_with(|| local.clone()).$field = new.$field.clone();
                }
            )+
        };
    }
    follow!(
        name, size, fill, stroke, additional_fills, additional_strokes, hidden_appearance,
        appearance_stack, appearance_blends, path_brush, paint_order, opacity, rotation_deg,
        shear_deg, text_scale, shape, frame, vector_crop, transparency, blend_mode, blend_ranges, effects,
    );
    updated
}
fn inherit_story(
    before: &Document,
    doc: &mut Document,
    (old_id, new_id, local_id): (Option<crate::document::StoryId>, Option<crate::document::StoryId>, Option<crate::document::StoryId>),
) -> bool {
    let (Some(old_id), Some(new_id), Some(local_id)) = (old_id, new_id, local_id) else { return false };
    let Some(old) = before.stories.iter().find(|story| story.id == old_id) else { return false };
    let Some(new) = doc.stories.iter().find(|story| story.id == new_id) else { return false };
    if old == new || new_id == local_id {
        return false;
    }
    let new = new.clone();
    let replaces_text = new.text != old.text;
    let new_len = new.text.chars().count();
    let position = doc.story_pos(local_id, 0);
    let Some(local) = doc.stories.iter_mut().find(|story| story.id == local_id) else { return false };
    if local.text != old.text {
        return false;
    }
    if new.text != old.text && (local.ranges != old.ranges || local.specials != old.specials || local.paras != old.paras) {
        return false;
    }
    let mut changed = false;
    macro_rules! follow {
        ($field:ident) => {
            if local.$field == old.$field && new.$field != old.$field {
                local.$field = new.$field;
                changed = true;
            }
        };
    }
    if new.text == old.text {
        changed |= inherit_runs(&old.ranges, &new.ranges, &mut local.ranges);
        changed |= inherit_runs(&old.paras, &new.paras, &mut local.paras);
    } else {
        follow!(text);
        follow!(ranges);
        follow!(paras);
    }
    follow!(specials);
    if changed && replaces_text {
        crate::longdoc::marks::shift_after_story_rebuild(doc, position, old.text.chars().count(), new_len);
    }
    changed
}

fn inherit_runs(old: &[Value], new: &[Value], local: &mut Vec<Value>) -> bool {
    if old == new {
        return false;
    }
    if old == local.as_slice() {
        *local = new.to_vec();
        return true;
    }
    if old.len() != new.len() || old.len() != local.len() {
        return false;
    }
    let same_spans = old.iter().zip(new).zip(local.iter()).all(|((old, new), local)| {
        ["start", "end"].iter().all(|key| old.get(key) == new.get(key) && old.get(key) == local.get(key))
    });
    if !same_spans {
        return false;
    }
    let mut changed = false;
    for ((old, new), local) in old.iter().zip(new).zip(local) {
        changed |= inherit_fields(old, new, local);
    }
    changed
}

fn inherit_fields(old: &Value, new: &Value, local: &mut Value) -> bool {
    match (old, new, local) {
        (Value::Object(old), Value::Object(new), Value::Object(local)) => {
            let mut changed = false;
            for key in old.keys().chain(new.keys().filter(|key| !old.contains_key(*key))) {
                let before = old.get(key);
                let after = new.get(key);
                if before == after {
                    continue;
                }
                if local.get(key) == before {
                    if let Some(value) = after {
                        local.insert(key.clone(), value.clone());
                    } else {
                        local.remove(key);
                    }
                    changed = true;
                } else if let (Some(before), Some(after), Some(current)) = (before, after, local.get_mut(key)) {
                    changed |= inherit_fields(before, after, current);
                }
            }
            changed
        }
        _ => false,
    }
}

pub fn lock_positions(doc: &mut Document) {
    let mut positions = Vec::new();
    for assignment in &doc.master_pages.assignments {
        let Some(root) = doc.get_object(assignment.master) else { continue };
        let Some(index) = doc.pages.iter().position(|page| page.id == assignment.page) else { continue };
        let [x, y, _, _] = page_rect(doc, index);
        for &(source, instance) in &assignment.links {
            if let Some(source) = doc.get_object(source) {
                positions.push((instance, [source.pos[0] - root.pos[0] + x, source.pos[1] - root.pos[1] + y]));
            }
        }
    }
    for (instance, position) in positions {
        if let Some(object) = doc.find_object_mut(instance.0) {
            object.pos = position;
        }
    }
}
