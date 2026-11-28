use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::data_merge::layout::{clone_template, page_rect};
use crate::document::PageId;
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, ObjectKind, Session};

pub(super) mod spread_flow;
mod placement;
mod conform;
mod page_move;
pub(super) use page_move::update_after_move;
pub use page_move::PageMoveMasterPolicy;
pub use placement::{MasterPlacement, ScaleMode};
use placement::Projection;

const CREATE: &str = "master.create";
const UPDATE: &str = "master.update";
const DUPLICATE: &str = "master.duplicate";
const DELETE: &str = "master.delete";
const DELETE_UNUSED: &str = "master.delete_unused";
const LIST: &str = "master.list";
const APPLY: &str = "master.apply";
const RESET: &str = "master.object.reset";
const DETACH: &str = "master.detach";
const REMOVE: &str = "master.remove";
const SET_BASE: &str = "master.base.set";
const SET_TAG: &str = "master.tag.set";
const STORY_TEXT_OVERRIDE: &str = "story.text";
const LOCAL_POSITION_OVERRIDE: &str = "pos";
const PREFIX: &str = "마스터_";
const GROUP_BASIS: [f32; 2] = [1.0, 1.0];

pub const MAX_MASTER_PAGES: usize = 12;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MasterPages {
    pub roots: Vec<ObjectId>,
    pub assignments: Vec<Assignment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub master_assignments: Vec<MasterAssignment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spreads: Vec<MasterSpread>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) tag_colours: BTreeMap<u64, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) page_setup: BTreeMap<u64, MasterPageSetup>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterPageSetup {
    pub margins: crate::document::SetupMargins,
    pub margin_colour: Option<String>,
    pub drawing_scale: Option<[f32; 2]>,
    pub measurement_unit: crate::units::ReadoutUnit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MasterSpread {
    pub pages: Vec<ObjectId>,
    /// Number of pages on the leading side of the spine.
    pub spine: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub page: PageId,
    #[serde(default, skip_serializing_if = "zero_slot")]
    pub stack_slot: u32,
    #[serde(flatten)]
    pub inherited: InheritedMaster,
}

fn zero_slot(slot: &u32) -> bool { *slot == 0 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MasterAssignment {
    pub target: ObjectId,
    #[serde(flatten)]
    pub inherited: InheritedMaster,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InheritedMaster {
    pub master: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<ObjectId>,
    /// Untransformed size of the layer's invisible reference box in document units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) group_basis: Option<[f32; 2]>,
    pub links: Vec<(ObjectId, ObjectId)>,
    #[serde(default, skip_serializing_if = "MasterPlacement::is_default")]
    pub placement: MasterPlacement,
    /// Overrides that would otherwise become indistinguishable from inherited values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "deserialize_override_keys")]
    pub(crate) matching_overrides: BTreeMap<u64, BTreeSet<String>>,
}

fn deserialize_override_keys<'de, D>(input: D) -> Result<BTreeMap<u64, BTreeSet<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // JSON stores these IDs as strings, including when serde buffers flattened fields.
    BTreeMap::<String, BTreeSet<String>>::deserialize(input)?.into_iter()
        .map(|(id, fields)| Ok((id.parse().map_err(serde::de::Error::custom)?, fields)))
        .collect()
}

impl InheritedMaster {
    fn new(master: ObjectId) -> Self {
        Self { master, group: None, group_basis: None, links: Vec::new(),
            placement: MasterPlacement::default(), matching_overrides: BTreeMap::new() }
    }
}

impl std::ops::Deref for Assignment {
    type Target = InheritedMaster;
    fn deref(&self) -> &Self::Target { &self.inherited }
}

impl std::ops::DerefMut for Assignment {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inherited }
}

impl MasterPages {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty() && self.assignments.is_empty() && self.master_assignments.is_empty()
            && self.spreads.is_empty() && self.tag_colours.is_empty() && self.page_setup.is_empty()
    }

    pub fn spread(&self, page: ObjectId) -> Option<&MasterSpread> {
        self.spreads.iter().find(|spread| spread.pages.contains(&page))
    }

    pub fn spread_id(&self, page: ObjectId) -> ObjectId {
        self.spread(page).and_then(|spread| spread.pages.first().copied()).unwrap_or(page)
    }

    pub fn spread_pages<'a>(&'a self, page: &'a ObjectId) -> &'a [ObjectId] {
        self.spread(*page).map_or_else(|| std::slice::from_ref(page), |spread| spread.pages.as_slice())
    }

    pub fn spread_roots(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.roots.iter().copied().filter(|&id| self.spread_id(id) == id)
    }

    pub fn tag_colour(&self, page: ObjectId) -> Option<&str> {
        self.tag_colours.get(&self.spread_id(page).0).map(String::as_str)
    }

    pub fn page_setup(&self, page: ObjectId) -> Option<&MasterPageSetup> {
        self.page_setup.get(&self.spread_id(page).0)
    }

    pub fn has_tag_colours(&self) -> bool {
        !self.tag_colours.is_empty()
    }

    fn base_pairs(&self, target: ObjectId, base: Option<ObjectId>) -> Vec<(ObjectId, Option<ObjectId>)> {
        let logical = self.spread_id(target) == target;
        let targets = if logical { self.spread_pages(&target) } else { std::slice::from_ref(&target) };
        let target_spine = self.spread(target).map_or(1, |spread| spread.spine);
        targets.iter().enumerate().map(|(index, &id)| {
            let source = base.map(|source| {
                if !logical || self.spread_id(source) != source {
                    return source;
                }
                let Some(spread) = self.spread(source) else { return source; };
                let slot = (spread.spine + index).saturating_sub(target_spine).min(spread.pages.len() - 1);
                spread.pages[slot]
            });
            (id, source)
        }).collect()
    }

    pub fn can_set_spread_base(&self, target: ObjectId, base: ObjectId) -> bool {
        if self.spread_id(target) == self.spread_id(base) {
            return false;
        }
        self.base_pairs(target, Some(base)).into_iter()
            .all(|(target, base)| base.is_some_and(|base| self.can_set_base(target, base)))
    }

    pub fn source_of(&self, object: ObjectId) -> Option<ObjectId> {
        self.inherited_content().flat_map(|item| &item.links)
            .find_map(|&(source, instance)| (instance == object).then_some(source))
    }

    pub fn for_page(&self, page: PageId) -> Option<&Assignment> {
        self.on_page(page).next()
    }

    pub fn on_page(&self, page: PageId) -> impl Iterator<Item = &Assignment> {
        self.assignments.iter().filter(move |item| item.page == page)
    }

    pub fn for_group(&self, group: ObjectId) -> Option<&Assignment> {
        self.assignments.iter().find(|item| item.group == Some(group))
    }

    pub fn for_master(&self, id: ObjectId) -> Option<&MasterAssignment> {
        self.master_assignments.iter().find(|item| item.target == id)
    }

    pub fn inherited_content(&self) -> impl Iterator<Item = &InheritedMaster> {
        self.assignments.iter().map(|item| &item.inherited)
            .chain(self.master_assignments.iter().map(|item| &item.inherited))
    }

    fn inherited_content_mut(&mut self) -> impl Iterator<Item = &mut InheritedMaster> {
        self.assignments.iter_mut().map(|item| &mut item.inherited)
            .chain(self.master_assignments.iter_mut().map(|item| &mut item.inherited))
    }

    pub fn can_set_base(&self, target: ObjectId, base: ObjectId) -> bool {
        if target == base || !self.roots.contains(&target) || !self.roots.contains(&base) {
            return false;
        }
        let mut visited = HashSet::from([target]);
        let mut current = Some(base);
        while let Some(id) = current {
            if !visited.insert(id) {
                return false;
            }
            current = self.for_master(id).map(|item| item.inherited.master);
        }
        true
    }
}

#[derive(Clone, Copy)]
enum TargetSurface {
    Page(PageId),
    Master(ObjectId),
}

impl TargetSurface {
    fn page(self) -> Option<PageId> {
        match self { Self::Page(id) => Some(id), Self::Master(_) => None }
    }

    fn origin(self, doc: &Document) -> Option<[f32; 2]> {
        self.bounds(doc).map(|[x, y, _, _]| [x, y])
    }

    fn bounds(self, doc: &Document) -> Option<[f32; 4]> {
        match self {
            Self::Page(id) => {
                let index = doc.pages.iter().position(|page| page.id == id)?;
                Some(page_rect(doc, index))
            }
            Self::Master(id) => doc.get_object(id).map(|root| [root.pos[0], root.pos[1], root.size[0], root.size[1]]),
        }
    }

    fn layer_matrix(self, doc: &Document, assignment: &InheritedMaster) -> Option<[f32; 6]> {
        let group = doc.get_object(assignment.group?)?;
        let basis = assignment.group_basis?;
        let origin = self.origin(doc)?;
        placement::layer_matrix(group, basis, origin)
    }

    fn projection(self, doc: &Document, assignment: &InheritedMaster) -> Option<Projection> {
        self.projection_with_layer(doc, assignment, self.layer_matrix(doc, assignment))
    }

    fn projection_with_layer(
        self, doc: &Document, assignment: &InheritedMaster, layer: Option<[f32; 6]>,
    ) -> Option<Projection> {
        if assignment.placement.is_default() && layer.is_none() {
            return None;
        }
        let [x, y, w, h] = self.bounds(doc)?;
        let root = doc.get_object(assignment.master)?;
        let projection = assignment.placement.projection(root, [x, y, w, h])?;
        Some(layer.map_or(projection, |layer| projection.with_layer(layer, [x, y])))
    }

    fn binding<'a>(self, doc: &'a Document, inherited: &InheritedMaster) -> Option<&'a InheritedMaster> {
        match self {
            Self::Page(id) => doc.master_pages.on_page(id).find(|old| {
                old.group == inherited.group && old.master == inherited.master
            }).map(|assignment| &assignment.inherited),
            Self::Master(id) => doc.master_pages.for_master(id).map(|assignment| &assignment.inherited),
        }
    }

    fn parent(self, doc: &Document) -> Option<ObjectId> {
        match self {
            Self::Page(id) => doc.linked_artboard(id).map(|board| board.id),
            Self::Master(id) => Some(id),
        }
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

pub fn unused_master_spreads(doc: &Document) -> Vec<ObjectId> {
    let masters = &doc.master_pages;
    if masters.roots.is_empty() {
        return Vec::new();
    }
    let mut pending: Vec<_> = masters.assignments.iter().map(|item| masters.spread_id(item.master)).collect();
    let mut dependencies: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for item in &masters.master_assignments {
        dependencies.entry(masters.spread_id(item.target)).or_default().push(masters.spread_id(item.inherited.master));
    }
    // A path-text object can reference a path on a different master without a master assignment.
    let owner = |id| crate::artboard::owner_or_self(doc, id)
        .filter(|board| masters.roots.contains(&board.id)).map(|board| masters.spread_id(board.id));
    for object in &doc.objects {
        if let Some(path) = object.path_text_data().and_then(|data| owner(data.path)) {
            if let Some(target) = owner(object.id) {
                dependencies.entry(target).or_default().push(path);
            } else {
                pending.push(path);
            }
        }
    }
    let mut used = HashSet::new();
    while let Some(master) = pending.pop() {
        if used.insert(master) && let Some(bases) = dependencies.get(&master) {
            pending.extend(bases);
        }
    }
    masters.spread_roots().filter(|id| !used.contains(id)).collect()
}

pub fn fill_clips(doc: &Document) -> HashMap<ObjectId, [f32; 4]> {
    let mut clips = HashMap::new();
    if doc.master_pages.assignments.iter().any(|assignment| assignment.placement.scale == ScaleMode::Fill) {
        let rects: HashMap<_, _> = doc.pages.iter().map(|page| page.id).zip(crate::scene::page_rects(doc)).collect();
        for assignment in doc.master_pages.assignments.iter().filter(|item| item.placement.scale == ScaleMode::Fill) {
            if let Some(&rect) = rects.get(&assignment.page) {
                clips.extend(assignment.links.iter().map(|&(_, instance)| (instance, rect)));
            }
        }
    }
    clips
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), CREATE | UPDATE | DUPLICATE | DELETE | DELETE_UNUSED | LIST | APPLY | RESET | DETACH | REMOVE | SET_BASE | SET_TAG)
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
            DUPLICATE => duplicate(session, command),
            DELETE => delete(session, command),
            DELETE_UNUSED => {
                let unused = unused_master_spreads(&session.document);
                if unused.is_empty() {
                    Ok(json!({"ids": unused}))
                } else {
                    delete_spreads(session, DELETE_UNUSED, &unused, json!({"ids": unused}))
                }
            }
            LIST => Ok(list(&session.document)),
            APPLY => apply(session, command),
            RESET => reset(session, command),
            SET_BASE => set_base(session, command),
            SET_TAG => set_tag(session, command),
            _ => detach(session, command),
        }
    })
}

fn list(doc: &Document) -> Value {
    let page = |root: &Object| {
        json!({"id": root.id, "name": master_name(root), "size": root.size, "origin": root.pos})
    };
    let masters: Vec<_> = doc.master_pages.spread_roots().filter_map(|id| doc.get_object(id)).map(|root| {
        let mut value = page(root);
        let pages: Vec<_> = doc.master_pages.spread_pages(&root.id).iter().filter_map(|&id| doc.get_object(id)).map(page).collect();
        value["pages"] = json!(pages);
        value["spine"] = json!(doc.master_pages.spread(root.id).map(|spread| spread.spine));
        value["tag_colour"] = json!(doc.master_pages.tag_colour(root.id));
        value["page_setup"] = json!(doc.master_pages.page_setup(root.id));
        value
    }).collect();
    json!({"masters": masters, "assignments": doc.master_pages.assignments, "master_assignments": doc.master_pages.master_assignments})
}

fn read_id(params: &Value, key: &str) -> Result<u64, CommandError> {
    crate::text_cmd::param_u64(params, key)
}

fn set_tag(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    let id = ObjectId(read_id(&command.params, "id")?);
    if !before.master_pages.roots.contains(&id) || before.get_object(id).is_none() {
        return Err(CommandError::not_found());
    }
    let id = before.master_pages.spread_id(id);
    let colour = super::layer_meta::read_tag_colour(&command.params)?;
    if before.master_pages.tag_colour(id) == colour.as_deref() {
        return Ok(json!({"id": id, "tag_colour": colour, "doc_version": session.generation, "changes": []}));
    }
    let mut doc = before.clone();
    if let Some(colour) = &colour {
        doc.master_pages.tag_colours.insert(id.0, colour.clone());
    } else {
        doc.master_pages.tag_colours.remove(&id.0);
    }
    super::document_change::commit(session, SET_TAG, doc, session.selection.ids.clone(),
        json!({"id": id, "tag_colour": colour}))
}

fn options(doc: &Document, params: &Value, id: Option<ObjectId>) -> Result<(String, [f32; 2]), CommandError> {
    let name = params["name"].as_str().ok_or_else(CommandError::invalid_params)?.trim();
    if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err(CommandError::invalid_params());
    }
    if doc.master_pages.spread_roots().filter(|root| Some(*root) != id).any(|root| {
        doc.get_object(root).is_some_and(|object| master_name(object) == name)
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

pub(crate) fn physical_name(name: &str, index: usize) -> String {
    if index == 0 { name.to_owned() } else { format!("{name} · {}", index + 1) }
}

fn create(session: &mut Session, command: &Command) -> CommandResult {
    let (name, size) = options(&session.document, &command.params, None)?;
    let tag_colour = command.params.get("colour").map(|_| super::layer_meta::read_tag_colour(&command.params))
        .transpose()?.flatten();
    let setup = command.params.get("page_setup").map(|value| {
        let setup: MasterPageSetup = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        let m = &setup.margins;
        if [m.left, m.top, m.right, m.bottom].iter().any(|v| !v.is_finite() || *v < 0.0)
            || setup.margin_colour.as_ref().is_some_and(|s| {
                s.len() != 7 || !s.starts_with('#') || !s.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            }) {
            return Err(CommandError::invalid_params());
        }
        if let Some(scale) = setup.drawing_scale {
            Document::validate_drawing_scale(scale)?;
        }
        Ok::<_, CommandError>(setup)
    }).transpose()?;
    let sizes = if let Some(sizes) = command.params.get("page_sizes") {
        if command.params.get("page_count").is_some() || command.params.get("size").is_some() {
            return Err(CommandError::invalid_params());
        }
        serde_json::from_value::<Vec<[f32; 2]>>(sizes.clone()).map_err(|_| CommandError::invalid_params())?
    } else {
        let count = command.params.get("page_count").map(|value| value.as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|count| (1..=MAX_MASTER_PAGES).contains(count)).ok_or_else(CommandError::invalid_params))
            .transpose()?.unwrap_or(1);
        vec![size; count]
    };
    if !(1..=MAX_MASTER_PAGES).contains(&sizes.len())
        || ((sizes.len() > 1 || command.params.get("page_sizes").is_some())
            && !sizes.iter().copied().all(super::pages::valid_page_size))
    {
        return Err(CommandError::invalid_params());
    }
    let spine = command.params.get("spine").map(|value| value.as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|spine| (1..sizes.len()).contains(spine)).ok_or_else(CommandError::invalid_params))
        .transpose()?.unwrap_or(sizes.len() / 2);
    let mut doc = session.document.clone();
    let pages: Vec<_> = sizes.into_iter().enumerate().map(|(index, size)| doc.create_object(Object {
        name: physical_name(&name, index), size, kind: ObjectKind::Artboard,
        blend_mode: crate::blend::BlendMode::PassThrough, ..Object::default()
    })).collect();
    let id = pages[0];
    doc.master_pages.roots.extend(&pages);
    if let Some(colour) = tag_colour {
        doc.master_pages.tag_colours.insert(id.0, colour);
    }
    if let Some(setup) = setup {
        doc.master_pages.page_setup.insert(id.0, setup);
    }
    if pages.len() > 1 {
        doc.master_pages.spreads.push(MasterSpread { pages: pages.clone(), spine });
    }
    arrange_masters(&mut doc);
    super::document_change::commit(session, CREATE, doc, vec![id], json!({"id": id, "page_ids": pages}))
}

fn update(session: &mut Session, command: &Command) -> CommandResult {
    let requested = ObjectId(read_id(&command.params, "id")?);
    let mut doc = session.document.clone();
    if !doc.master_pages.roots.contains(&requested) {
        return Err(CommandError::not_found());
    }
    let id = doc.master_pages.spread_id(requested);
    let (name, size) = options(&doc, &command.params, Some(id))?;
    let pages = doc.master_pages.spread_pages(&id).to_vec();
    let sizes = if let Some(sizes) = command.params.get("page_sizes") {
        if command.params.get("size").is_some() {
            return Err(CommandError::invalid_params());
        }
        let sizes: Vec<[f32; 2]> = serde_json::from_value(sizes.clone()).map_err(|_| CommandError::invalid_params())?;
        if sizes.len() != pages.len() || !sizes.iter().copied().all(super::pages::valid_page_size) {
            return Err(CommandError::invalid_params());
        }
        sizes
    } else if command.params.get("size").is_some() {
        vec![size; pages.len()]
    } else {
        pages.iter().map(|&id| doc.get_object(id).map(|root| root.size).ok_or_else(CommandError::not_found))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut changed = false;
    let mut resized_fill = false;
    for (index, (&page, size)) in pages.iter().zip(sizes).enumerate() {
        let root = doc.find_object_mut(page.0).ok_or_else(CommandError::not_found)?;
        let name = physical_name(&name, index);
        if root.name == name && root.size == size {
            continue;
        }
        changed = true;
        let before = crate::geometry::object_world_bounds(root);
        root.name.clone_from(&name);
        root.size = size;
        let after = crate::geometry::object_world_bounds(root);
        resized_fill |= super::layer_kinds::follow_surface_resize(&mut doc, Some(page), before, after);
        let groups: Vec<_> = doc.master_pages.inherited_content()
            .filter(|assignment| assignment.master == page)
            .filter_map(|assignment| assignment.group)
            .collect();
        for group in groups {
            if let Some(folder) = doc.find_object_mut(group.0) {
                folder.name.clone_from(&name);
            }
        }
    }
    if !changed {
        return Ok(json!({"id": id, "changes": [], "doc_version": session.generation}));
    }
    arrange_masters(&mut doc);
    if resized_fill || !doc.master_pages.master_assignments.is_empty() {
        inherit_changes(&session.document, &mut doc, command);
    }
    super::document_change::commit(session, UPDATE, doc, session.selection.ids.clone(), json!({"id": id}))
}

fn duplicate(session: &mut Session, command: &Command) -> CommandResult {
    let before = &session.document;
    let original = before.master_pages.spread_id(ObjectId(read_id(&command.params, "id")?));
    let source_pages = before.master_pages.spread_pages(&original);
    let index = before.master_pages.roots.iter().rposition(|id| source_pages.contains(id))
        .ok_or_else(CommandError::not_found)?;
    let root = before.get_object(original).ok_or_else(CommandError::not_found)?;
    let names: Vec<_> = before.master_pages.spread_roots().filter_map(|id| before.get_object(id))
        .map(|object| master_name(object).to_owned()).collect();
    let base: String = master_name(root).chars().take(112).collect();
    let name = crate::longdoc::helpers::unique_name(&names, &base);
    let members = source_pages.iter().flat_map(|&page| before.subtree_ids(page)).collect();
    let bundle = crate::clipboard::collect_bundle_members(before, &members);
    let mut doc = before.clone();
    let map = crate::clipboard::copy_bundle_in_document(&mut doc, &bundle, [0.0; 2]);
    let pages = source_pages.iter().map(|id| map.get(id).copied().ok_or_else(CommandError::not_found))
        .collect::<Result<Vec<_>, _>>()?;
    let id = pages[0];
    doc.master_pages.roots.splice(index + 1..index + 1, pages.iter().copied());
    if let Some(colour) = before.master_pages.tag_colour(original) {
        doc.master_pages.tag_colours.insert(id.0, colour.to_owned());
    }
    if let Some(setup) = before.master_pages.page_setup(original) {
        doc.master_pages.page_setup.insert(id.0, setup.clone());
    }
    if let Some(spread) = before.master_pages.spread(original) {
        doc.master_pages.spreads.push(MasterSpread { pages: pages.clone(), spine: spread.spine });
    }
    for (index, (&source, &target)) in source_pages.iter().zip(&pages).enumerate() {
        let copy = doc.find_object_mut(target.0).ok_or_else(CommandError::not_found)?;
        copy.name = physical_name(&format!("{PREFIX}{name}"), index);
        if let Some(previous) = before.master_pages.for_master(source) {
            let mut inherited = previous.inherited.clone();
            inherited.group = inherited.group.and_then(|group| map.get(&group).copied());
            inherited.links.retain_mut(|(_, instance)| match map.get(instance) {
                Some(&copy) => { *instance = copy; true }
                None => false,
            });
            doc.master_pages.master_assignments.push(MasterAssignment { target, inherited });
        }
    }
    crate::longdoc::page_import::copy_document_metadata(before, &mut doc, &map, &bundle, 1.0);
    arrange_masters(&mut doc);
    let same_pages = HashMap::new();
    crate::commands::symbols::page_import::import(before, &mut doc, &map, &same_pages, None)?;
    crate::longdoc::page_import::navigation::copy(before, before, &mut doc, &bundle, &map, &[],
        crate::longdoc::page_import::navigation::PageTransfer::Duplicate(&same_pages))?;
    doc.validate_refs().map_err(CommandError::from)?;
    let mut guides = session.guides.clone();
    for guide in session.guides.list().iter().filter(|guide| guide.master_id.is_some_and(|id| source_pages.contains(&id))) {
        let mut copy = guide.clone();
        copy.id = guides.next_id();
        copy.master_id = guide.master_id.and_then(|id| map.get(&id).copied());
        guides.insert(copy).map_err(|_| CommandError::invalid_params())?;
    }
    super::document_change::commit_guides(session, DUPLICATE, doc, vec![id], guides, json!({"id": id, "page_ids": pages}))
}

fn delete(session: &mut Session, command: &Command) -> CommandResult {
    let id = ObjectId(read_id(&command.params, "id")?);
    let before = &session.document;
    if !before.master_pages.roots.contains(&id) || before.get_object(id).is_none() {
        return Err(CommandError::not_found());
    }
    let id = before.master_pages.spread_id(id);
    delete_spreads(session, DELETE, &[id], json!({"id": id}))
}

fn delete_spreads(session: &mut Session, command: &str, ids: &[ObjectId], response: Value) -> CommandResult {
    let before = &session.document;
    let pages: HashSet<_> = ids.iter().flat_map(|id| before.master_pages.spread_pages(id).iter().copied()).collect();
    let doomed = pages.iter().flat_map(|&page| before.subtree_ids(page)).collect();
    let mut doc = before.clone();
    doc.master_pages.roots.retain(|root| !pages.contains(root));
    doc.master_pages.tag_colours.retain(|id, _| !pages.contains(&ObjectId(*id)));
    doc.master_pages.page_setup.retain(|id, _| !pages.contains(&ObjectId(*id)));
    doc.master_pages.spreads.retain(|spread| !spread.pages.first().is_some_and(|id| pages.contains(id)));
    doc.master_pages.assignments.retain(|assignment| !pages.contains(&assignment.master));
    doc.master_pages.master_assignments.retain(|assignment| !pages.contains(&assignment.target) && !pages.contains(&assignment.inherited.master));
    super::pages::remove_layout_objects(&mut doc, &doomed);
    arrange_masters(&mut doc);
    doc.validate_refs().map_err(|_| CommandError::new("PAGE_OBJECT_REFERENCED"))?;
    let selection = session.selection.ids.iter().copied().filter(|object| !doomed.contains(object)).collect();
    let mut guides = session.guides.clone();
    guides.items.retain(|guide| !guide.master_id.is_some_and(|id| pages.contains(&id)));
    super::document_change::commit_guides(session, command, doc, selection, guides, response)
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
        let end = doc.master_pages.spread(id).is_none_or(|spread| spread.pages.last() == Some(&id));
        x += root.size[0] + if end { gap } else { 0.0 };
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
    let add = flag("add")?;
    let keep_previous = flag("keep_previous")?;
    let conform_layout = flag("conform_layout")?;
    let conform_spread = flag("conform_spread")?;
    let placement = command.params.get("placement").map(|value| {
        serde_json::from_value::<MasterPlacement>(value.clone()).map_err(|_| CommandError::invalid_params())
    }).transpose()?;
    let selected_group = command.params.get("group_id").map(|_| read_id(&command.params, "group_id").map(ObjectId)).transpose()?;
    if (keep_previous && !replace) || (add && replace)
        || (selected_group.is_some() && (targets.len() != 1 || add || replace || keep_previous))
    {
        return Err(CommandError::invalid_params());
    }
    let mut doc = session.document.clone();
    let source_page = command.params.get("source_page").map(|value| {
        value.as_u64().and_then(|value| usize::try_from(value).ok())
            .and_then(|value| value.checked_sub(1)).ok_or_else(CommandError::invalid_params)
    }).transpose()?;
    if conform_spread && (add || selected_group.is_some() || source_page.is_some()) {
        return Err(CommandError::invalid_params());
    }
    let (targets, sources, inserted, topology_changed) = if conform_spread {
        let plan = conform::ConformPlan::new(&session.document, id, &targets)?;
        if plan.conflict && !replace {
            return Err(CommandError::new("MASTER_ALREADY_APPLIED"));
        }
        let result = plan.apply(&session.document, &mut doc)?;
        (result.targets, result.sources, result.inserted, result.topology_changed)
    } else if let Some(group) = selected_group {
        let binding = session.document.master_pages.for_group(group).filter(|item| item.page == targets[0].1)
            .ok_or_else(CommandError::not_found)?;
        if source_page.is_some() || binding.master != id {
            return Err(CommandError::invalid_params());
        }
        (targets, vec![id], Vec::new(), false)
    } else {
        let sources = application_sources(&session.document, id, &targets, source_page)?;
        (targets, sources, Vec::new(), false)
    };
    let slots = if add { added_slots(&session.document, &targets, &sources)? } else { vec![0; targets.len()] };
    let page_ids: Vec<_> = targets.iter().map(|&(_, page)| page).collect();
    let mut extra = json!({"master_id": id, "page_id": page_ids[0], "page_ids": page_ids});
    if conform_spread {
        extra["inserted_page_ids"] = json!(inserted);
    }
    let resizes: Vec<_> = if conform_layout || conform_spread {
        let rects = crate::scene::page_rects(&doc);
        targets.iter().zip(&sources).filter_map(|(&(index, page), &source)| {
            let size = session.document.get_object(source)?.size;
            let [_, _, width, height] = rects[index];
            ([width, height] != size).then_some((index, page, size))
        }).collect()
    } else {
        Vec::new()
    };
    if !resizes.is_empty() {
        if topology_changed {
            let before_resize = doc.clone();
            super::pages::resize_surfaces(&before_resize, &mut doc, &resizes)?;
        } else {
            super::pages::resize_surfaces(&session.document, &mut doc, &resizes)?;
        }
    }
    if topology_changed || !resizes.is_empty() {
        arrange_masters(&mut doc);
    }
    let resized: HashSet<_> = resizes.iter().map(|&(_, id, _)| id).collect();
    let mut changed = Vec::new();
    let mut placement_changed = false;
    let mut group_ids = Vec::new();
    for (((_, page), source), slot) in targets.into_iter().zip(sources).zip(slots) {
        let same = !add && (!replace || doc.master_pages.on_page(page).count() == 1);
        if same && let Some(current) = doc.master_pages.assignments.iter_mut().find(|current| {
            current.page == page && current.master == source && selected_group.is_none_or(|group| current.group == Some(group))
        }) {
            let reposition = placement.is_some_and(|placement| placement != current.placement);
            if let Some(placement) = placement {
                current.inherited.placement = placement;
            }
            placement_changed |= reposition;
            if resized.contains(&page) || reposition || topology_changed {
                changed.push(page);
            }
            group_ids.extend(current.group);
            continue;
        }
        let (previous, group) = apply_on_page(&mut doc, source, page, add, replace,
            keep_previous, placement.unwrap_or_default(), slot)?;
        group_ids.extend(group);
        if page_ids.len() == 1 && let Some(previous) = previous {
            extra["previous_master_id"] = json!(previous);
            extra["kept_previous"] = json!(keep_previous);
        }
        changed.push(page);
    }
    extra["group_ids"] = json!(group_ids);
    extra["changed_page_ids"] = json!(changed);
    if changed.is_empty() {
        extra["changes"] = json!([]);
        extra["doc_version"] = json!(session.generation);
        return Ok(extra);
    }
    spread_flow::stitch_new(&session.document, &mut doc);
    if placement_changed || !resizes.is_empty() {
        inherit_changes(&session.document, &mut doc, command);
    }
    doc.validate_refs().map_err(|_| CommandError::new("PAGE_OBJECT_REFERENCED"))?;
    let selection = session.selection.ids.iter().copied().filter(|id| doc.get_object(*id).is_some()).collect();
    super::document_change::commit(session, APPLY, doc, selection, extra)
}

fn added_slots(doc: &Document, targets: &[(usize, PageId)], sources: &[ObjectId]) -> Result<Vec<u32>, CommandError> {
    let spreads = crate::data_merge::layout::spread_ranges(doc);
    let mut reserved = HashMap::new();
    let mut row = 0;
    let mut slots = Vec::with_capacity(targets.len());
    let mut max_slots = HashMap::new();
    let mut rows = HashMap::new();
    for (row, spread) in spreads.iter().enumerate() {
        for page in &doc.pages[spread.clone()] {
            rows.insert(page.id, row);
        }
    }
    for item in &doc.master_pages.assignments {
        if let Some(&row) = rows.get(&item.page) {
            let slot = max_slots.entry((row, doc.master_pages.spread_id(item.master))).or_insert(0);
            *slot = (*slot).max(item.stack_slot);
        }
    }
    for (&(index, _), &source) in targets.iter().zip(sources) {
        while spreads.get(row).is_some_and(|range| index >= range.end) {
            row += 1;
        }
        let key = (row, doc.master_pages.spread_id(source));
        let slot = *reserved.entry(key).or_insert(
            max_slots.get(&key).map_or(Some(0), |value| value.checked_add(1)).ok_or_else(CommandError::out_of_range)?);
        slots.push(slot);
    }
    Ok(slots)
}

pub fn preview_spread_conform(doc: &Document, root: ObjectId, targets: &[(usize, PageId)]) -> Result<bool, CommandError> {
    conform::ConformPlan::new(doc, root, targets).map(|plan| plan.conflict)
}

pub fn application_sources(
    doc: &Document, root: ObjectId, targets: &[(usize, PageId)], source_page: Option<usize>,
) -> Result<Vec<ObjectId>, CommandError> {
    let pages = &doc.master_pages;
    if !pages.roots.contains(&root) || doc.get_object(root).is_none() {
        return Err(CommandError::not_found());
    }
    let Some(master) = pages.spread(root) else {
        return if source_page.is_none_or(|slot| slot == 0) {
            Ok(vec![root; targets.len()])
        } else {
            Err(CommandError::invalid_params())
        };
    };
    if master.pages.first() != Some(&root) && source_page.is_none() {
        return Ok(vec![root; targets.len()]);
    }
    let slot_count = master.pages.len();
    if slot_count == 0 || source_page.is_some_and(|slot| slot >= slot_count) {
        return Err(CommandError::invalid_params());
    }
    let spreads = crate::data_merge::layout::spread_ranges(doc);
    let right_cover = crate::data_merge::layout::has_right_cover(doc);
    let mut range_index = 0;
    let mut first_selected = None;
    let sources = targets.iter().map(|&(target, _)| {
        while spreads.get(range_index).is_some_and(|spread| target >= spread.end) {
            range_index += 1;
            first_selected = None;
        }
        let range = spreads.get(range_index).ok_or_else(CommandError::not_found)?;
        let index = if let Some(source) = source_page {
            let start = *first_selected.get_or_insert(target);
            source + target - start
        } else if let Some(fixed) = super::pages::spreads::fixed_spread(doc, range) {
            let slot = fixed.slots.iter().position(|slot| slot.page == Some(doc.pages[target].id))
                .unwrap_or(target - range.start);
            (master.spine + slot).saturating_sub(fixed.spine)
        } else if right_cover && range.start == 0 {
            master.spine
        } else if range.len() > 1 {
            let destination_spine = crate::data_merge::layout::spread_spine(doc, range);
            (master.spine + target - range.start).saturating_sub(destination_spine)
        } else {
            master.spine.saturating_sub(1)
        };
        let root = master.pages[index.min(slot_count - 1)];
        doc.get_object(root).map(|_| root).ok_or_else(CommandError::not_found)
    }).collect();
    sources
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
    doc: &mut Document, id: ObjectId, page_id: PageId, add: bool, replace: bool,
    keep_previous: bool, placement: MasterPlacement, stack_slot: u32,
) -> Result<(Option<ObjectId>, Option<ObjectId>), CommandError> {
    let mut insertion = doc.objects.len();
    let previous = if add { None } else { doc.master_pages.for_page(page_id).map(|assignment| assignment.master) };
    let old_slot = previous.and_then(|_| doc.master_pages.assignments.iter().position(|assignment| assignment.page == page_id));
    if add {
        let owned: HashSet<_> = doc.master_pages.on_page(page_id)
            .flat_map(|assignment| assignment.group.into_iter().chain(assignment.links.iter().map(|&(_, id)| id)))
            .collect();
        if let Some(last) = doc.objects.iter().rposition(|object| owned.contains(&object.id)) {
            insertion = last + 1;
        }
    } else if previous.is_some() {
        if !replace { return Err(CommandError::new("MASTER_ALREADY_APPLIED")); }
        let existing: Vec<_> = doc.master_pages.on_page(page_id).cloned().collect();
        insertion = retire_content(doc, existing.iter().map(|item| &item.inherited), keep_previous)?;
        doc.master_pages.assignments.retain(|assignment| assignment.page != page_id);
    }
    let before_apply = doc.objects.len();
    let group = apply_page_with_placement(doc, id, page_id, placement, stack_slot);
    if let Some(slot) = old_slot {
        doc.master_pages.assignments[slot..].rotate_right(1);
    }
    let added = doc.objects.len() - before_apply;
    doc.objects[insertion..].rotate_right(added);
    Ok((previous, group))
}

fn retire_content<'a>(
    doc: &mut Document, current: impl IntoIterator<Item = &'a InheritedMaster>, keep: bool,
) -> Result<usize, CommandError> {
    let owned: HashSet<_> = current.into_iter().flat_map(|item| item.links.iter().map(|&(_, instance)| instance)
        .chain(item.group)).collect();
    let at = doc.objects.iter().position(|object| owned.contains(&object.id)).unwrap_or(doc.objects.len());
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

pub(super) fn apply_to_page(doc: &mut Document, master: ObjectId, page: PageId) {
    apply_page_with_placement(doc, master, page, MasterPlacement::default(), 0);
}

fn apply_page_with_placement(
    doc: &mut Document, master: ObjectId, page: PageId, placement: MasterPlacement, stack_slot: u32,
) -> Option<ObjectId> {
    let inherited = apply_to_surface(doc, master, TargetSurface::Page(page), placement);
    let group = inherited.group;
    doc.master_pages.assignments.push(Assignment { page, stack_slot, inherited });
    group
}

fn apply_to_surface(doc: &mut Document, master: ObjectId, surface: TargetSurface, placement: MasterPlacement) -> InheritedMaster {
    let template = source_ids(doc, master);
    let mut inherited = InheritedMaster::new(master);
    inherited.placement = placement;
    create_assignment_group(doc, &mut inherited, surface);
    add_instances(doc, &mut inherited, surface, &template);
    inherited
}

pub(crate) fn materialize_groups(doc: &mut Document) {
    if doc.master_pages.inherited_content().all(|binding| binding.group.is_some() && binding.group_basis.is_some()) {
        return;
    }
    let mut assignments = std::mem::take(&mut doc.master_pages.assignments);
    for assignment in &mut assignments {
        materialize_group(doc, &mut assignment.inherited, TargetSurface::Page(assignment.page));
    }
    doc.master_pages.assignments = assignments;
    let mut nested = std::mem::take(&mut doc.master_pages.master_assignments);
    for binding in &mut nested {
        materialize_group(doc, &mut binding.inherited, TargetSurface::Master(binding.target));
    }
    doc.master_pages.master_assignments = nested;
}

fn materialize_group(doc: &mut Document, assignment: &mut InheritedMaster, surface: TargetSurface) {
    let Some(id) = assignment.group else {
        create_assignment_group(doc, assignment, surface);
        return;
    };
    if assignment.group_basis.is_none()
        && let Some(pos) = surface.origin(doc)
        && let Some(group) = doc.find_object_mut(id.0)
    {
        group.pos = pos;
        group.size = GROUP_BASIS;
        group.rotation_deg = 0.0;
        group.shear_deg = 0.0;
        assignment.group_basis = Some(GROUP_BASIS);
    }
}

fn create_assignment_group(doc: &mut Document, assignment: &mut InheritedMaster, surface: TargetSurface) {
    let Some(master) = doc.get_object(assignment.master) else { return };
    let name = master.name.clone();
    let Some(pos) = surface.origin(doc) else { return };
    let parent = surface.parent(doc);
    let linked: HashSet<_> = assignment.links.iter().map(|&(_, id)| id).collect();
    let roots: HashSet<_> = doc.objects.iter().filter(|object| {
        linked.contains(&object.id) && object.parent_id == parent
    }).map(|object| object.id).collect();
    let at = doc.objects.iter().position(|object| roots.contains(&object.id)).unwrap_or(doc.objects.len());
    let group = doc.create_object(Object {
        name,
        pos,
        size: GROUP_BASIS,
        kind: ObjectKind::Group,
        layer_folder: true,
        parent_id: parent,
        page_id: surface.page(),
        blend_mode: crate::blend::BlendMode::PassThrough,
        ..Object::default()
    });
    doc.objects[at..].rotate_right(1);
    for object in doc.objects.iter_mut().filter(|object| roots.contains(&object.id)) {
        object.parent_id = Some(group);
    }
    assignment.group = Some(group);
    assignment.group_basis = Some(GROUP_BASIS);
}

fn source_ids(doc: &Document, id: ObjectId) -> Vec<ObjectId> {
    let members = doc.subtree_ids(id);
    doc.objects.iter().filter(|object| object.id != id && members.contains(&object.id))
        .map(|object| object.id).collect()
}

fn add_instances(doc: &mut Document, assignment: &mut InheritedMaster, surface: TargetSurface, template: &[ObjectId]) {
    if template.is_empty() {
        return;
    }
    let Some(root) = doc.get_object(assignment.master) else { return };
    let Some([x, y]) = surface.origin(doc) else { return };
    let projection = surface.projection(doc, assignment);
    let offset = if projection.is_some() { [0.0; 2] } else { [x - root.pos[0], y - root.pos[1]] };
    let parent = assignment.group.or_else(|| surface.parent(doc));
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
            if let Some(projection) = projection && let Ok(projected) = projection.object(object) {
                *object = projected;
            }
            object.page_id = surface.page();
            object.parent_id = inherited_parent;
            if let (ObjectKind::PathText(data), Some(path)) = (&mut object.kind, path) {
                data.path = path;
            }
        }
        if projection.is_some() {
            project_copied_picture(doc, source, instance);
        }
    }
    connect_cloned_frames(doc, assignment, &links);
    assignment.links.extend(links);
}

fn connect_cloned_frames(doc: &mut Document, assignment: &InheritedMaster, links: &[(ObjectId, ObjectId)]) {
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
    let group = command.params.get("group_id").map(|_| read_id(&command.params, "group_id").map(ObjectId)).transpose()?;
    let matches = |assignment: &Assignment| assignment.page == page
        && group.is_none_or(|group| assignment.group == Some(group));
    let mut doc = session.document.clone();
    let removed: Vec<_> = doc.master_pages.assignments.iter().filter(|item| matches(item)).cloned().collect();
    if removed.is_empty() {
        return Err(CommandError::not_found());
    }
    if command.name == REMOVE {
        retire_content(&mut doc, removed.iter().map(|item| &item.inherited), false)?;
    }
    doc.master_pages.assignments.retain(|assignment| !matches(assignment));
    let group_ids: Vec<_> = removed.iter().filter_map(|item| item.group).collect();
    let selection = session.selection.ids.iter().copied().filter(|id| doc.get_object(*id).is_some()).collect();
    super::document_change::commit(session, &command.name, doc, selection, json!({"page_id": page, "group_ids": group_ids}))
}

fn set_base(session: &mut Session, command: &Command) -> CommandResult {
    let target = ObjectId(read_id(&command.params, "id")?);
    let parent = match command.params.get("master_id") {
        Some(Value::Null) => None,
        Some(_) => Some(ObjectId(read_id(&command.params, "master_id")?)),
        None => return Err(CommandError::invalid_params()),
    };
    let keep_previous = command.params.get("keep_previous").map(|value| {
        value.as_bool().ok_or_else(CommandError::invalid_params)
    }).transpose()?.unwrap_or(false);
    let before = &session.document;
    if !before.master_pages.roots.contains(&target)
        || parent.is_some_and(|id| !before.master_pages.roots.contains(&id))
    {
        return Err(CommandError::not_found());
    }
    if parent.is_some_and(|id| !before.master_pages.can_set_spread_base(target, id)) {
        return Err(CommandError::new("MASTER_BASE_CYCLE"));
    }
    let result = json!({"id": target, "master_id": parent});
    let pairs: Vec<_> = before.master_pages.base_pairs(target, parent).into_iter().filter(|&(target, source)| {
        before.master_pages.for_master(target).map(|assignment| assignment.inherited.master) != source
    }).collect();
    if pairs.is_empty() {
        let mut result = result;
        result["doc_version"] = json!(session.generation);
        result["changes"] = json!([]);
        return Ok(result);
    }
    let mut doc = before.clone();
    for (target, source) in pairs {
        doc.master_pages.master_assignments.retain(|assignment| assignment.target != target);
        if let Some(master) = source {
            let insertion = if let Some(old) = before.master_pages.for_master(target) {
                retire_content(&mut doc, std::iter::once(&old.inherited), keep_previous)?
            } else {
                let members = doc.subtree_ids(target);
                doc.objects.iter().position(|object| object.id != target && members.contains(&object.id))
                    .unwrap_or(doc.objects.len())
            };
            let previous_count = doc.objects.len();
            let inherited = apply_to_surface(&mut doc, master, TargetSurface::Master(target), MasterPlacement::default());
            let added = doc.objects.len() - previous_count;
            doc.objects[insertion..].rotate_right(added);
            doc.master_pages.master_assignments.push(MasterAssignment { target, inherited });
        }
    }
    spread_flow::stitch_new(before, &mut doc);
    inherit_changes(before, &mut doc, command);
    lock_positions(&mut doc);
    doc.validate_refs().map_err(CommandError::from)?;
    let selection = session.selection.ids.iter().copied().filter(|id| doc.get_object(*id).is_some()).collect();
    super::document_change::commit(session, SET_BASE, doc, selection, result)
}

fn reset(session: &mut Session, command: &Command) -> CommandResult {
    let id = ObjectId(read_id(&command.params, "id")?);
    let mut doc = session.document.clone();
    let assignment = doc.master_pages.inherited_content().find(|item| item.links.iter().any(|link| link.1 == id))
        .ok_or_else(CommandError::not_found)?;
    let source_id = doc.master_pages.source_of(id).ok_or_else(CommandError::not_found)?;
    let mut source = doc.get_object(source_id).cloned().ok_or_else(CommandError::not_found)?;
    let current = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let surface = current.page_id.map(TargetSurface::Page).or_else(||
        crate::artboard::owner(&doc, id).map(|root| TargetSurface::Master(root.id)));
    let projection = surface.and_then(|surface| surface.projection(&doc, assignment));
    if let Some(projection) = projection {
        source = projection.object(&source)?;
    }
    let old_story = current.story_id();
    let spread_links = spread_flow::links_for(&doc, id);
    let links = spread_links.as_deref().unwrap_or(&assignment.links);
    let preserve_flow = if let (
        ObjectKind::FrameText { story: original },
        ObjectKind::FrameText { story: local },
    ) = (&mut source.kind, &current.kind) {
        let original_flow = crate::frame::flow_frames(&doc, *original);
        let local_flow = crate::frame::flow_frames(&doc, *local);
        let mapped = original_flow.iter().map(|frame| {
            links.iter().find_map(|&(source, instance)| (source == *frame).then_some(instance))
        });
        let differs = if spread_links.is_some() {
            !mapped.flatten().eq(local_flow.iter().copied())
        } else {
            !mapped.eq(local_flow.iter().copied().map(Some))
        };
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
    let story_override_instances: Vec<_> = if let Some(story) = old_story && source_story.is_some() {
        links.iter().filter_map(|&(_, instance)| {
            (doc.get_object(instance).and_then(Object::story_id) == Some(story)).then_some(instance)
        }).collect()
    } else {
        Vec::new()
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
    if projection.is_some() {
        project_copied_picture(&mut doc, source_id, id);
    }
    if let Some(story) = old_story {
        crate::frame::drop_story_if_unowned(&mut doc, story);
    }
    if let Some(resource) = resource {
        doc.place_image(id, resource)?;
    } else {
        doc.unplace_image(id);
    }
    let overrides = &mut doc.master_pages.inherited_content_mut()
        .find(|item| item.links.iter().any(|link| link.1 == id)).ok_or_else(CommandError::not_found)?
        .matching_overrides;
    if let Some(mut properties) = overrides.remove(&source_id.0)
        && preserve_flow && properties.remove(STORY_TEXT_OVERRIDE)
    {
        overrides.entry(source_id.0).or_default().insert(STORY_TEXT_OVERRIDE.into());
    }
    for binding in doc.master_pages.inherited_content_mut() {
        for &(source, instance) in &binding.links {
            if story_override_instances.contains(&instance)
                && let Some(properties) = binding.matching_overrides.get_mut(&source.0)
            {
                properties.remove(STORY_TEXT_OVERRIDE);
                if properties.is_empty() {
                    binding.matching_overrides.remove(&source.0);
                }
            }
        }
    }
    lock_positions(&mut doc);
    if !doc.master_pages.master_assignments.is_empty() {
        inherit_changes(&session.document, &mut doc, command);
        lock_positions(&mut doc);
    }
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

fn project_copied_picture(doc: &mut Document, source: ObjectId, instance: ObjectId) {
    if let (Some(original), Some(local), Some(mut frame)) = (
        doc.get_object(source), doc.get_object(instance), super::picture_frame::get_frame(doc, instance),
    ) {
        placement::project_picture_frame(doc, original, local.size, &mut frame);
        super::picture_frame::put_frame(doc, frame);
    }
}

fn ordered_master_assignments(assignments: &[MasterAssignment]) -> Vec<usize> {
    let mut pending: HashSet<_> = assignments.iter().map(|item| item.target).collect();
    let mut order = Vec::with_capacity(assignments.len());
    while !pending.is_empty() {
        let before = order.len();
        for (index, item) in assignments.iter().enumerate() {
            if !pending.contains(&item.inherited.master) && pending.remove(&item.target) {
                order.push(index);
            }
        }
        if order.len() == before {
            break;
        }
    }
    order
}

pub(crate) fn inherit_changes(before: &Document, doc: &mut Document, command: &Command) -> bool {
    let roots: HashSet<_> = doc.master_pages.roots.iter().copied()
        .filter(|id| doc.get_object(*id).is_some()).collect();
    let mut changed = doc.master_pages.roots.len() != roots.len();
    doc.master_pages.roots.retain(|id| roots.contains(id));
    let old_tag_count = doc.master_pages.tag_colours.len();
    doc.master_pages.tag_colours.retain(|id, _| roots.contains(&ObjectId(*id)));
    changed |= doc.master_pages.tag_colours.len() != old_tag_count;
    let old_setup_count = doc.master_pages.page_setup.len();
    doc.master_pages.page_setup.retain(|id, _| roots.contains(&ObjectId(*id)));
    changed |= doc.master_pages.page_setup.len() != old_setup_count;
    let old_spread_count = doc.master_pages.spreads.len();
    doc.master_pages.spreads.retain(|spread| spread.pages.iter().all(|id| roots.contains(id)));
    changed |= old_spread_count != doc.master_pages.spreads.len();

    let valid_masters: Vec<_> = doc.master_pages.master_assignments.iter().map(|item| {
        roots.contains(&item.target) && roots.contains(&item.inherited.master)
            && item.inherited.group.is_none_or(|id| doc.get_object(id).is_some())
    }).collect();
    let mut valid = valid_masters.into_iter();
    let old_count = doc.master_pages.master_assignments.len();
    doc.master_pages.master_assignments.retain(|_| valid.next().unwrap_or(false));
    changed |= old_count != doc.master_pages.master_assignments.len();

    let (flow_changed, suppressed_text) = spread_flow::propagate_topology(before, doc, command);
    changed |= flow_changed;
    let mut propagated = HashMap::new();
    for index in ordered_master_assignments(&doc.master_pages.master_assignments) {
        let mut binding = doc.master_pages.master_assignments[index].clone();
        let surface = TargetSurface::Master(binding.target);
        let source = binding.inherited.master;
        let old = source_ids(before, source);
        let current = source_ids(doc, source);
        let source_command = propagated.get(&source).unwrap_or(command);
        let (updated, flow_changed) = inherit_target(
            before, doc, &mut binding.inherited, surface, &old, &current, source_command, &suppressed_text);
        changed |= updated | lock_inherited(doc, &binding.inherited, surface);
        if flow_changed && let Some(translated) = translate_flow(source_command, &binding.inherited.links) {
            propagated.insert(binding.target, translated);
        }
        doc.master_pages.master_assignments[index] = binding;
    }

    let mut templates = HashMap::new();
    let page_assignments = doc.master_pages.assignments.clone();
    let mut invalid = Vec::new();
    for (index, mut assignment) in page_assignments.into_iter().enumerate() {
        let surface = TargetSurface::Page(assignment.page);
        if surface.origin(doc).is_none() {
            continue;
        }
        if !roots.contains(&assignment.master) || assignment.group.is_some_and(|id| doc.get_object(id).is_none()) {
            // Removing a template leaves the current page content editable.
            invalid.push(index);
            changed = true;
            continue;
        }
        let source = assignment.master;
        let (old, current) = templates.entry(source)
            .or_insert_with(|| (source_ids(before, source), source_ids(doc, source)));
        let source_command = propagated.get(&source).unwrap_or(command);
        let (updated, _) = inherit_target(
            before, doc, &mut assignment.inherited, surface, old, current, source_command, &suppressed_text);
        if updated {
            doc.master_pages.assignments[index] = assignment;
        }
        changed |= updated;
    }
    for index in invalid.into_iter().rev() {
        doc.master_pages.assignments.remove(index);
    }
    if !matches!(command.name.as_str(), "edit.undo" | "edit.redo") {
        changed |= spread_flow::stitch_new(before, doc);
    }
    if changed {
        crate::longdoc::xref::refresh_xrefs(doc);
    }
    changed
}

fn inherit_target(
    before: &Document, doc: &mut Document, inherited: &mut InheritedMaster, surface: TargetSurface,
    old: &[ObjectId], current: &[ObjectId], command: &Command, suppressed_text: &HashSet<ObjectId>,
) -> (bool, bool) {
    let mut changed = false;
    if let Some(group) = inherited.group
        && let Some(master) = doc.get_object(inherited.master)
        && before.get_object(inherited.master).is_some_and(|old| old.name != master.name)
    {
        let name = master.name.clone();
        if let Some(folder) = doc.find_object_mut(group.0) {
            changed |= folder.name != name;
            folder.name = name;
        }
    }
    let old_links = inherited.links.clone();
    let old_overrides = inherited.matching_overrides.clone();
    let members: HashSet<_> = current.iter().copied().collect();
    // Explicitly removed source objects leave their current copies editable.
    inherited.links.retain(|(source, instance)| members.contains(source) && doc.get_object(*instance).is_some());
    let (flow_changed, flow_sources) = inherit_flow(before, doc, &inherited.links, old, command);
    changed |= flow_changed;
    inherited.matching_overrides.retain(|source, _| inherited.links.iter().any(|link| link.0.0 == *source));
    let restoring = matches!(command.name.as_str(), "edit.undo" | "edit.redo");
    let previous = surface.binding(before, inherited).filter(|old| old.master == inherited.master).unwrap_or(inherited);
    // The native transform has already moved the whole group and its local children.
    // Compare the old source in the current layer pose to keep those moves inherited.
    let layer = surface.layer_matrix(doc, inherited);
    let layer_moved = matches!(command.name.as_str(), "transform.selection_box" | "transform.set")
        && layer != surface.layer_matrix(before, previous);
    let old_projection = surface.projection_with_layer(before, previous, layer);
    let projection = surface.projection_with_layer(doc, inherited, layer);
    for &(source, instance) in &inherited.links {
        let history_flow_change = restoring && frame_flow(before, source) != frame_flow(doc, source);
        let inherit_text = !history_flow_change && !flow_sources.contains(&source) && !suppressed_text.contains(&instance);
        let mut overrides = inherited.matching_overrides.remove(&source.0).unwrap_or_default();
        changed |= inherit_object(before, doc, source, instance, inherit_text, &mut overrides,
            old_projection, projection, layer.is_some(), layer_moved);
        if !overrides.is_empty() {
            inherited.matching_overrides.insert(source.0, overrides);
        }
    }
    if old != current {
        let previous: HashSet<_> = old.iter().copied().collect();
        let assigned: HashSet<_> = inherited.links.iter().map(|(source, _)| *source).collect();
        let added: Vec<_> = current.iter().copied()
            .filter(|id| !previous.contains(id) && !assigned.contains(id)).collect();
        add_instances(doc, inherited, surface, &added);
    }
    changed |= inherit_structure(before, doc, inherited, surface, &old_links, old, current);
    (changed || old_links != inherited.links || old_overrides != inherited.matching_overrides, flow_changed)
}

fn translate_flow(command: &Command, links: &[(ObjectId, ObjectId)]) -> Option<Command> {
    let map = |key| {
        let original = command.params.get(key)?.as_u64()?;
        links.iter().find_map(|(source, local)| (source.0 == original).then_some(*local))
    };
    match command.name.as_str() {
        "text.link" => Some(Command::with_params("text.link", json!({"from": map("from")?, "to": map("to")?}))),
        "text.unlink" => Some(Command::with_params("text.unlink", json!({"id": map("id")?}))),
        _ => None,
    }
}

fn frame_flow(doc: &Document, frame: ObjectId) -> Vec<ObjectId> {
    doc.get_object(frame)
        .and_then(Object::story_id)
        .map_or_else(Vec::new, |story| crate::frame::flow_frames(doc, story))
}

fn inherit_flow(
    before: &Document, doc: &mut Document, links: &[(ObjectId, ObjectId)], template: &[ObjectId], command: &Command,
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
    let mapped = |source| links.iter().find_map(|&(id, instance)| (source == id).then_some(instance));
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
    before: &Document, doc: &mut Document, assignment: &InheritedMaster, surface: TargetSurface,
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
    let old_board = assignment.group.or_else(|| surface.parent(before));
    let board = assignment.group.or_else(|| surface.parent(doc));
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

fn inherit_object(
    before: &Document, doc: &mut Document, source: ObjectId, instance: ObjectId,
    inherit_text: bool, overrides: &mut BTreeSet<String>, old_projection: Option<Projection>, projection: Option<Projection>,
    transformed_layer: bool, layer_moved: bool,
) -> bool {
    let (Some(old), Some(new), Some(local)) = (before.get_object(source), doc.get_object(source), doc.get_object(instance)) else {
        return false;
    };
    let story_ids = (old.story_id(), new.story_id(), local.story_id());
    let projected_old = old_projection.and_then(|projection| projection.object(old).ok());
    let projected_new = projection.and_then(|projection| projection.object(new).ok());
    let updated = inherited_properties(projected_old.as_ref().unwrap_or(old),
        projected_new.as_ref().unwrap_or(new), local, overrides, transformed_layer);
    let mut old_frame = super::picture_frame::get_frame(before, source);
    let mut new_frame = super::picture_frame::get_frame(doc, source);
    if let (Some(frame), Some(projected)) = (&mut old_frame, &projected_old) {
        placement::project_picture_frame(before, old, projected.size, frame);
    }
    if let (Some(frame), Some(projected)) = (&mut new_frame, &projected_new) {
        placement::project_picture_frame(doc, new, projected.size, frame);
    }
    let mut inherited_frame = super::picture_frame::get_frame(doc, instance);
    let converted = old.kind != ObjectKind::PictureFrame && new.kind == ObjectKind::PictureFrame
        && local.kind == old.kind;
    let mut frame_changed = false;
    if layer_moved
        && let Some(frame) = &mut inherited_frame
        && let Some(previous) = before.get_object(instance).filter(|previous| previous.size != local.size)
    {
        let original = frame.clone();
        // Resizing a single picture frame keeps the crop composition fixed.
        // Scaling its entire applied layer also scales the picture inside the frame.
        placement::project_picture_frame(before, previous, local.size, frame);
        frame_changed = *frame != original;
    }
    if converted && inherited_frame.is_none() && let Some(mut frame) = new_frame.clone() {
        frame.object = instance;
        frame.image = doc.placed_resource(instance);
        inherited_frame = Some(frame);
        frame_changed = true;
    }
    if let (Some(old), Some(new), Some(local)) = (&old_frame, &new_frame, &mut inherited_frame) {
        macro_rules! follow {
            ($($prop:ident).+) => {
                let old_value = &old.$($prop).+;
                let local_value = &local.$($prop).+;
                let follows = transformed_layer && projected_values_match(old_value, local_value);
                if follows_master(overrides, concat!("picture_frame.", stringify!($($prop).+)),
                    old_value, &new.$($prop).+, if follows { old_value } else { local_value })
                {
                    local.$($prop).+ = new.$($prop).+;
                    frame_changed = true;
                }
            };
        }
        follow!(shape);
        follow!(fit);
        follow!(anchor);
        follow!(manual_layout);
        follow!(content.offset);
        follow!(content.scale);
        follow!(content.rotation);
    }
    let old_resource = before.placed_resource(source);
    let new_resource = doc.placed_resource(source);
    let image_changed = follows_master(
        overrides, "image_resource", &old_resource, &new_resource, &doc.placed_resource(instance),
    );
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
    let text_changed = inherit_text && inherit_story(before, doc, story_ids, overrides);
    if text_changed && let Some(story) = story_ids.2 {
        let _ = crate::text_cmd::refresh_art_bounds(doc, story);
    }
    changed || text_changed
}

fn follows_master<T: PartialEq>(overrides: &mut BTreeSet<String>, field: &str, old: &T, new: &T, local: &T) -> bool {
    if new == old || overrides.contains(field) {
        return false;
    }
    if local == old {
        return true;
    }
    if local == new {
        overrides.insert(field.to_owned());
    }
    false
}

fn projected_values_match<T: serde::Serialize>(old: &T, local: &T) -> bool {
    serde_json::to_value(old).ok().zip(serde_json::to_value(local).ok())
        .is_some_and(|(old, local)| same_projected_value(&old, &local))
}

// A layer matrix reconstructed from its rotated reference box can round a few
// floating-point units differently from the original canvas transform.
fn same_projected_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) if a.is_f64() || b.is_f64() => {
            a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)|
                (a - b).abs() <= 0.0002 + a.abs().max(b.abs()) * (4.0 * f64::from(f32::EPSILON)))
        }
        (Value::Array(a), Value::Array(b)) => a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| same_projected_value(a, b)),
        (Value::Object(a), Value::Object(b)) => a.len() == b.len()
            && a.iter().all(|(key, val)| b.get(key).is_some_and(|other| same_projected_value(val, other))),
        _ => left == right,
    }
}

fn inherited_properties(
    old: &Object, new: &Object, local: &Object, overrides: &mut BTreeSet<String>, transformed_layer: bool,
) -> Option<Object> {
    let mut updated = None;
    macro_rules! follow {
        ($($field:ident),+ $(,)?) => {
            $(
                if let (Ok(old_value), Ok(new_value)) = (serde_json::to_value(&old.$field), serde_json::to_value(&new.$field))
                    && old_value != new_value
                    && let Ok(local_value) = serde_json::to_value(&local.$field)
                    && follows_master(overrides, stringify!($field), &old_value, &new_value,
                        if transformed_layer && same_projected_value(&old_value, &local_value) {
                            &old_value
                        } else {
                            &local_value
                        })
                {
                    updated.get_or_insert_with(|| local.clone()).$field = new.$field.clone();
                }
            )+
        };
    }
    if let (Some(old_curve), Some(new_curve), Some(local_curve)) = (old.curve(), new.curve(), local.curve())
        && (old_curve.contour != new_curve.contour || old_curve.holes != new_curve.holes)
    {
        // A node edit changes the local contours and their bounds together.
        let old_shape = (&old_curve.contour, &old_curve.holes, old.size);
        let new_shape = (&new_curve.contour, &new_curve.holes, new.size);
        let local_shape = (&local_curve.contour, &local_curve.holes, local.size);
        let follows_shape = transformed_layer
            && projected_values_match(&old_shape, &local_shape);
        if follows_master(overrides, "curve_geometry", &old_shape, &new_shape,
            if follows_shape { &old_shape } else { &local_shape }) && !overrides.contains("size")
        {
            let mut curve = local_curve.clone();
            curve.contour = new_curve.contour.clone();
            curve.holes = new_curve.holes.clone();
            let object = updated.get_or_insert_with(|| local.clone());
            object.kind = ObjectKind::Curve(curve);
            object.size = new.size;
        }
    } else if !overrides.contains("curve_geometry") {
        follow!(size);
    }
    follow!(
        name, fill, stroke, additional_fills, additional_strokes, hidden_appearance,
        appearance_stack, appearance_blends, path_brush, paint_order, opacity, rotation_deg,
        shear_deg, text_scale, path_text_linear, shape, frame, vector_crop, transparency, blend_mode, blend_ranges, effects,
    );
    updated
}
fn inherit_story(
    before: &Document,
    doc: &mut Document,
    (old_id, new_id, local_id): (Option<crate::document::StoryId>, Option<crate::document::StoryId>, Option<crate::document::StoryId>),
    overrides: &mut BTreeSet<String>,
) -> bool {
    let (Some(old_id), Some(new_id), Some(local_id)) = (old_id, new_id, local_id) else { return false };
    let Some(old) = before.stories.iter().find(|story| story.id == old_id) else { return false };
    let Some(new) = doc.stories.iter().find(|story| story.id == new_id) else { return false };
    if old == new || new_id == local_id {
        return false;
    }
    let new = new.clone();
    let replaces_text = new.text != old.text;
    if replaces_text {
        // A preceding frame may already have updated this page's shared story.
        if before.stories.iter().find(|story| story.id == local_id).is_some_and(|local| local.text == new.text) {
            overrides.insert(STORY_TEXT_OVERRIDE.into());
        }
        if overrides.contains(STORY_TEXT_OVERRIDE) {
            return false;
        }
    }
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

// The document setup rescales both the master and its copies. Rebase the
// reference box so a resize does not apply the same scale to the copies twice.
pub(crate) fn resize_layer_references(before: &Document, doc: &mut Document, scale: [f32; 2]) {
    let surfaces = doc.master_pages.assignments.iter().map(|binding| (TargetSurface::Page(binding.page), &binding.inherited))
        .chain(doc.master_pages.master_assignments.iter().map(|binding| (TargetSurface::Master(binding.target), &binding.inherited)));
    let positions: Vec<_> = surfaces.filter_map(|(surface, binding)| {
        binding.group_basis?;
        let group = before.get_object(binding.group?)?;
        let old_origin = surface.origin(before)?;
        let new_origin = surface.origin(doc)?;
        let pos = std::array::from_fn(|axis| new_origin[axis] + (group.pos[axis] - old_origin[axis]) * scale[axis]);
        Some((group.id, pos))
    }).collect();
    for (id, pos) in positions {
        if let Some(group) = doc.find_object_mut(id.0) {
            group.pos = pos;
        }
    }
    for binding in doc.master_pages.inherited_content_mut() {
        binding.group_basis = binding.group_basis.map(|basis| std::array::from_fn(|axis| basis[axis] * scale[axis]));
    }
}

pub fn lock_positions(doc: &mut Document) {
    for index in ordered_master_assignments(&doc.master_pages.master_assignments) {
        let binding = doc.master_pages.master_assignments[index].clone();
        lock_inherited(doc, &binding.inherited, TargetSurface::Master(binding.target));
    }
    let mut positions = Vec::new();
    for assignment in &doc.master_pages.assignments {
        collect_positions(doc, &assignment.inherited, TargetSurface::Page(assignment.page), &mut positions);
    }
    for (instance, position) in positions {
        if let Some(object) = doc.find_object_mut(instance.0) {
            object.pos = position;
        }
    }
}

fn lock_inherited(doc: &mut Document, inherited: &InheritedMaster, surface: TargetSurface) -> bool {
    let mut positions = Vec::new();
    collect_positions(doc, inherited, surface, &mut positions);
    let mut changed = false;
    for (instance, position) in positions {
        if let Some(object) = doc.find_object_mut(instance.0) && object.pos != position {
            object.pos = position;
            changed = true;
        }
    }
    changed
}

fn collect_positions(doc: &Document, inherited: &InheritedMaster, surface: TargetSurface, out: &mut Vec<(ObjectId, [f32; 2])>) {
    let Some(root) = doc.get_object(inherited.master) else { return };
    let Some([x, y]) = surface.origin(doc) else { return };
    let projection = surface.projection(doc, inherited);
    for &(source, instance) in &inherited.links {
        if inherited.matching_overrides.get(&source.0).is_some_and(|fields| fields.contains(LOCAL_POSITION_OVERRIDE)) {
            continue;
        }
        if let Some(source) = doc.get_object(source) {
            let position = projection.map_or_else(
                || [source.pos[0] - root.pos[0] + x, source.pos[1] - root.pos[1] + y],
                |projection| projection.position(source));
            out.push((instance, position));
        }
    }
}
