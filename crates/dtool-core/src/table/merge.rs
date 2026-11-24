use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use super::commands::parse_cell_id;
use super::model::{CellId, TableCell, TableNode};
use super::selection::{self, TableSelection};
use crate::{Command, CommandError, CommandResult, Document, ObjectId, ObjectKind, Session, Story, StoryId};

pub const MERGE: &str = "table.cell.merge";
pub const UNMERGE: &str = "table.cell.unmerge";

#[derive(Clone, Copy, PartialEq, Eq)]
struct Bounds {
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
}

impl Bounds {
    fn cell(cell: &TableCell) -> Self {
        Self { top: cell.row, left: cell.col, bottom: cell.row + cell.row_span, right: cell.col + cell.col_span }
    }

    fn union(self, other: Self) -> Self {
        Self {
            top: self.top.min(other.top), left: self.left.min(other.left),
            bottom: self.bottom.max(other.bottom), right: self.right.max(other.right),
        }
    }

    fn intersects(self, other: Self) -> bool {
        self.top < other.bottom && other.top < self.bottom && self.left < other.right && other.left < self.right
    }

    fn has_slot(self, cell: &TableCell) -> bool {
        (self.top..self.bottom).contains(&cell.row) && (self.left..self.right).contains(&cell.col)
    }
}

pub fn range_cells(table: &TableNode, first: CellId, last: CellId) -> Option<Vec<CellId>> {
    let first = table.cells.iter().find(|cell| cell.id == first)?;
    let last = table.cells.iter().find(|cell| cell.id == last)?;
    let bounds = Bounds::cell(first).union(Bounds::cell(last));
    Some(table.cells.iter().filter(|cell| cell.merged_into.is_none() && bounds.intersects(Bounds::cell(cell))).map(|cell| cell.id).collect())
}

enum Edit {
    Merge { bounds: Bounds, anchor: CellId, covered: Vec<CellId> },
    Unmerge(HashSet<CellId>),
}

fn endpoint(point: &Value, table: &TableNode) -> Option<(u32, u32)> {
    if let Some(id) = parse_cell_id(point) {
        let cell = table.cells.iter().find(|cell| cell.id == id)?;
        return Some((cell.row, cell.col));
    }
    Some((u32::try_from(point.get("row")?.as_u64()?).ok()?, u32::try_from(point.get("col")?.as_u64()?).ok()?))
}

fn plan(session: &Session, command: &Command) -> Result<(ObjectId, Edit), CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let params = &command.params;
    let active = selection::current(session);
    let id = match params.get("id").or_else(|| params.get("object_id")) {
        Some(id) => ObjectId(id.as_u64().ok_or_else(CommandError::invalid_params)?),
        None => active.table.ok_or_else(CommandError::not_found)?,
    };
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if object.locked { return Err(CommandError::new("LOCKED")); }
    let ObjectKind::Table(table) = &object.kind else { return Err(CommandError::invalid_params()); };
    let invalid = CommandError::invalid_params;
    if command.name == MERGE {
        if params.get("content_policy").is_some_and(|policy| policy.as_str() != Some("preserve_all")) {
            return Err(invalid());
        }
        if let Some(range) = params.get("range") {
            let (top, left) = endpoint(&range["from"], table).ok_or_else(invalid)?;
            let (bottom, right) = endpoint(&range["to"], table).ok_or_else(invalid)?;
            if top > bottom || left > right || bottom as usize >= table.row_heights_pt.len() || right as usize >= table.col_widths_pt.len() {
                return Err(invalid());
            }
            let bounds = Bounds { top, left, bottom: bottom + 1, right: right + 1 };
            let owners = table.cells.iter().filter(|cell| cell.merged_into.is_none() && bounds.intersects(Bounds::cell(cell)));
            return Ok((id, plan_merge(table, owners, Some(bounds))?));
        }
    }
    let requested = if let Some(cells) = params.get("cells") {
        cells.as_array().ok_or_else(invalid)?.iter().map(|id| parse_cell_id(id).ok_or_else(invalid)).collect::<Result<Vec<_>, _>>()?
    } else if let Some(cell) = params.get("cell") {
        vec![parse_cell_id(cell).ok_or_else(invalid)?]
    } else if active.table == Some(id) {
        active.cells
    } else {
        return Err(invalid());
    };
    let by_id: HashMap<_, _> = table.cells.iter().map(|cell| (cell.id, cell)).collect();
    let mut seen = HashSet::new();
    let mut owners = Vec::new();
    for id in requested {
        let cell = by_id.get(&id).ok_or_else(invalid)?;
        let owner = cell.merged_into.unwrap_or(id);
        if seen.insert(owner) {
            owners.push(*by_id.get(&owner).filter(|cell| cell.merged_into.is_none()).ok_or_else(invalid)?);
        }
    }
    let edit = if command.name == MERGE {
        plan_merge(table, owners.into_iter(), None)?
    } else {
        let anchors: HashSet<_> = owners.into_iter().filter(|cell| cell.row_span > 1 || cell.col_span > 1).map(|cell| cell.id).collect();
        if anchors.is_empty() { return Err(invalid()); }
        Edit::Unmerge(anchors)
    };
    Ok((id, edit))
}

fn plan_merge<'a>(table: &TableNode, owners: impl Iterator<Item = &'a TableCell>, requested: Option<Bounds>) -> Result<Edit, CommandError> {
    let invalid = CommandError::invalid_params;
    let owners: Vec<_> = owners.collect();
    let bounds = owners.iter().map(|cell| Bounds::cell(cell)).reduce(Bounds::union).ok_or_else(invalid)?;
    if owners.len() < 2 || requested.is_some_and(|requested| requested != bounds) {
        return Err(invalid());
    }
    let selected: HashSet<_> = owners.iter().map(|cell| cell.id).collect();
    let anchor = owners.iter().find(|cell| cell.row == bounds.top && cell.col == bounds.left).ok_or_else(invalid)?.id;
    let mut slots: Vec<_> = table.cells.iter().filter(|cell| bounds.has_slot(cell)).collect();
    let area = (bounds.bottom - bounds.top) as usize * (bounds.right - bounds.left) as usize;
    if slots.len() != area || slots.iter().any(|slot| !selected.contains(&slot.merged_into.unwrap_or(slot.id))) {
        return Err(invalid());
    }
    slots.sort_unstable_by_key(|cell| (cell.row, cell.col));
    let covered = slots.into_iter().filter(|cell| cell.id != anchor).map(|cell| cell.id).collect();
    Ok(Edit::Merge { bounds, anchor, covered })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), MERGE | UNMERGE).then(|| plan(session, command).is_ok())
}

fn append_story(target: &mut Story, mut source: Story, offset: usize) {
    target.text.push_str(&source.text);
    for run in source.ranges.iter_mut().chain(source.paras.iter_mut()) {
        let pin = run.get("pin").is_some();
        for key in ["start", "end"] {
            if let Some(index) = run[key].as_u64() { run[key] = json!(index + offset as u64); }
        }
        if pin {
            if let Some(index) = run["index"].as_u64() { run["index"] = json!(index + offset as u64); }
            run["story"] = json!(target.id.0);
        }
    }
    target.ranges.append(&mut source.ranges);
    target.paras.append(&mut source.paras);
    target.specials.extend(source.specials.into_iter().map(|(at, mark)| (at + offset, mark)));
}

fn rebase_positions(before: &Document, after: &mut Document, anchor: StoryId, moved: &HashMap<StoryId, usize>) {
    let relocate = |position| {
        before.story_at_pos(position).map_or(position, |(story, at)| {
            match moved.get(&story) {
                Some(offset) => after.story_pos(anchor, offset + at),
                None => after.story_pos(story, at),
            }
        })
    };
    let anchors = before.anchors.iter().cloned().map(|mut value| { value.story_pos = relocate(value.story_pos); value }).collect();
    let hyperlinks = before.hyperlinks.iter().cloned().map(|mut value| { value.range = (relocate(value.range.0), relocate(value.range.1)); value }).collect();
    let notes = before.notes.notes.iter().cloned().map(|mut value| { value.story_pos = relocate(value.story_pos); value }).collect();
    let marks = before.index.marks.iter().cloned().map(|mut value| { value.story_pos = relocate(value.story_pos); value }).collect();
    after.anchors = anchors;
    after.hyperlinks = hyperlinks;
    after.notes.notes = notes;
    after.index.marks = marks;
}

fn move_content(before: &Document, next: &mut Document, id: ObjectId, anchor: (CellId, StoryId), sources: &[(CellId, StoryId)]) -> Result<Vec<Value>, CommandError> {
    if crate::frame::story_owners(before, anchor.1).as_slice() != [crate::frame::StoryOwner::TableCell(id, anchor.0)] {
        return Err(CommandError::invalid_params());
    }
    let indices: HashMap<_, _> = next.stories.iter().enumerate().map(|(index, story)| (story.id, index)).collect();
    let target_index = indices.get(&anchor.1).copied().unwrap_or_else(|| {
        next.stories.push(Story { id: anchor.1, ..Story::default() });
        next.stories.len() - 1
    });
    let mut moved = HashMap::new();
    let mut changes = Vec::new();
    for &(cell, source) in sources {
        let Some(&index) = indices.get(&source) else { continue; };
        if next.stories[index].text.is_empty() { continue; }
        if crate::frame::story_owners(before, source).as_slice() != [crate::frame::StoryOwner::TableCell(id, cell)] {
            return Err(CommandError::invalid_params());
        }
        let content = std::mem::replace(&mut next.stories[index], Story { id: source, ..Story::default() });
        let target = &mut next.stories[target_index];
        if !target.text.is_empty() { target.text.push('\n'); }
        let start = target.text.chars().count();
        let end = start + content.text.chars().count();
        append_story(target, content, start);
        moved.insert(source, start);
        changes.push(json!({"cell": cell.text(), "story_id": source.0, "range": [start, end]}));
    }
    if !moved.is_empty() { rebase_positions(before, next, anchor.1, &moved); }
    Ok(changes)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    matches!(command.name.as_str(), MERGE | UNMERGE).then(|| {
        let (id, edit) = plan(session, command)?;
        let mut next = session.document.clone();
        let object = next.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        let ObjectKind::Table(table) = &mut object.kind else { return Err(CommandError::invalid_params()); };
        let (cells, details) = match edit {
            Edit::Merge { bounds, anchor, covered } => {
                let order: HashMap<_, _> = covered.iter().enumerate().map(|(index, id)| (*id, index)).collect();
                let mut sources = Vec::with_capacity(covered.len());
                let mut anchor_story = None;
                let row_span = bounds.bottom - bounds.top;
                let col_span = bounds.right - bounds.left;
                for cell in &mut table.cells {
                    if cell.id == anchor {
                        anchor_story = Some(cell.story);
                        (cell.row_span, cell.col_span) = (row_span, col_span);
                    } else if order.contains_key(&cell.id) {
                        (cell.row_span, cell.col_span) = (1, 1);
                        cell.merged_into = Some(anchor);
                        sources.push((cell.id, cell.story));
                    }
                }
                sources.sort_unstable_by_key(|(id, _)| order[id]);
                let moved = move_content(&session.document, &mut next, id, (anchor, anchor_story.ok_or_else(CommandError::not_found)?), &sources)?;
                (vec![anchor], json!({"anchor":anchor.text(), "row_span":row_span, "col_span":col_span, "covered":covered.iter().map(CellId::text).collect::<Vec<_>>(), "moved":moved}))
            }
            Edit::Unmerge(anchors) => {
                let mut separated = Vec::new();
                for cell in &mut table.cells {
                    if anchors.contains(&cell.id) || cell.merged_into.is_some_and(|owner| anchors.contains(&owner)) {
                        cell.merged_into = None;
                        (cell.row_span, cell.col_span) = (1, 1);
                        separated.push(cell.id);
                    }
                }
                let details = json!({"separated": separated.iter().map(CellId::text).collect::<Vec<_>>()});
                (separated, details)
            }
        };
        session.text_edit = None;
        crate::commands::document_change::commit_table(session, &command.name, next, vec![id], TableSelection { table: Some(id), cells, tracks: None }, details)
    })
}
