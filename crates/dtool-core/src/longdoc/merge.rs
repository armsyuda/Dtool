use crate::command::Session;
use crate::document::{Document, PageId};
use crate::object::{ObjectId, ObjectKind};
use crate::resources::ResourceId;
use crate::table::model::StoryId;
use crate::text_cmd::push_change;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;
use std::path::PathBuf;

pub const MERGE_UNDO: &str = "doc.merge.undo";

fn page_step(doc: &Document) -> [f32; 2] {
    let size = crate::data_merge::layout::page_size(doc);
    if doc.setup.arrange_vertical { [0.0, size[1]] } else { [size[0], 0.0] }
}

impl Document {
    pub fn merge_document(&mut self, mut other: Document, at: usize) {
        let at = at.min(self.pages.len());
        let count = other.pages.len();
        let step = page_step(self);
        let other_step = page_step(&other);
        let shift: Vec<(usize, bool)> = self
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| (i, crate::data_merge::layout::object_page_index(self, o.id) >= at))
            .collect();
        if at < self.pages.len() {
            for (i, moved) in shift {
                if moved {
                    let object = &mut self.objects[i];
                    object.pos[0] += step[0] * count as f32;
                    object.pos[1] += step[1] * count as f32;
                }
            }
        }
        self.raise_id_high_water();
        other.raise_id_high_water();
        let text_offset: usize = self.stories.iter().map(|s| s.text.chars().count() + 1).sum();
        let object_offset = self.next_object_id.max(1) - 1;
        let story_offset = self.next_story_id.max(1) - 1;
        let resource_offset = self.next_resource_id.max(1) - 1;
        let page_offset = self.next_page_id.max(1) - 1;
        let map_id = |id: ObjectId| ObjectId(id.0 + object_offset);
        let map_story = |id: StoryId| {
            if id.0 == 0 {
                id
            } else {
                StoryId(id.0 + story_offset)
            }
        };
        let map_resource = |id: ResourceId| {
            if id.0 == 0 {
                id
            } else {
                ResourceId(id.0 + resource_offset)
            }
        };
        let map_page = |id: PageId| {
            if id.0 == 0 {
                id
            } else {
                PageId(id.0 + page_offset)
            }
        };
        let page_indices: Vec<usize> = other
            .objects
            .iter()
            .map(|o| crate::data_merge::layout::object_page_index(&other, o.id))
            .collect();
        for (mut object, page) in other.objects.drain(..).zip(page_indices) {
            object.pos[0] += step[0] * (at + page) as f32 - other_step[0] * page as f32;
            object.pos[1] += step[1] * (at + page) as f32 - other_step[1] * page as f32;
            object.id = map_id(object.id);
            object.parent_id = object.parent_id.map(map_id);
            object.baked_warps.iter_mut().for_each(|warp| *warp = map_id(*warp));
            object.page_id = object.page_id.map(map_page);
            match &mut object.kind {
                ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => *story = map_story(*story),
                ObjectKind::PathText(data) => {
                    data.path = map_id(data.path);
                    data.story = map_story(data.story);
                }
                ObjectKind::Table(table) => {
                    for cell in &mut table.cells {
                        cell.story = map_story(cell.story);
                    }
                }
                _ => {}
            }
            self.objects.push(object);
        }
        for mut story in other.stories.drain(..) {
            story.id = map_story(story.id);
            self.stories.push(story);
        }
        for mut resource in other.resources.drain(..) {
            resource.id = map_resource(resource.id);
            self.resources.push(resource);
        }
        for mut placed in other.placed_images.drain(..) {
            placed.object_id = map_id(placed.object_id);
            placed.resource = map_resource(placed.resource);
            self.placed_images.push(placed);
        }
        for entry in other.pixel_layers.entries.drain(..) {
            self.pixel_layers.insert(map_id(entry.object), entry.layer);
        }
        for mut mask in other.layer_masks.drain(..) {
            mask.object = map_id(mask.object);
            self.layer_masks.push(mask);
        }
        for section in self.sections.iter_mut() {
            if section.start_page > at {
                section.start_page += count;
            }
        }
        let existing: Vec<String> = self.anchors.iter().map(|a| a.name.clone()).collect();
        let mut renamed = Vec::new();
        for mut anchor in other.anchors.drain(..) {
            let mut names = existing.clone();
            names.extend(renamed.iter().map(|(_, n): &(String, String)| n.clone()));
            let unique = crate::longdoc::helpers::unique_name(&names, &anchor.name);
            renamed.push((anchor.name.clone(), unique.clone()));
            anchor.name = unique;
            anchor.story_pos += text_offset;
            self.anchors.push(anchor);
        }
        let rename = |name: &str| renamed.iter().find(|(old, _)| old == name).map(|(_, new)| new.clone()).unwrap_or_else(|| name.to_string());
        for mut link in other.hyperlinks.drain(..) {
            link.range = (link.range.0 + text_offset, link.range.1 + text_offset);
            match &mut link.target {
                crate::longdoc::anchors::LinkTarget::Anchor(name) => *name = rename(name),
                crate::longdoc::anchors::LinkTarget::Page(page) => *page += at,
                _ => {}
            }
            self.hyperlinks.push(link);
        }
        for mut xref in other.cross_refs.drain(..) {
            xref.anchor = rename(&xref.anchor);
            self.cross_refs.push(xref);
        }
        for page in other.pages.iter_mut() {
            page.id = map_page(page.id);
        }
        let tail = self.pages.split_off(at);
        self.pages.extend(other.pages.drain(..));
        self.pages.extend(tail);
        self.raise_id_high_water();
    }
}

pub fn handle_merge(cmd: &Command, doc: &mut Document) -> CommandResult {
    let path_str = cmd.params.get("path").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).ok_or_else(CommandError::invalid_params)?;
    let at_val = cmd.params.get("at").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    if at_val > doc.pages.len() {
        return Err(CommandError::out_of_range());
    }
    let path = PathBuf::from(path_str);
    let other_doc = crate::commands::document_files::read_document(&path)?;
    let added = other_doc.pages.len();
    doc.merge_document(other_doc, at_val);
    Ok(json!({ "pages_added": added, "page_count": doc.pages.len() }))
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        "doc.merge" | MERGE_UNDO => Some(session.has_document),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        "doc.merge" => Some((|| {
            let before = serde_json::to_value(&session.document).map_err(|_| CommandError::new("IO.OTHER"))?;
            let result = handle_merge(command, &mut session.document)?;
            let undo = Command::with_params(MERGE_UNDO, json!({ "document": before }));
            push_change(session, "doc.merge", command.clone(), undo);
            let mut result = result;
            result["doc_version"] = json!(session.generation);
            Ok(result)
        })()),
        MERGE_UNDO => Some((|| {
            let value = command.params.get("document").cloned().ok_or_else(CommandError::invalid_params)?;
            let mut document: Document = serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?;
            document.pixel_selection = session.document.pixel_selection.take();
            document.version = session.document.version;
            document.keep_high_water_from(&session.document);
            session.document = document;
            crate::text_cmd::bump(session);
            Ok(json!({ "doc_version": session.generation }))
        })()),
        _ => None,
    }
}
