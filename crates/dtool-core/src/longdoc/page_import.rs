use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::clipboard::{collect_bundle_members, paste_bundle_mapped};
use crate::commands::{document_change, masters, page_artboards, pages};
use crate::document::{Page, PageId};
use crate::guides::Guides;
use crate::longdoc::helpers::{parse_page_ranges, unique_name};
use crate::workspace::{ContextId, Scope, Workspace};
use crate::{Command, CommandError, CommandResult, Document, ObjectId, Session};
use serde_json::{Value, json};

pub(crate) mod navigation;

pub const IMPORT: &str = "page.import";
pub const COPY_TO_DOCUMENT: &str = "page.copy_to_document";
pub const MOVE_TO_DOCUMENT: &str = "page.move_to_document";

fn section_index(value: &Value) -> Result<usize, CommandError> {
    value.as_u64().and_then(|n| usize::try_from(n).ok())
        .and_then(|n| n.checked_sub(1)).ok_or_else(CommandError::invalid_params)
}

fn selected_indices(source: &Document, params: &Value) -> Result<Vec<usize>, CommandError> {
    let count = source.pages.len();
    let pages = match (params.get("pages"), params.get("section")) {
        (None, None) => (1..=count).collect(),
        (Some(range), None) => parse_page_ranges(range.as_str().ok_or_else(CommandError::invalid_params)?, count)?,
        (None, Some(section)) => {
            let range = source.section_page_range(section_index(section)?).ok_or_else(CommandError::out_of_range)?;
            range.map(|index| index + 1).collect()
        }
        _ => return Err(CommandError::invalid_params()),
    };
    if pages.is_empty() || pages.iter().any(|page| !(1..=count).contains(page)) {
        return Err(CommandError::out_of_range());
    }
    Ok(pages.into_iter().map(|page| page - 1).collect())
}

struct Placement {
    index: usize,
    replacing: bool,
    retained_section_start: Option<usize>,
}

fn placement(destination: &Document, params: &Value) -> Result<Placement, CommandError> {
    let index_of = |value: &Value| {
        let id = PageId(value.as_u64().ok_or_else(CommandError::invalid_params)?);
        destination.pages.iter().position(|page| page.id == id).ok_or_else(CommandError::not_found)
    };
    let section_edge = params.get("section_edge");
    let target = params.get("into_section");
    if target.is_none() && section_edge.is_some() { return Err(CommandError::invalid_params()); }
    let (index, replacing, retained_section_start) = match (params.get("before_id"), params.get("after_id"), params.get("replace_from_id"), target) {
        (Some(before), None, None, None) => (index_of(before)?, false, None),
        (None, Some(after), None, None) => (index_of(after)? + 1, false, None),
        (None, None, Some(first), None) => (index_of(first)?, true, None),
        (None, None, None, Some(section)) => {
            let index = section_index(section)?;
            let range = destination.section_page_range(index).ok_or_else(CommandError::out_of_range)?;
            match section_edge.map(Value::as_str) {
                Some(Some("start")) => (range.start, false, Some(index)),
                None | Some(Some("end")) => (range.end, false, None),
                _ => return Err(CommandError::invalid_params()),
            }
        }
        (None, None, None, None) => (destination.pages.len(), false, None),
        _ => return Err(CommandError::invalid_params()),
    };
    Ok(Placement { index, replacing, retained_section_start })
}

fn source_in_destination_units(mut source: Document, destination: &Document) -> Result<(Document, f32), CommandError> {
    if let Some((layout, _)) = page_artboards::enter_layout(&source, true) {
        source = layout;
    }
    let factor = crate::doc_setup::unit_factor(source.units, source.dpi, destination.units, destination.dpi);
    if factor != 1.0 || source.units != destination.units || source.dpi != destination.dpi {
        let mut setup = crate::doc_setup::DocSetup::from_document(&source);
        setup.units = destination.units;
        setup.dpi = destination.dpi;
        setup.page_size = setup.page_size.map(|value| value * factor);
        setup.page_origin = setup.page_origin.map(|value| value * factor);
        for side in [&mut setup.margins, &mut setup.bleed] {
            side.left *= factor;
            side.right *= factor;
            side.top *= factor;
            side.bottom *= factor;
        }
        setup.resize = crate::doc_setup::ResizeMode::AnchorToPage;
        setup.try_apply(&mut source)?;
    }
    Ok((source, factor))
}

pub(crate) fn import_swatches(doc: &mut Document, bundle: &crate::clipboard::ClipboardBundle) {
    for swatch in &bundle.swatches {
        if doc.swatches.iter().any(|entry| entry.name == swatch.name && entry.colour == swatch.colour) {
            continue;
        }
        let mut copy = swatch.clone();
        copy.id = crate::colour::swatch::next_id(doc);
        doc.next_swatch_id = copy.id + 1;
        doc.swatches.push(copy);
        if !swatch.palette.is_empty() && !doc.palettes.contains(&swatch.palette) {
            doc.palettes.push(swatch.palette.clone());
        }
    }
}

pub(crate) fn resolve_bundle_links(bundle: &mut crate::clipboard::ClipboardBundle, directory: &Path) {
    for resource in bundle.resources.iter_mut().chain(bundle.brush_sources.values_mut()) {
        if resource.is_linked() && let Some(path) = resource.resolved_path(Some(directory)) {
            resource.path = Some(path);
        }
    }
    for link in &mut bundle.text_links {
        if link.path.is_relative() {
            link.path = directory.join(&link.path);
        }
    }
}

fn source_directory(path: &Path) -> Result<std::path::PathBuf, CommandError> {
    let full = std::path::absolute(path).map_err(|err| crate::import_hook::read_error(&err))?;
    full.parent().map(Path::to_path_buf).ok_or_else(CommandError::invalid_params)
}

pub(crate) fn run(session: &mut Session, command: &Command) -> CommandResult {
    if session.document.layout_artboard_origins.is_none() {
        return Err(CommandError::invalid_params());
    }
    let path = command.path_param().filter(|path| !path.as_os_str().is_empty()).ok_or_else(CommandError::invalid_params)?;
    let directory = source_directory(&path)?;
    let placement = placement(&session.document, &command.params)?;
    let (source, extras) = crate::native_io::read_file(&path).map_err(|err| err.read_error())?;
    let incoming = extras.get(crate::workspace::NATIVE_GUIDES_KEY)
        .and_then(|value| serde_json::from_value(value.clone()).ok());
    let source = source_in_destination_units(source, &session.document)?;
    let indices = selected_indices(&source.0, &command.params)?;
    prepare_import(session, source, indices, placement, &directory, incoming)?.commit(session, IMPORT)
}

pub fn insert_imported_pages(session: &mut Session, source: Document, path: &Path) -> CommandResult {
    if session.document.layout_artboard_origins.is_none() {
        return Err(CommandError::invalid_params());
    }
    let directory = source_directory(path)?;
    let params = pages::viewed_page(session).map_or_else(|| json!({}), |page| json!({"after_id": page}));
    let placement = placement(&session.document, &params)?;
    let source = source_in_destination_units(source, &session.document)?;
    let indices = (0..source.0.pages.len()).collect();
    prepare_import(session, source, indices, placement, &directory, None)?.commit(session, IMPORT)
}

pub(crate) fn run_workspace(workspace: &mut Workspace, scope: Scope, command: &Command) -> Option<(Option<ContextId>, CommandResult)> {
    if !matches!(command.name.as_str(), COPY_TO_DOCUMENT | MOVE_TO_DOCUMENT) {
        return None;
    }
    let moving = command.name == MOVE_TO_DOCUMENT;
    let source = scope.context.or_else(|| workspace.active());
    let destination = command.params["destination_context"].as_u64().map(ContextId);
    let result = (|| {
        let source = source.ok_or_else(CommandError::no_document)?;
        let destination = destination.ok_or_else(CommandError::invalid_params)?;
        if source == destination {
            return Err(CommandError::invalid_params());
        }
        let target = workspace.get(destination).ok_or_else(CommandError::context_not_found)?;
        let origin = workspace.get(source).ok_or_else(CommandError::context_not_found)?;
        if origin.session.document.layout_artboard_origins.is_none() || target.session.document.layout_artboard_origins.is_none() {
            return Err(CommandError::invalid_params());
        }
        let ids: Vec<PageId> = serde_json::from_value(command.params["ids"].clone())
            .map_err(|_| CommandError::invalid_params())?;
        if command.params.get("replace_from_id").is_some() || command.params.get("into_section").is_some() {
            return Err(CommandError::invalid_params());
        }
        let directory = match &origin.path {
            Some(path) => source_directory(path)?,
            None => std::env::current_dir().map_err(|err| crate::import_hook::read_error(&err))?,
        };
        workspace.finish_pending_edit(source, true)?;
        workspace.finish_pending_edit(destination, true)?;
        let origin = workspace.get(source).ok_or_else(CommandError::context_not_found)?;
        let target = workspace.get(destination).ok_or_else(CommandError::context_not_found)?;
        let indices = pages::selected_page_indices(&origin.session.document, &ids)?;
        let placement = placement(&target.session.document, &command.params)?;
        let removal = moving.then(|| pages::prepare_page_deletion(&origin.session, ids, true)).transpose()?;
        let incoming = source_in_destination_units(origin.session.document.clone(), &target.session.document)?;
        let prepared = prepare_import(&target.session, incoming, indices, placement,
            &directory, Some(origin.session.guides.clone()))?;
        let mut result = if let Some(removal) = removal {
            let value = workspace.apply_linked_page_change(source, destination, |source, destination| {
                let mut incoming = destination.run_with(command, |session, command| prepared.commit(session, &command.name))?;
                let removed = source.run_with(command, |session, command| removal.commit(session, &command.name))?;
                incoming["source_doc_version"] = removed["doc_version"].clone();
                incoming["source_ids"] = removed["ids"].clone();
                Ok(incoming)
            })?;
            workspace.record_paired_page_move(source, destination);
            value
        } else {
            let target = workspace.get_mut(destination).ok_or_else(CommandError::context_not_found)?;
            let result = target.session.run_with(command, |session, command| prepared.commit(session, &command.name))?;
            crate::longdoc::fields::refresh_if_stale(&mut target.session);
            result
        };
        result["context_id"] = json!(destination.0);
        result["source_context"] = json!(source.0);
        Ok(result)
    })();
    Some((if result.is_ok() { destination } else { source }, result))
}

struct PageImport {
    document: Document,
    selection: Vec<ObjectId>,
    guides: Guides,
    result: Value,
}

impl PageImport {
    fn commit(self, session: &mut Session, command: &str) -> CommandResult {
        document_change::commit_guides(session, command, self.document, self.selection, self.guides, self.result)
    }
}

fn prepare_import(
    session: &Session, (source, factor): (Document, f32), indices: Vec<usize>, placement: Placement,
    directory: &Path, incoming: Option<Guides>,
) -> Result<PageImport, CommandError> {
    let target = &session.document;
    let Placement { index: at, replacing, retained_section_start } = placement;
    let end = at + indices.len();
    if replacing && end > target.pages.len() {
        return Err(CommandError::out_of_range());
    }
    let included: HashSet<_> = indices.iter().copied().collect();
    let mut chosen_masters: HashSet<_> = indices.iter().flat_map(|&index| {
        source.master_pages.on_page(source.pages[index].id).map(|item| item.master)
    }).collect();
    let mut pending: Vec<_> = chosen_masters.iter().copied().collect();
    while let Some(id) = pending.pop() {
        for &page in source.master_pages.spread_pages(&id) {
            if chosen_masters.insert(page) {
                pending.push(page);
            }
        }
        if let Some(binding) = source.master_pages.for_master(id)
            && chosen_masters.insert(binding.inherited.master)
        {
            pending.push(binding.inherited.master);
        }
    }
    let mut members = HashSet::new();
    let mut page_of = HashMap::new();
    for object in &source.objects {
        if masters::owns_source(&source, object.id) {
            continue;
        }
        let index = crate::data_merge::layout::object_page_index(&source, object.id);
        if included.contains(&index) {
            members.insert(object.id);
            page_of.insert(object.id, index);
        }
    }
    for &master in &chosen_masters {
        members.extend(source.subtree_ids(master));
    }
    if source.objects.iter().any(|object| members.contains(&object.id)
        && object.path_text_data().is_some_and(|data| !members.contains(&data.path)))
    {
        return Err(CommandError::new("PAGE_OBJECT_REFERENCED"));
    }

    let mut bundle = collect_bundle_members(&source, &members);
    let excerpts = super::pages::restrict_page_copy(&source, &mut bundle);
    resolve_bundle_links(&mut bundle, directory);
    for object in &mut bundle.objects {
        object.page_id = None;
    }

    let mut document = target.clone();
    let mut guides = session.guides.clone();
    let mut selection = session.selection.ids.clone();
    if replacing {
        let replaced: Vec<_> = (at..end).collect();
        let doomed = pages::remove_page_content(target, &mut document, &replaced, true)?;
        selection.retain(|id| !doomed.contains(id));
        let ids: HashSet<_> = target.pages[at..end].iter().map(|page| page.id).collect();
        guides.items.retain(|guide| !guide.page_id.is_some_and(|id| ids.contains(&id)));
    }
    let rects = crate::scene::page_rects(&source);
    let destination_group = if replacing { None } else { pages::spread_at_insertion(target, at) };
    let mut source_groups = HashMap::new();
    if !replacing && destination_group.is_none() {
        let mut previous: Option<u64> = None;
        for range in crate::data_merge::layout::spread_ranges(&source) {
            if source.pages.get(range.start).and_then(|page| page.spread_group).is_none() {
                continue;
            }
            let selected: Vec<_> = indices.iter().filter(|index| range.contains(index)).copied().collect();
            if selected.len() >= 2 || (range.len() == 1 && selected.len() == 1) {
                let group = match previous {
                    Some(id) => id.checked_add(1).ok_or_else(CommandError::out_of_range)?,
                    None => pages::next_spread_group(&document)?,
                };
                source_groups.extend(selected.into_iter().map(|index| (index, group)));
                previous = Some(group);
            }
        }
    }
    let mut new_pages = Vec::with_capacity(indices.len());
    let mut new_ids = HashMap::new();
    for (offset, &index) in indices.iter().enumerate() {
        let [_, _, w, h] = rects[index];
        let id = if replacing { target.pages[at + offset].id } else { document.alloc_page_id() };
        let mut page = Page::new(id, [w, h]);
        page.name.clone_from(&source.pages[index].name);
        page.tag_colour.clone_from(&source.pages[index].tag_colour);
        page.fold = source.pages[index].fold;
        page.column_guides = source.pages[index].column_guides;
        page.spread_group = if replacing {
            target.pages[at + offset].spread_group
        } else {
            destination_group.or_else(|| source_groups.get(&index).copied())
        };
        page.spread_spine = if replacing {
            target.pages[at + offset].spread_spine
        } else if destination_group.is_none() && source_groups.contains_key(&index) {
            source.pages[index].spread_spine
        } else {
            None
        };
        new_ids.insert(source.pages[index].id, id);
        new_pages.push(page);
    }
    document.pages.splice(at..if replacing { end } else { at }, new_pages);
    if !replacing {
        if at == 0 && document.fixed_spreads.is_none() && crate::data_merge::layout::has_right_cover(&document) {
            document.pages[0].spread_group = None;
        }
        for (index, section) in document.sections.iter_mut().enumerate().skip(1) {
            if section.start_page > at && Some(index) != retained_section_start {
                section.start_page += indices.len();
            }
        }
    }
    pages::preserve_content(target, &mut document);
    let destinations = crate::scene::page_rects(&document);
    let shifts: HashMap<_, _> = indices.iter().enumerate().map(|(offset, &index)| {
        let rect = destinations[at + offset];
        (index, (document.pages[at + offset].id, [rect[0] - rects[index][0], rect[1] - rects[index][1]]))
    }).collect();

    import_swatches(&mut document, &bundle);
    let map = paste_bundle_mapped(&mut document, &bundle, None, [0.0; 2]);
    let mut names: Vec<_> = document.master_pages.spread_roots().filter_map(|id| document.get_object(id))
        .map(|root| root.name.clone()).collect();
    for old in source.master_pages.spread_roots().filter(|id| map.contains_key(id)) {
        let root = source.get_object(old).ok_or_else(CommandError::not_found)?;
        let name = unique_name(&names, &root.name);
        names.push(name.clone());
        let pages = source.master_pages.spread_pages(&old).iter().map(|id|
            map.get(id).copied().ok_or_else(CommandError::not_found)).collect::<Result<Vec<_>, _>>()?;
        for (index, &id) in pages.iter().enumerate() {
            let root = document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
            root.name = masters::physical_name(&name, index);
        }
        document.master_pages.roots.extend(&pages);
        if let Some(colour) = source.master_pages.tag_colour(old) {
            document.master_pages.tag_colours.insert(pages[0].0, colour.to_owned());
        }
        if let Some(setup) = source.master_pages.page_setup(old) {
            document.master_pages.page_setup.insert(pages[0].0, setup.clone());
        }
        if let Some(spread) = source.master_pages.spread(old) {
            document.master_pages.spreads.push(masters::MasterSpread { pages, spine: spread.spine });
        }
    }
    for (&old, &new) in &map {
        let copy = document.find_object_mut(new.0).ok_or_else(CommandError::not_found)?;
        if let Some((page, [dx, dy])) = page_of.get(&old).and_then(|index| shifts.get(index)).copied() {
            copy.page_id = Some(page);
            copy.pos[0] += dx;
            copy.pos[1] += dy;
        }
        if let crate::ObjectKind::Curve(curve) = &mut copy.kind {
            curve.source_object = curve.source_object.and_then(|id| map.get(&ObjectId(id)).map(|copy| copy.0));
        }
    }
    for binding in &source.master_pages.master_assignments {
        if let Some(&target) = map.get(&binding.target) {
            document.master_pages.master_assignments.push(masters::MasterAssignment {
                target, inherited: remap_master(&binding.inherited, &map)?,
            });
        }
    }
    for (offset, &index) in indices.iter().enumerate() {
        let old = &source.pages[index];
        let page = &mut document.pages[at + offset];
        page.artboard_id = old.artboard_id.and_then(|id| map.get(&id).copied());
        if page.artboard_id.is_some() && source.artboard_backing_pages.contains(&old.id) {
            document.artboard_backing_pages.insert(page.id);
        }
        for assignment in source.master_pages.on_page(old.id) {
            document.master_pages.assignments.push(masters::Assignment {
                page: page.id, stack_slot: assignment.stack_slot, inherited: remap_master(&assignment.inherited, &map)?,
            });
        }
    }
    copy_document_metadata(&source, &mut document, &map, &bundle, factor);
    masters::materialize_groups(&mut document);
    masters::arrange_masters(&mut document);
    masters::lock_positions(&mut document);
    let imported_symbols = crate::commands::symbols::page_import::import(&source, &mut document, &map, &new_ids, Some(directory))?;
    let omitted_links = navigation::copy(&source, target, &mut document, &bundle, &map, &excerpts,
        navigation::PageTransfer::Import { indices: &indices, at })?;
    document.validate_refs().map_err(CommandError::from)?;

    for mut guide in incoming.into_iter().flat_map(|guides| guides.items) {
        if let Some(id) = guide.master_id {
            let Some(&master) = map.get(&id) else { continue };
            guide.master_id = Some(master);
        } else {
            let Some(&page) = guide.page_id.and_then(|id| new_ids.get(&id)) else { continue };
            guide.page_id = Some(page);
        }
        guide.id = guides.next_id();
        guide.position *= factor;
        guides.insert(guide).map_err(|_| CommandError::invalid_params())?;
    }
    let ids: Vec<_> = document.pages[at..end].iter().map(|page| page.id).collect();
    Ok(PageImport {
        document, selection, guides,
        result: json!({"id": ids[0], "ids": ids, "replaced": replacing, "omitted_links": omitted_links, "imported_symbols": imported_symbols}),
    })
}

fn remap_master(
    source: &masters::InheritedMaster, ids: &HashMap<ObjectId, ObjectId>,
) -> Result<masters::InheritedMaster, CommandError> {
    Ok(masters::InheritedMaster {
        master: *ids.get(&source.master).ok_or_else(CommandError::not_found)?,
        group: source.group.and_then(|id| ids.get(&id).copied()),
        group_basis: source.group_basis,
        links: source.links.iter().filter_map(|(original, instance)| Some((*ids.get(original)?, *ids.get(instance)?))).collect(),
        placement: source.placement,
        matching_overrides: source.matching_overrides.iter()
            .filter_map(|(id, keys)| Some((ids.get(&ObjectId(*id))?.0, keys.clone()))).collect(),
    })
}

pub(crate) fn copy_document_metadata(
    source: &Document,
    destination: &mut Document,
    map: &HashMap<ObjectId, ObjectId>,
    bundle: &crate::clipboard::ClipboardBundle,
    factor: f32,
) {
    for mut frame in source.picture_frames.iter().cloned() {
        if let Some(&id) = map.get(&frame.object) {
            frame.object = id;
            frame.image = destination.placed_resource(id);
            frame.content.offset = frame.content.offset.map(|value| value * factor);
            if let Some(layout) = &mut frame.manual_layout {
                layout.fit_size = layout.fit_size.map(|value| value * factor);
                layout.anchor_size = layout.anchor_size.map(|value| value * factor);
                if frame.fit == crate::commands::picture_frame::ImageFit::Native {
                    frame.content.scale = frame.content.scale.map(|value| value * factor);
                }
            }
            destination.picture_frames.push(frame);
        }
    }
    destination.layer_meta.extend(source.layer_meta.iter().filter_map(|meta| {
        let mut copy = meta.clone();
        copy.object = *map.get(&meta.object)?;
        Some(copy)
    }));
    destination.text_wraps.entries.extend(source.text_wraps.entries.iter().filter_map(|entry| {
        let mut copy = entry.clone();
        copy.object = *map.get(&entry.object)?;
        if let Some(wrap) = &mut copy.wrap {
            wrap.distance = wrap.distance.map(|value| value * factor);
        }
        Some(copy)
    }));
    for rule in &source.font_substitutions {
        let is_used = bundle.stories.iter().flat_map(|story| &story.ranges).any(|range| {
            let family = range["family"].as_str().unwrap_or_default();
            let style = range["style"].as_str().unwrap_or_default();
            rule.source.matches(family, style)
        });
        if is_used && !destination.font_substitutions.iter().any(|old| old.source.matches(&rule.source.family, &rule.source.style)) {
            destination.font_substitutions.push(rule.clone());
        }
    }
}
