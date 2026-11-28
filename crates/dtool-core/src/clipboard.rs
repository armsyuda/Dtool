use serde::{Deserialize, Serialize};
use crate::object::Object;
use crate::commands::symbols::{Symbols, clipboard as symbol_clipboard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardKind {
    Empty,
    Objects,
    Image,
    Text,
    Style,
    Effects,
    Table,
}

impl Default for ClipboardKind {
    fn default() -> Self {
        ClipboardKind::Empty
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardPayload {
    pub kind: ClipboardKind,
    pub objects: Option<Vec<Object>>,
    pub image_png: Option<Vec<u8>>,
    pub text: Option<String>,
    pub style: Option<serde_json::Value>,
    pub effects: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<ClipboardBundle>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardBundle {
    #[serde(default)]
    pub objects: Vec<Object>,
    #[serde(default)]
    pub roots: Vec<crate::object::ObjectId>,
    #[serde(default)]
    pub stories: Vec<crate::document::Story>,
    #[serde(default)]
    pub resources: Vec<crate::resources::ImageResource>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub brush_sources: std::collections::BTreeMap<String, crate::resources::ImageResource>,
    #[serde(default)]
    pub placed_images: Vec<crate::resources::PlacedImage>,
    #[serde(default)]
    pub layer_masks: Vec<crate::raster_source::LayerMask>,
    #[serde(default)]
    pub pixel_layers: Vec<crate::pixel::PixelLayerEntry>,
    #[serde(default)]
    pub frame_chains: Vec<crate::frame::FrameChain>,
    #[serde(default)]
    pub swatches: Vec<crate::colour::swatch::Swatch>,
    #[serde(default)]
    pub swatch_bindings: Vec<crate::colour::swatch::SwatchBinding>,
    #[serde(default)]
    pub text_links: Vec<crate::text_link::TextLink>,
    #[serde(default, skip_serializing_if = "Symbols::is_empty")]
    pub symbols: Symbols,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_dimensions: Option<(crate::document::Units, f32)>,
    #[serde(skip)]
    pub(crate) symbol_source: Option<crate::workspace::ContextId>,
}

#[derive(Debug, Default)]
pub struct AppClipboard {
    payload: std::sync::Arc<std::sync::RwLock<std::sync::Arc<ClipboardPayload>>>,
}

impl AppClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_imported_objects(&mut self, document: &crate::Document, roots: &[crate::object::ObjectId]) {
        let mut bundle = collect_bundle(document, roots);
        bundle.source_dimensions = Some((document.units, document.dpi));
        self.set_bundle(bundle);
    }

    pub fn share(&self) -> Self {
        Self { payload: std::sync::Arc::clone(&self.payload) }
    }

    pub fn is_shared_with(&self, other: &AppClipboard) -> bool {
        std::sync::Arc::ptr_eq(&self.payload, &other.payload)
    }

    fn replace(&mut self, payload: ClipboardPayload) {
        let mut slot = self.payload.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = std::sync::Arc::new(payload);
    }

    pub fn set_objects(&mut self, objects: Vec<Object>) {
        self.replace(ClipboardPayload { kind: ClipboardKind::Objects, objects: Some(objects), ..ClipboardPayload::default() });
    }

    pub fn set_bundle(&mut self, bundle: ClipboardBundle) {
        self.replace_bundle(bundle, None);
    }

    fn set_flattened(&mut self, bundle: ClipboardBundle, png: Vec<u8>) {
        self.replace_bundle(bundle, Some(png));
    }

    fn replace_bundle(&mut self, bundle: ClipboardBundle, image_png: Option<Vec<u8>>) {
        let roots = bundle.roots.iter().filter_map(|id| bundle.objects.iter().find(|o| o.id == *id).cloned()).collect();
        self.replace(ClipboardPayload { kind: ClipboardKind::Objects, objects: Some(roots), bundle: Some(bundle), image_png, ..ClipboardPayload::default() });
    }

    pub fn set_image(&mut self, png: Vec<u8>) {
        self.replace(ClipboardPayload { kind: ClipboardKind::Image, image_png: Some(png), ..ClipboardPayload::default() });
    }

    pub fn set_text(&mut self, text: String) {
        self.replace(ClipboardPayload { kind: ClipboardKind::Text, text: Some(text), ..ClipboardPayload::default() });
    }

    pub fn set_style(&mut self, style: serde_json::Value) {
        let current = self.get_payload();
        if current.kind == ClipboardKind::Text {
            let mut kept = (*current).clone();
            kept.style = Some(style);
            self.replace(kept);
            return;
        }
        self.replace(ClipboardPayload { kind: ClipboardKind::Style, style: Some(style), ..ClipboardPayload::default() });
    }

    pub fn set_effects(&mut self, effects: serde_json::Value) {
        self.replace(ClipboardPayload { kind: ClipboardKind::Effects, effects: Some(effects), ..ClipboardPayload::default() });
    }

    pub fn clear(&mut self) {
        self.replace(ClipboardPayload::default());
    }

    pub fn kind(&self) -> ClipboardKind {
        self.get_payload().kind
    }

    pub fn restore_payload(&mut self, payload: std::sync::Arc<ClipboardPayload>) {
        let mut slot = self.payload.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = payload;
    }

    pub fn get_payload(&self) -> std::sync::Arc<ClipboardPayload> {
        let slot = self.payload.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        std::sync::Arc::clone(&slot)
    }
}

#[derive(Debug, Default)]
pub struct OsClipboard;

impl OsClipboard {
    pub fn new() -> Self {
        Self
    }

    pub fn query(&self) -> ClipboardPayload {
        ClipboardPayload::default()
    }
}

pub fn clipboard_query(app: &AppClipboard, os: &OsClipboard) -> ClipboardPayload {
    let app_payload = app.get_payload();
    
    if app_payload.kind != ClipboardKind::Empty {
        return (*app_payload).clone();
    }

    os.query()
}

pub(crate) fn object_stories(object: &Object) -> Vec<crate::document::StoryId> {
    let mut out: Vec<crate::document::StoryId> = object.story_id().into_iter().collect();
    if let crate::object::ObjectKind::Table(table) = &object.kind {
        out.extend(table.cells.iter().map(|cell| cell.story));
    }
    out
}

impl ClipboardBundle {
    pub(crate) fn remove_members(&mut self, removed: &std::collections::HashSet<crate::ObjectId>) {
        self.objects.retain(|object| !removed.contains(&object.id));
        let stories: std::collections::HashSet<_> = self.objects.iter().flat_map(object_stories).collect();
        self.stories.retain(|story| stories.contains(&story.id));
        self.text_links.retain(|link| stories.contains(&link.story));
        self.placed_images.retain(|image| !removed.contains(&image.object_id));
        self.resources.retain(|resource| self.placed_images.iter().any(|image| image.resource == resource.id));
        self.layer_masks.retain(|mask| !removed.contains(&mask.object));
        self.pixel_layers.retain(|entry| !removed.contains(&entry.object));
        self.swatch_bindings.retain(|binding| !removed.contains(&binding.object));
        self.frame_chains.retain_mut(|chain| {
            chain.frames.retain(|id| !removed.contains(id));
            !chain.frames.is_empty()
        });
    }
}

fn subtree(doc: &crate::document::Document, ids: &[crate::object::ObjectId]) -> std::collections::HashSet<crate::object::ObjectId> {
    let mut set: std::collections::HashSet<crate::object::ObjectId> = ids.iter().copied().filter(|id| doc.get_object(*id).is_some()).collect();
    loop {
        let before = set.len();
        for object in &doc.objects {
            if object.parent_id.is_some_and(|parent| set.contains(&parent)) {
                set.insert(object.id);
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

pub fn collect_bundle(doc: &crate::document::Document, ids: &[crate::object::ObjectId]) -> ClipboardBundle {
    collect_bundle_with_guides(doc, ids, false)
}

fn collect_bundle_with_guides(doc: &crate::Document, ids: &[crate::ObjectId], reuse_external_guides: bool) -> ClipboardBundle {
    let mut set = subtree(doc, ids);
    let paths: Vec<_> = doc.objects.iter().filter(|object| set.contains(&object.id)).filter_map(|object| {
        let path = object.path_text_data()?.path;
        (!(reuse_external_guides && symbol_clipboard::has_external_guide(doc, object.id))).then_some(path)
    }).collect();
    for path in paths {
        if doc.get_object(path).is_some() && set.insert(path) {
            for id in subtree(doc, &[path]) {
                set.insert(id);
            }
        }
    }
    collect_bundle_members(doc, &set)
}

pub(crate) fn collect_bundle_members(doc: &crate::Document, set: &std::collections::HashSet<crate::ObjectId>) -> ClipboardBundle {
    let objects: Vec<Object> = doc.objects.iter().filter(|o| set.contains(&o.id)).cloned().collect();
    let roots = objects.iter().filter(|o| !o.parent_id.is_some_and(|parent| set.contains(&parent))).map(|o| o.id).collect();
    let story_ids: std::collections::HashSet<crate::document::StoryId> = objects.iter().flat_map(object_stories).collect();
    let placed_images: Vec<crate::resources::PlacedImage> = doc.placed_images.iter().filter(|p| set.contains(&p.object_id)).cloned().collect();
    let resources = doc.resources.iter().filter(|r| placed_images.iter().any(|p| p.resource == r.id)).cloned().collect();
    use crate::paint::{PaintResourceKind, ResourceCollector, visit_vector_resources};
    let mut collector = ResourceCollector::new();
    for object in &objects {
        visit_vector_resources(&mut collector, object);
    }
    let brush_sources = collector.finish().into_iter()
        .filter(|reference| matches!(reference.kind, PaintResourceKind::Bitmap | PaintResourceKind::Pattern))
        .filter_map(|reference| {
            let image = crate::path_brush::snapshot_resource(doc, &reference.id).or_else(|| {
                // Keep an unavailable link so paste assigns a new ID and the user can relink it.
                crate::path_brush::resource(doc, &reference.id).filter(|image| image.is_linked()).cloned()
            })?;
            Some((reference.id, image))
        })
        .collect();
    let frame_chains = doc
        .frame_chains
        .iter()
        .filter(|chain| story_ids.contains(&chain.story))
        .map(|chain| crate::frame::FrameChain { story: chain.story, frames: chain.frames.iter().copied().filter(|f| set.contains(f)).collect() })
        .filter(|chain| !chain.frames.is_empty())
        .collect();
    let swatch_bindings: Vec<crate::colour::swatch::SwatchBinding> = doc.swatch_bindings.iter().copied().filter(|b| set.contains(&b.object)).collect();
    let swatches = doc.swatches.iter().filter(|w| swatch_bindings.iter().any(|b| b.swatch == w.id)).cloned().collect();
    let text_links = doc.text_links.iter().filter(|l| story_ids.contains(&l.story)).cloned().collect();
    ClipboardBundle {
        swatches,
        swatch_bindings,
        text_links,
        roots,
        stories: doc.stories.iter().filter(|s| story_ids.contains(&s.id)).cloned().collect(),
        resources,
        brush_sources,
        placed_images,
        layer_masks: doc.layer_masks.iter().filter(|m| set.contains(&m.object)).cloned().collect(),
        pixel_layers: doc.pixel_layers.entries.iter().filter(|e| set.contains(&e.object)).cloned().collect(),
        frame_chains,
        objects,
        ..ClipboardBundle::default()
    }
}

pub fn bundle_from_objects(objects: &[Object]) -> ClipboardBundle {
    let ids: std::collections::HashSet<crate::object::ObjectId> = objects.iter().map(|o| o.id).collect();
    ClipboardBundle {
        roots: objects.iter().filter(|o| !o.parent_id.is_some_and(|p| ids.contains(&p))).map(|o| o.id).collect(),
        objects: objects.to_vec(),
        ..ClipboardBundle::default()
    }
}

pub fn payload_bundle(payload: &ClipboardPayload) -> Option<ClipboardBundle> {
    if payload.kind != ClipboardKind::Objects {
        return None;
    }
    let bundle = match &payload.bundle {
        Some(bundle) => bundle.clone(),
        None => bundle_from_objects(payload.objects.as_deref().unwrap_or(&[])),
    };
    (!bundle.roots.is_empty()).then_some(bundle)
}

pub fn paste_bundle(doc: &mut crate::document::Document, bundle: &ClipboardBundle, parent: Option<crate::object::ObjectId>, offset: [f32; 2]) -> Vec<crate::object::ObjectId> {
    let ids = paste_bundle_mapped(doc, bundle, parent, offset);
    bundle.roots.iter().filter_map(|id| ids.get(id).copied()).collect()
}

pub(crate) fn paste_bundle_mapped(doc: &mut crate::document::Document, bundle: &ClipboardBundle, parent: Option<crate::object::ObjectId>, offset: [f32; 2]) -> std::collections::HashMap<crate::object::ObjectId, crate::object::ObjectId> {
    paste_bundle_with_reuse(doc, bundle, parent, offset, &std::collections::HashMap::new())
}

pub(crate) fn paste_bundle_with_reuse(
    doc: &mut crate::Document, bundle: &ClipboardBundle, parent: Option<crate::ObjectId>,
    offset: [f32; 2], existing: &std::collections::HashMap<crate::ObjectId, crate::ObjectId>,
) -> std::collections::HashMap<crate::ObjectId, crate::ObjectId> {
    paste_bundle_impl(doc, bundle, parent, offset, existing, false)
}

pub(crate) fn copy_bundle_in_document(
    doc: &mut crate::Document, bundle: &ClipboardBundle, offset: [f32; 2],
) -> std::collections::HashMap<crate::ObjectId, crate::ObjectId> {
    paste_bundle_impl(doc, bundle, None, offset, &std::collections::HashMap::new(), true)
}

fn remap_pasted_paints(
    doc: &mut crate::Document, object: &mut Object, resources: &std::collections::HashMap<String, String>,
) {
    use crate::paint::{Paint, Transparency};
    let remap = |id: &mut String| {
        if let Some(mapped) = resources.get(id) {
            id.clone_from(mapped);
        }
    };
    object.path_brush = object.path_brush.take().and_then(|mut texture| {
        texture.resource_id = resources.get(&texture.resource_id)?.clone();
        Some(texture)
    });
    let fills = std::iter::once(&mut object.fill).chain(&mut object.additional_fills);
    let strokes = std::iter::once(&mut object.stroke).chain(&mut object.additional_strokes);
    for paint in fills.chain(strokes.map(|stroke| &mut stroke.paint)) {
        match paint {
            Paint::Bitmap(bitmap) => remap(&mut bitmap.resource_id),
            Paint::Pattern(pattern) => remap(&mut pattern.resource_id),
            _ => {}
        }
    }
    if let Transparency::InverseBitmap(bitmap) = &mut object.transparency {
        remap(&mut bitmap.resource_id);
    }
    if let crate::object::ObjectKind::Compound(compound) = &mut object.kind {
        for operand in &mut compound.operands {
            // Releasing a compound promotes these stored IDs to layer IDs.
            operand.id = doc.alloc_object_id();
            remap_pasted_paints(doc, operand, resources);
        }
    }
}

fn paste_bundle_impl(
    doc: &mut crate::Document, bundle: &ClipboardBundle, parent: Option<crate::ObjectId>,
    offset: [f32; 2], existing: &std::collections::HashMap<crate::ObjectId, crate::ObjectId>,
    reuse_resources: bool,
) -> std::collections::HashMap<crate::ObjectId, crate::ObjectId> {
    use crate::object::{ObjectId, ObjectKind};
    let mut stories = std::collections::HashMap::new();
    for (original, actual) in bundle.objects.iter().filter_map(|object| {
        doc.get_object(*existing.get(&object.id)?).map(|target| (object, target))
    }) {
        for (source, target) in object_stories(original).into_iter().zip(object_stories(actual)) {
            stories.entry(source).or_insert(target);
        }
    }
    let reused_stories: std::collections::HashSet<_> = stories.keys().copied().collect();
    let mut needed_stories = std::collections::HashSet::new();
    if !existing.is_empty() {
        needed_stories.extend(bundle.objects.iter().filter(|object| !existing.contains_key(&object.id)).flat_map(object_stories));
    }
    for story in &bundle.stories {
        if reused_stories.contains(&story.id) || (!existing.is_empty() && !needed_stories.contains(&story.id)) {
            continue;
        }
        let id = doc.alloc_story_id();
        stories.insert(story.id, id);
        let mut copy = story.clone();
        copy.id = id;
        doc.stories.push(copy);
    }
    let mut resources = std::collections::HashMap::new();
    for resource in &bundle.resources {
        if !existing.is_empty() && !bundle.placed_images.iter().any(|image| image.resource == resource.id && !existing.contains_key(&image.object_id)) {
            continue;
        }
        if reuse_resources && doc.image_resource(resource.id).is_some() {
            resources.insert(resource.id, resource.id);
            continue;
        }
        let id = doc.alloc_resource_id();
        resources.insert(resource.id, id);
        let mut copy = resource.clone();
        copy.id = id;
        doc.resources.push(copy);
    }
    let mut brush_ids = std::collections::HashMap::new();
    for (reference, image) in &bundle.brush_sources {
        let id = crate::path_brush::import_snapshot(doc, image);
        brush_ids.insert(reference.clone(), id.0.to_string());
    }
    let mut ids = existing.clone();
    let new_objects = || bundle.objects.iter().filter(|object| !existing.contains_key(&object.id));
    for object in new_objects() {
        let mut copy = object.clone();
        copy.id = ObjectId(0);
        remap_pasted_paints(doc, &mut copy, &brush_ids);
        let new_id = doc.create_object(copy);
        ids.insert(object.id, new_id);
    }
    let story = |old: crate::document::StoryId| stories.get(&old).copied().unwrap_or(old);
    let pages: Vec<crate::document::PageId> = doc.pages.iter().map(|p| p.id).collect();
    for object in new_objects() {
        let Some(new_id) = ids.get(&object.id).copied() else { continue };
        let Some(target) = doc.find_object_mut(new_id.0) else { continue };
        target.parent_id = match object.parent_id.and_then(|p| ids.get(&p).copied()) {
            Some(mapped) => Some(mapped),
            None => parent,
        };
        target.baked_warps = object.baked_warps.iter().filter_map(|id| ids.get(id).copied()).collect();
        if target.page_id.is_some_and(|page| !pages.contains(&page)) {
            target.page_id = None;
        }
        target.pos[0] += offset[0];
        target.pos[1] += offset[1];
        match &mut target.kind {
            ObjectKind::ArtText { story: s } | ObjectKind::FrameText { story: s } => *s = story(*s),
            ObjectKind::PathText(data) => {
                data.story = story(data.story);
                if let Some(path) = ids.get(&data.path) {
                    data.path = *path;
                }
            }
            ObjectKind::Table(table) => {
                for cell in &mut table.cells {
                    cell.story = story(cell.story);
                }
            }
            _ => {}
        }
    }
    for placed in bundle.placed_images.iter().filter(|image| !existing.contains_key(&image.object_id)) {
        if let (Some(object_id), Some(resource)) = (ids.get(&placed.object_id), resources.get(&placed.resource)) {
            doc.placed_images.push(crate::resources::PlacedImage { object_id: *object_id, resource: *resource });
        }
    }
    for mask in bundle.layer_masks.iter().filter(|mask| !existing.contains_key(&mask.object)) {
        if let Some(object) = ids.get(&mask.object) {
            let mut copy = mask.clone();
            copy.object = *object;
            doc.layer_masks.push(copy);
        }
    }
    for entry in bundle.pixel_layers.iter().filter(|entry| !existing.contains_key(&entry.object)) {
        if let Some(object) = ids.get(&entry.object) {
            doc.pixel_layers.insert(*object, entry.layer.clone());
        }
    }
    for chain in &bundle.frame_chains {
        if !chain.frames.iter().any(|frame| !existing.contains_key(frame) && ids.contains_key(frame)) {
            continue;
        }
        let frames: Vec<ObjectId> = chain.frames.iter().filter_map(|id| ids.get(id).copied()).collect();
        let id = story(chain.story);
        if let Some(actual) = doc.frame_chains.iter_mut().find(|entry| entry.story == id) {
            actual.frames.retain(|frame| !frames.contains(frame));
            actual.frames.extend(frames);
        } else {
            doc.frame_chains.push(crate::frame::FrameChain { story: id, frames });
        }
    }
    let mut swatch_ids = std::collections::HashMap::new();
    for swatch in &bundle.swatches {
        let same = |w: &&crate::colour::swatch::Swatch| w.name == swatch.name && w.colour == swatch.colour;
        if let Some(existing) = doc.swatches.iter().filter(same).find(|w| w.id == swatch.id).or_else(|| doc.swatches.iter().find(same)) {
            swatch_ids.insert(swatch.id, existing.id);
        }
    }
    for binding in bundle.swatch_bindings.iter().filter(|binding| !existing.contains_key(&binding.object)) {
        if let (Some(object), Some(swatch)) = (ids.get(&binding.object), swatch_ids.get(&binding.swatch)) {
            doc.swatch_bindings.push(crate::colour::swatch::SwatchBinding { object: *object, swatch: *swatch, story: binding.story.map(story), ..*binding });
        }
    }
    for link in bundle.text_links.iter().filter(|link| !reused_stories.contains(&link.story)) {
        if let Some(new_story) = stories.get(&link.story) {
            let mut copy = link.clone();
            copy.story = *new_story;
            doc.text_links.push(copy);
        }
    }
    ids
}

pub fn remove_subtree(doc: &mut crate::document::Document, ids: &[crate::object::ObjectId]) {
    let set = subtree(doc, ids);
    doc.objects.retain(|o| !set.contains(&o.id));
    doc.placed_images.retain(|p| !set.contains(&p.object_id));
    doc.layer_masks.retain(|m| !set.contains(&m.object));
    doc.pixel_layers.entries.retain(|e| !set.contains(&e.object));
    doc.swatch_bindings.retain(|b| !set.contains(&b.object));
    crate::frame::prune_chains(doc);
}

pub const EDIT_COPY: &str = "edit.copy";
pub const EDIT_CUT: &str = "edit.cut";
pub const EDIT_PASTE: &str = "edit.paste";
pub const EDIT_PASTE_INSIDE: &str = "edit.paste_inside";
pub const EDIT_PASTE_STYLE: &str = "edit.paste_style";
pub const EDIT_PASTE_FX: &str = "edit.paste_fx";
pub const EDIT_DUPLICATE: &str = "edit.duplicate";
pub const EDIT_POWER_DUPLICATE: &str = "edit.power_duplicate";
pub mod move_duplicate;
pub mod flattened;
pub const EDIT_PASTE_SPECIAL: &str = "edit.paste_special";
pub const EDIT_PASTE_WITHOUT_FORMAT: &str = "edit.paste_without_format";
pub const DOC_NEW_FROM_CLIPBOARD: &str = "doc.new_from_clipboard";
const PASTE_STEP: f32 = 10.0;

fn selected_roots(session: &crate::command::Session) -> Vec<crate::object::ObjectId> {
    session.selection.ids.iter().copied().filter(|id| session.document.get_object(*id).is_some()).collect()
}

fn is_group(session: &crate::command::Session, id: crate::object::ObjectId) -> bool {
    session.document.get_object(id).is_some_and(|o| o.kind == crate::object::ObjectKind::Group)
}

fn paste_text(payload: &ClipboardPayload) -> Option<String> {
    (payload.kind == ClipboardKind::Text).then(|| payload.text.clone().unwrap_or_default()).filter(|t| !t.is_empty())
}

pub fn os_text_for_payload(payload: &ClipboardPayload) -> String {
    match payload.kind {
        ClipboardKind::Text => payload.text.clone().unwrap_or_default(),
        ClipboardKind::Objects => payload
            .bundle
            .as_ref()
            .map(|bundle| bundle.stories.iter().map(|story| story.text.as_str()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn take_os_text(session: &mut crate::command::Session, text: &str) -> bool {
    let text = text.replace("\r\n", "\n");
    if text.is_empty() || os_text_for_payload(&session.clipboard.get_payload()) == text {
        return false;
    }
    session.clipboard.set_text(text);
    true
}

pub fn can_execute(session: &crate::command::Session, command: &dtool_protocol::Command) -> Option<bool> {
    let payload = || session.clipboard.get_payload();
    let has_selection = !selected_roots(session).is_empty();
    let allowed = match command.name.as_str() {
        EDIT_COPY | EDIT_CUT | EDIT_DUPLICATE => has_selection,
        flattened::COPY_FLATTENED => true,
        move_duplicate::MOVE_DUPLICATE => move_duplicate::available(session),
        EDIT_PASTE => {
            let payload = payload();
            payload_bundle(&payload).is_some() || paste_text(&payload).is_some()
        }
        EDIT_PASTE_INSIDE => session.selection.ids.len() == 1 && is_group(session, session.selection.ids[0]) && payload_bundle(&payload()).is_some(),
        EDIT_PASTE_STYLE => has_selection && matches!(payload().kind, ClipboardKind::Style | ClipboardKind::Objects),
        EDIT_PASTE_FX => has_selection && matches!(payload().kind, ClipboardKind::Effects | ClipboardKind::Objects),
        EDIT_POWER_DUPLICATE => !session.last_duplicate_ids.is_empty() && (session.last_transform_delta.is_some() || !session.last_duplicate_steps.is_empty()),
        EDIT_PASTE_WITHOUT_FORMAT => paste_text(&payload()).is_some(),
        EDIT_PASTE_SPECIAL => !crate::command::paste_special_formats(&payload()).is_empty(),
        _ => return None,
    };
    Some(session.has_document && allowed)
}

pub fn run(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Option<Result<serde_json::Value, dtool_protocol::CommandError>> {
    let result = match command.name.as_str() {
        EDIT_COPY => copy(session, false),
        flattened::COPY_FLATTENED => flattened::copy(session),
        EDIT_CUT => copy(session, true),
        EDIT_PASTE => paste(session, command),
        EDIT_PASTE_INSIDE => paste_inside(session),
        EDIT_PASTE_STYLE => paste_style(session),
        EDIT_PASTE_FX => paste_fx(session),
        EDIT_PASTE_WITHOUT_FORMAT => paste_without_format(session, command),
        EDIT_DUPLICATE => duplicate(session, command),
        move_duplicate::MOVE_DUPLICATE => move_duplicate::run(session, &command.params),
        EDIT_POWER_DUPLICATE => power_duplicate(session, EDIT_POWER_DUPLICATE),
        EDIT_PASTE_SPECIAL => paste_special(session, command),
        _ => return None,
    };
    Some(result)
}

fn ids_json(ids: &[crate::object::ObjectId]) -> serde_json::Value {
    serde_json::json!({ "ids": ids.iter().map(|id| id.0).collect::<Vec<u64>>() })
}

fn collect_for_edit(session: &crate::Session, roots: &[crate::ObjectId]) -> ClipboardBundle {
    let mut bundle = collect_bundle(&session.document, roots);
    let symbols = symbol_clipboard::capture(&session.document, &bundle);
    let guides: Vec<_> = symbol_clipboard::external_guides(&symbols)
        .filter(|guide| !bundle.objects.iter().any(|object| object.id == *guide))
        .filter(|guide| session.document.get_object(*guide).is_some()).collect();
    if !guides.is_empty() {
        let ids: Vec<_> = roots.iter().copied().chain(guides).collect();
        bundle = collect_bundle(&session.document, &ids);
    }
    bundle.symbols = symbols;
    bundle.source_dimensions = Some((session.document.units, session.document.dpi));
    bundle.symbol_source = session.context;
    bundle
}

fn copy(session: &mut crate::command::Session, cut: bool) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if !session.has_document {
        return Err(dtool_protocol::CommandError::no_document());
    }
    let roots = selected_roots(session);
    if roots.is_empty() {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let bundle = collect_for_edit(session, &roots);
    if !cut {
        session.clipboard.set_bundle(bundle);
        return Ok(serde_json::json!({ "doc_version": session.generation, "changes": [{ "name": EDIT_COPY }] }));
    }
    let before = crate::commands::arrange::snapshot(session);
    remove_subtree(&mut session.document, &roots);
    session.selection.ids.clear();
    let result = crate::commands::arrange::commit_snapshot(session, EDIT_CUT, before, ids_json(&roots))?;
    session.clipboard.set_bundle(bundle);
    Ok(result)
}

fn commit_paste(session: &mut crate::command::Session, name: &str, before: crate::commands::arrange::NodeSnapshot, ids: Vec<crate::object::ObjectId>) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    session.selection.set(ids.clone(), crate::selection::SelectOp::Replace);
    crate::commands::arrange::commit_snapshot(session, name, before, ids_json(&ids))
}

fn paste_into_artboards(
    doc: &mut crate::Document,
    bundle: &ClipboardBundle,
    parent: Option<crate::ObjectId>,
    offset: [f32; 2],
) -> std::collections::HashMap<crate::ObjectId, crate::ObjectId> {
    // Resolve destinations against the existing boards before adding the copied objects.
    let explicit_board = parent.and_then(|id| crate::artboard::owner_or_self(doc, id));
    let boards: Vec<_> = if parent.is_none() { crate::view::ViewTarget::artboards(doc).collect() } else { Vec::new() };
    if boards.is_empty() && explicit_board.is_none() {
        return paste_bundle_mapped(doc, bundle, parent, offset);
    }
    let roots: std::collections::HashSet<_> = bundle.roots.iter().copied().collect();
    let mut destinations = Vec::new();
    for source in bundle.objects.iter().filter(|object| roots.contains(&object.id) && !crate::artboard::is_artboard(object)) {
        let board = if parent.is_some() {
            explicit_board
        } else {
            let point = std::array::from_fn(|axis| source.pos[axis] + offset[axis]);
            boards.iter().rev().copied().find(|board| crate::artboard::contains(board, point)).filter(|board| !board.locked)
        };
        if let Some(board) = board {
            let index = crate::data_merge::layout::object_page_index(doc, board.id);
            let page = doc.pages.get(index).map(|page| page.id);
            destinations.push((source.id, parent.unwrap_or(board.id), page));
        }
    }

    let mapping = paste_bundle_mapped(doc, bundle, parent, offset);
    for (source, destination, page) in destinations {
        if let Some(&id) = mapping.get(&source)
            && let Some(object) = doc.find_object_mut(id.0)
        {
            object.parent_id = Some(destination);
            if let Some(page) = page {
                crate::artboard::edit::set_subtree_page(doc, id, page);
            }
        }
    }
    mapping
}

fn paste_objects(
    session: &mut crate::Session, name: &str, mut bundle: ClipboardBundle,
    parent: Option<crate::ObjectId>, at: Option<[f32; 2]>, default_offset: [f32; 2],
) -> crate::CommandResult {
    let factor = bundle.source_dimensions.map_or(1.0, |(units, dpi)| {
        crate::doc_setup::unit_factor(units, dpi, session.document.units, session.document.dpi)
    });
    if !factor.is_finite() || factor <= 0.0 {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    if factor != 1.0 {
        crate::doc_setup::transform_object_dimensions(&mut bundle.objects, factor, [factor; 2], |pos, size, _| {
            (pos * factor, size * factor)
        })?;
    }
    let offset = at.map_or(default_offset, |at| std::array::from_fn(|axis| {
        let min = bundle.objects.iter().filter(|object| bundle.roots.contains(&object.id))
            .map(|object| object.pos[axis]).fold(f32::INFINITY, f32::min);
        at[axis] - min
    }));
    if offset.iter().any(|value| !value.is_finite()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    if !bundle.symbols.is_empty() && symbol_clipboard::can_link_under(&session.document, parent) {
        let mut document = session.document.clone();
        let mapping = paste_into_artboards(&mut document, &bundle, parent, offset);
        symbol_clipboard::paste(&mut document, &bundle, &mapping, session.context, offset, factor)?;
        let ids: Vec<_> = bundle.roots.iter().filter_map(|id| mapping.get(id).copied()).collect();
        let extra = ids_json(&ids);
        return crate::commands::document_change::commit(session, name, document, ids, extra);
    }
    let before = crate::commands::arrange::snapshot(session);
    let mapping = paste_into_artboards(&mut session.document, &bundle, parent, offset);
    let ids = bundle.roots.iter().filter_map(|id| mapping.get(id).copied()).collect();
    commit_paste(session, name, before, ids)
}

fn paste(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if !session.has_document {
        return Err(dtool_protocol::CommandError::no_document());
    }
    let at: Option<[f32; 2]> = match command.params.get("at") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| dtool_protocol::CommandError::invalid_params())?),
    };
    if at.is_some_and(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let payload = session.clipboard.get_payload();
    if let Some(text) = paste_text(&payload) {
        return paste_plain_text(session, &text, at);
    }
    let bundle = payload_bundle(&payload).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    paste_objects(session, EDIT_PASTE, bundle, None, at, [PASTE_STEP; 2])
}

fn paste_special(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if !session.has_document {
        return Err(dtool_protocol::CommandError::no_document());
    }
    let payload = session.clipboard.get_payload();
    let formats = crate::command::paste_special_formats(&payload);
    let format = match command.params.get("format") {
        None | Some(serde_json::Value::Null) => formats.first().cloned().ok_or_else(dtool_protocol::CommandError::invalid_params)?,
        Some(serde_json::Value::String(format)) => format.clone(),
        Some(_) => return Err(dtool_protocol::CommandError::invalid_params()),
    };
    if !formats.contains(&format) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let at = command.params.get("at").cloned().unwrap_or(serde_json::Value::Null);
    if format == crate::command::PASTE_FORMAT_DTOOL {
        let params = if at.is_null() { serde_json::json!({}) } else { serde_json::json!({ "at": at }) };
        return paste(session, &dtool_protocol::Command { name: EDIT_PASTE.to_string(), params });
    }
    let at: Option<[f32; 2]> = serde_json::from_value(at).map_err(|_| dtool_protocol::CommandError::invalid_params())?;
    if at.is_some_and(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let text = paste_text(&payload).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    paste_plain_text(session, &text, at)
}

fn paste_plain_text(session: &mut crate::command::Session, text: &str, at: Option<[f32; 2]>) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if let Some(edit) = session.text_edit.clone() {
        let caret = edit.caret.min(edit.anchor);
        let insert = dtool_protocol::Command { name: "text.insert".to_string(), params: serde_json::json!({ "story": edit.story.0, "at": caret, "text": text }) };
        return session.run(&insert);
    }
    let pos = at.unwrap_or([0.0, 0.0]);
    let add = dtool_protocol::Command { name: "text.art.add".to_string(), params: serde_json::json!({ "text": text, "pos": pos }) };
    let mut reply = session.run(&add)?;
    if let Some(id) = reply.get("id").and_then(serde_json::Value::as_u64) {
        session.selection.set(vec![crate::object::ObjectId(id)], crate::selection::SelectOp::Replace);
        if let Some(map) = reply.as_object_mut() {
            map.insert("ids".to_string(), serde_json::json!([id]));
        }
    }
    Ok(reply)
}

fn paste_inside(session: &mut crate::command::Session) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if session.selection.ids.len() != 1 {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let container = session.selection.ids[0];
    if session.document.get_object(container).is_none() {
        return Err(dtool_protocol::CommandError::not_found());
    }
    if !is_group(session, container) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let bundle = payload_bundle(&session.clipboard.get_payload()).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    paste_objects(session, EDIT_PASTE_INSIDE, bundle, Some(container), None, [0.0; 2])
}

fn style_source(payload: &ClipboardPayload) -> Result<(Option<crate::paint::Paint>, Option<crate::paint::StrokeStyle>, Option<f32>), dtool_protocol::CommandError> {
    let invalid = dtool_protocol::CommandError::invalid_params;
    match payload.kind {
        ClipboardKind::Style => {
            let style = payload.style.as_ref().and_then(serde_json::Value::as_object).ok_or_else(invalid)?;
            let fill = style.get("fill").map(crate::paint::paint_from_param).transpose()?;
            let stroke = style.get("stroke").map(|v| serde_json::from_value::<crate::paint::StrokeStyle>(v.clone()).map_err(|_| invalid())).transpose()?;
            if stroke.as_ref().is_some_and(|s| !crate::paint::validate_stroke(s)) {
                return Err(invalid());
            }
            let opacity = match style.get("opacity") {
                None => None,
                Some(v) => {
                    let o = v.as_f64().ok_or_else(invalid)? as f32;
                    if !(0.0..=1.0).contains(&o) {
                        return Err(invalid());
                    }
                    Some(o)
                }
            };
            if fill.is_none() && stroke.is_none() && opacity.is_none() {
                return Err(invalid());
            }
            Ok((fill, stroke, opacity))
        }
        ClipboardKind::Objects => {
            let bundle = payload_bundle(payload).ok_or_else(invalid)?;
            let first = bundle.objects.iter().find(|o| bundle.roots.first() == Some(&o.id)).ok_or_else(invalid)?;
            Ok((Some(first.fill.clone()), Some(first.stroke.clone()), Some(first.opacity)))
        }
        _ => Err(invalid()),
    }
}

fn paste_style(session: &mut crate::command::Session) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    let roots = selected_roots(session);
    if roots.is_empty() {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let (fill, stroke, opacity) = style_source(&session.clipboard.get_payload())?;
    let before = crate::commands::arrange::snapshot(session);
    for id in &roots {
        if let Some(object) = session.document.find_object_mut(id.0) {
            if let Some(fill) = &fill {
                object.fill = fill.clone();
            }
            if let Some(stroke) = &stroke {
                object.stroke = stroke.clone();
            }
            if let Some(opacity) = opacity {
                object.opacity = opacity;
            }
        }
    }
    crate::commands::arrange::commit_snapshot(session, EDIT_PASTE_STYLE, before, ids_json(&roots))
}

fn paste_without_format(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if !session.has_document {
        return Err(dtool_protocol::CommandError::no_document());
    }
    let at: Option<[f32; 2]> = match command.params.get("at") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| dtool_protocol::CommandError::invalid_params())?),
    };
    if at.is_some_and(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let text = paste_text(&session.clipboard.get_payload()).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    paste_plain_text(session, &text, at)
}

pub fn box_centre(objects: &[Object]) -> [f32; 2] {
    if objects.is_empty() {
        return [0.0, 0.0];
    }
    let min = [objects.iter().map(|o| o.pos[0]).fold(f32::INFINITY, f32::min), objects.iter().map(|o| o.pos[1]).fold(f32::INFINITY, f32::min)];
    let max = [objects.iter().map(|o| o.pos[0] + o.size[0].abs()).fold(f32::NEG_INFINITY, f32::max), objects.iter().map(|o| o.pos[1] + o.size[1].abs()).fold(f32::NEG_INFINITY, f32::max)];
    [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5]
}

pub fn new_document_from_clipboard(workspace: &mut crate::workspace::Workspace) -> Result<crate::workspace::ContextId, dtool_protocol::CommandError> {
    let payload = workspace.app_clipboard().get_payload();
    let Some(text) = paste_text(&payload) else {
        let bundle = payload_bundle(&payload).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
        return new_document_from_bundle(workspace, &bundle);
    };
    let id = workspace.insert_context("", None, crate::document::Document::default(), 0);
    let session = &mut workspace.get_mut(id).ok_or_else(dtool_protocol::CommandError::context_not_found)?.session;
    let add = dtool_protocol::Command { name: "text.art.add".to_string(), params: serde_json::json!({ "text": text, "pos": [0.0, 0.0] }) };
    session.run(&add)?;
    session.undo = crate::undo::UndoStack::default();
    Ok(id)
}

/// New document sized to the copied geometry and any strokes that extend beyond it.
fn new_document_from_bundle(workspace: &mut crate::workspace::Workspace, bundle: &ClipboardBundle) -> Result<crate::workspace::ContextId, dtool_protocol::CommandError> {
    let roots: std::collections::HashSet<_> = bundle.roots.iter().copied().collect();
    let bounds = bundle.objects.iter().filter(|object| roots.contains(&object.id))
        .map(|object| bounds_rect(crate::geometry::object_world_bounds(object)))
        .filter(|rect| rect.is_finite())
        .reduce(|acc, rect| acc.union(rect))
        .ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    let (units, dpi) = bundle.source_dimensions.unwrap_or_else(|| {
        let default = crate::document::Document::default();
        (default.units, default.dpi)
    });
    let size = [bounds.width().max(1.0) as f32, bounds.height().max(1.0) as f32];
    let mut doc = crate::document::Document::new(size, units, dpi, 1);
    let placed_at = [-bounds.x0 as f32, -bounds.y0 as f32];
    let mapping = paste_bundle_mapped(&mut doc, bundle, None, placed_at);
    let bounds = include_clipboard_strokes(&doc, bounds.with_origin(kurbo::Point::ORIGIN));
    let adjustment = [-bounds.x0 as f32, -bounds.y0 as f32];
    for object in &mut doc.objects {
        object.pos[0] += adjustment[0];
        object.pos[1] += adjustment[1];
    }
    doc.pages[0].size = [bounds.width().max(1.0) as f32, bounds.height().max(1.0) as f32];
    let offset = [placed_at[0] + adjustment[0], placed_at[1] + adjustment[1]];
    symbol_clipboard::paste(&mut doc, bundle, &mapping, None, offset, 1.0)?;
    let ids: Vec<_> = bundle.roots.iter().filter_map(|id| mapping.get(id).copied()).collect();
    let id = workspace.insert_context("", None, doc, 0);
    let session = &mut workspace.get_mut(id).ok_or_else(dtool_protocol::CommandError::context_not_found)?.session;
    session.selection.set(ids, crate::selection::SelectOp::Replace);
    session.undo = crate::undo::UndoStack::default();
    Ok(id)
}

fn bounds_rect([x, y, w, h]: [f32; 4]) -> kurbo::Rect {
    kurbo::Rect::from_origin_size((f64::from(x), f64::from(y)), (f64::from(w), f64::from(h)))
}

fn include_clipboard_strokes(doc: &crate::Document, bounds: kurbo::Rect) -> kurbo::Rect {
    use crate::commands::appearance::{self, Kind};
    use crate::paint::Paint;
    use kurbo::Shape;

    doc.objects.iter().filter_map(|object| {
        if !appearance::supports_multiple(object)
            || !appearance::visible_strokes(object).any(|stroke| stroke.width > 0.0 && stroke.paint != Paint::None)
            || crate::clip_mask::is_mask(doc, object)
            || !crate::scene::is_drawn(doc, object.id)
        {
            return None;
        }
        let mut ink = appearance::vector_paths(doc, object, &Paint::None, false)?
            .into_iter()
            .filter(|path| path.slot.kind == Kind::Stroke && !path.blend.erases())
            .map(|path| path.path.bounding_box())
            .filter(|rect| rect.is_finite())
            .reduce(|acc, rect| acc.union(rect))?;
        if let Some(board) = crate::artboard::owner(doc, object.id) {
            ink = ink.intersect(bounds_rect(crate::geometry::object_world_bounds(board)));
        }
        if let Some(clip) = crate::clip_mask::clip_region(doc, object.id) {
            ink = ink.intersect(bounds_rect(crate::clip_mask::bounds(&clip)?));
        }
        (ink.width() > 0.0 && ink.height() > 0.0).then_some(ink)
    }).fold(bounds, |acc, rect| acc.union(rect))
}

pub fn run_workspace(workspace: &mut crate::workspace::Workspace, scope: crate::workspace::Scope, command: &dtool_protocol::Command) -> Option<(Option<crate::workspace::ContextId>, dtool_protocol::CommandResult)> {
    if command.name != DOC_NEW_FROM_CLIPBOARD {
        return None;
    }
    let result = new_document_from_clipboard(workspace);
    Some(match result {
        Ok(id) => {
            if let Some(context) = workspace.get_mut(id) {
                context.initial_view_pending = true;
            }
            (Some(id), Ok(serde_json::json!({ "context": id.0, "path": null })))
        }
        Err(err) => (scope.context.or_else(|| workspace.active()), Err(err)),
    })
}

fn paste_fx(session: &mut crate::command::Session) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    let roots = selected_roots(session);
    let payload = session.clipboard.get_payload();
    let effects: Vec<crate::layer_style::LayerEffect> = match payload.kind {
        ClipboardKind::Effects => payload.effects.clone().and_then(|value| serde_json::from_value(value).ok()),
        ClipboardKind::Objects => payload.objects.as_ref().and_then(|objects| objects.first()).map(|object| object.effects.clone()),
        _ => None,
    }
    .ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    if roots.is_empty() || !effects.iter().all(|effect| effect.is_valid()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    if roots.iter().all(|id| session.document.get_object(*id).is_some_and(|object| object.effects == effects)) {
        return Ok(serde_json::json!({ "doc_version": session.generation, "changes": [] }));
    }
    let before = crate::commands::arrange::snapshot(session);
    for id in &roots {
        if let Some(object) = session.document.find_object_mut(id.0) {
            object.effects = effects.clone();
        }
    }
    let mut extra = ids_json(&roots);
    extra["effects_applied"] = serde_json::Value::Bool(true);
    crate::commands::arrange::commit_snapshot(session, EDIT_PASTE_FX, before, extra)
}

fn duplicate(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if command.params.get("mode").and_then(serde_json::Value::as_str) == Some("Linked") {
        return Err(dtool_protocol::CommandError::not_implemented());
    }
    let roots = selected_roots(session);
    let offset = if let [id] = roots.as_slice() {
        session.document.get_object(*id).filter(|object| object.kind == crate::ObjectKind::Artboard)
            .map(|board| crate::artboard::edit::duplicate_offset(&session.document, board))
            .unwrap_or_default()
    } else {
        [0.0, 0.0]
    };
    // Preserve the free-space placement of an ordinary artboard duplicate.
    let can_repeat = !session.last_duplicate_steps.is_empty()
        || (offset == [0.0, 0.0] && session.last_transform_delta.is_some());
    if can_repeat && !roots.is_empty() && roots == session.last_duplicate_ids
        && session.duplicate_revision == Some(session.undo.revision())
    {
        return power_duplicate(session, EDIT_DUPLICATE);
    }
    duplicate_roots(session, EDIT_DUPLICATE, &roots, offset)
}

pub fn duplicate_selection(session: &mut crate::command::Session, name: &str, offset: [f32; 2]) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    let roots = selected_roots(session);
    duplicate_roots(session, name, &roots, offset)
}

fn copy_duplicate_roots(doc: &mut crate::Document, roots: &[crate::ObjectId], offset: [f32; 2]) -> Vec<crate::ObjectId> {
    let bundle = collect_bundle_with_guides(doc, roots, true);
    let mapping = paste_bundle_mapped(doc, &bundle, None, offset);
    let ids = bundle.roots.iter().filter_map(|original| {
        let id = *mapping.get(original)?;
        let parent = doc.get_object(*original).and_then(|object| object.parent_id);
        crate::artboard::edit::name_duplicate(doc, id);
        if let Some(object) = doc.find_object_mut(id.0) {
            object.parent_id = parent;
        }
        Some(id)
    }).collect();
    symbol_clipboard::duplicate(doc, &mapping, offset);
    ids
}

pub(crate) fn duplicate_roots(session: &mut crate::command::Session, name: &str, roots: &[crate::ObjectId], offset: [f32; 2]) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if roots.is_empty() {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let before = crate::commands::arrange::snapshot(session);
    let ids = copy_duplicate_roots(&mut session.document, roots, offset);
    session.last_duplicate_ids = ids.clone();
    session.last_transform_delta = (offset != [0.0, 0.0]).then_some(offset);
    session.last_duplicate_steps.clear();
    let result = commit_paste(session, name, before, ids)?;
    session.duplicate_revision = Some(session.undo.revision());
    Ok(result)
}

fn power_duplicate(session: &mut crate::command::Session, name: &str) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    if session.last_transform_delta.is_none() && session.last_duplicate_steps.is_empty() {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let delta = session.last_transform_delta.unwrap_or([0.0, 0.0]);
    let sources: Vec<crate::object::ObjectId> = session.last_duplicate_ids.clone();
    if sources.is_empty() || sources.iter().any(|id| session.document.get_object(*id).is_none()) {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let before = crate::commands::arrange::snapshot(session);
    let ids = copy_duplicate_roots(&mut session.document, &sources, delta);
    for mut step in session.last_duplicate_steps.clone() {
        let objects: Vec<Object> = ids.iter().filter_map(|id| session.document.get_object(*id).cloned()).collect();
        let centre = box_centre(&objects);
        step.origin = [step.origin[0] + centre[0], step.origin[1] + centre[1]];
        if let Err(err) = crate::command::execute_transform_selection_box(&mut session.document, &ids, &step) {
            crate::commands::arrange::restore(session, &before);
            return Err(err);
        }
    }
    session.last_duplicate_ids = ids.clone();
    let result = commit_paste(session, name, before, ids)?;
    session.duplicate_revision = Some(session.undo.revision());
    Ok(result)
}
