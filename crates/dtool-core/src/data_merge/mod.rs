pub mod sources;
pub mod rules;
pub mod layout;
pub mod run;
pub mod commands;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::document::Document;
use crate::object::{ObjectId, ObjectKind};
use crate::table::model::StoryId;
use crate::CommandError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SourceKind {
    File { path: PathBuf },
    Url { url: String },
}

pub fn drop_unknown_sources(raw: &mut serde_json::Value) {
    if let Some(sources) = raw.get_mut("merge_sources").and_then(|value| value.as_array_mut()) {
        sources.retain(|src| src.get("kind").map_or(true, |kind| serde_json::from_value::<SourceKind>(kind.clone()).is_ok()));
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FileFormat {
    Csv { delimiter: char, quote: Option<char> },
    Json,
    Xlsx { sheet: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSource {
    pub name: String,
    pub kind: SourceKind,
    pub format: FileFormat,
    pub fields: Vec<String>,
    pub records: Vec<Vec<String>>,
    pub enabled: bool,
    pub include_headers: bool,
    pub numbers_as_text: bool,
    pub case_sensitive: bool,
    pub records_range: RecordRange,
    pub pages: PageRepeat,
    pub rules: Vec<MergeRule>,
    pub scripts: Vec<MergeScript>,
    pub fetched_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RecordRange {
    All,
    Range(usize, usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PageRepeat {
    All,
    Range(usize, usize),
    Repeat(usize, usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeRule {
    pub match_all: bool,
    pub clauses: Vec<Clause>,
    pub action: RuleAction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Clause {
    FieldEquals(String, String),
    FieldContains(String, String),
    FieldRegex(String, String),
    FieldNotEquals(String, String),
    FieldBeginsWith(String, String),
    FieldEndsWith(String, String),
    FieldLessThan(String, String),
    FieldLessOrEqual(String, String),
    FieldGreaterThan(String, String),
    FieldGreaterOrEqual(String, String),
    And(Vec<Clause>),
    Or(Vec<Clause>),
    Nor(Vec<Clause>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuleAction {
    Skip,
    Warn,
    WarnSkip,
    ErrorStop,
    Prefix(String),
    Postfix(String),
    ReplaceAll(String),
    ReplaceMatched(String),
    RegexReplace(String, String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeScript {
    pub name: String,
    pub code: String,
    pub pre_parse: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MergeBinding {
    Text { field: String },
    Image { object_id: ObjectId, field: String },
    Qr { object_id: ObjectId, field: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    MergeField(String),
    RecordIndex,
    OriginalRecordIndex,
    SourceName,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeLayout {
    pub object_id: ObjectId,
    pub rows: u32,
    pub columns: u32,
    pub gutter: [f32; 2],
    pub cell: [f32; 2],
    pub preserve_cell_size: bool,
    pub record_offset: u32,
    pub record_advance: u32,
    pub origin: Corner,
    pub rows_first: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RowWarning {
    pub row_index: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FilteredRecords {
    pub records: Vec<Vec<String>>,
    pub original_indices: Vec<usize>,
    pub warnings: Vec<RowWarning>,
}

pub fn load_source(src: &mut DataSource) -> Result<Vec<RowWarning>, CommandError> {
    sources::load_source(src)
}

pub fn apply_rules(src: &DataSource) -> Result<FilteredRecords, CommandError> {
    rules::apply_rules(src)
}

pub fn add_field(src: &mut DataSource, name: &str, value: &str) -> Result<(), CommandError> {
    rules::add_field(src, name, value)
}

pub fn discard_field(src: &mut DataSource, name: &str) -> Result<(), CommandError> {
    rules::discard_field(src, name)
}

pub(crate) struct MergeEntry {
    pub(crate) source: usize,
    pub(crate) row: Vec<String>,
    pub(crate) original: usize,
}

pub(crate) fn merge_warning(row_index: usize, message: String) -> RowWarning {
    RowWarning { row_index, message }
}

pub(crate) fn select_range(filtered: FilteredRecords, range: &RecordRange) -> (Vec<Vec<String>>, Vec<usize>) {
    let total = filtered.records.len();
    let (from, to) = match range {
        RecordRange::All => (0, total),
        RecordRange::Range(start, end) => {
            let from = start.saturating_sub(1).min(total);
            let to = (*end).min(total).max(from);
            (from, to)
        }
    };
    let records = filtered.records.into_iter().skip(from).take(to - from).collect();
    let indices = filtered.original_indices.into_iter().skip(from).take(to - from).collect();
    (records, indices)
}

pub(crate) fn field_value(src: &DataSource, row: &[String], name: &str) -> Option<String> {
    let index = src.fields.iter().position(|f| {
        if src.case_sensitive {
            f == name
        } else {
            f.to_lowercase() == name.to_lowercase()
        }
    })?;
    Some(row.get(index).cloned().unwrap_or_default())
}

fn resolve_kind(doc: &Document, entry: &MergeEntry, sequence: usize, kind: &FieldKind) -> Option<String> {
    let src = doc.merge_sources.get(entry.source)?;
    match kind {
        FieldKind::MergeField(name) => field_value(src, &entry.row, name),
        FieldKind::RecordIndex => Some((sequence + 1).to_string()),
        FieldKind::OriginalRecordIndex => Some((entry.original + 1).to_string()),
        FieldKind::SourceName => Some(src.name.clone()),
    }
}

fn range_field(value: &serde_json::Value) -> Option<(usize, usize, FieldKind)> {
    let map = value.as_object()?;
    let kind: FieldKind = serde_json::from_value(map.get("field")?.clone()).ok()?;
    let start = map.get("start")?.as_u64()? as usize;
    let end = map.get("end")?.as_u64()? as usize;
    if end < start {
        return None;
    }
    Some((start, end, kind))
}

fn shift_bound(value: &mut serde_json::Value, key: &str, at: usize, delta: isize) {
    if let Some(map) = value.as_object_mut() {
        if let Some(pos) = map.get(key).and_then(|v| v.as_u64()) {
            let pos = pos as usize;
            if pos >= at {
                let moved = (pos as isize + delta).max(0) as usize;
                map.insert(key.to_string(), serde_json::json!(moved));
            }
        }
    }
}

fn fill_story(out: &mut Document, story: StoryId, entry: &MergeEntry, sequence: usize, warnings: &mut Vec<RowWarning>) -> usize {
    let Some(index) = out.stories.iter().position(|s| s.id == story) else {
        warnings.push(merge_warning(entry.original, format!("story {} missing; skipped", story.0)));
        return 0;
    };
    let mut runs: Vec<(usize, usize, usize, FieldKind)> = out.stories[index]
        .ranges
        .iter()
        .enumerate()
        .filter_map(|(i, r)| range_field(r).map(|(s, e, k)| (i, s, e, k)))
        .collect();
    runs.sort_by(|a, b| b.1.cmp(&a.1));
    let mut filled = 0;
    for (range_index, start, end, kind) in runs {
        let Some(value) = resolve_kind(out, entry, sequence, &kind) else {
            warnings.push(merge_warning(entry.original, format!("field {:?} not in source; skipped", kind)));
            continue;
        };
        let story = &mut out.stories[index];
        let chars: Vec<char> = story.text.chars().collect();
        if end > chars.len() {
            warnings.push(merge_warning(entry.original, format!("field run {}..{} outside story {}; skipped", start, end, story.id.0)));
            continue;
        }
        let mut text: String = chars[..start].iter().collect();
        text.push_str(&value);
        text.extend(chars[end..].iter());
        story.text = text;
        let delta = value.chars().count() as isize - (end - start) as isize;
        for (i, range) in story.ranges.iter_mut().enumerate() {
            if i == range_index {
                if let Some(map) = range.as_object_mut() {
                    map.insert("end".to_string(), serde_json::json!((end as isize + delta) as usize));
                }
                continue;
            }
            shift_bound(range, "start", end, delta);
            shift_bound(range, "end", end, delta);
        }
        for (pos, _) in story.specials.iter_mut() {
            if *pos >= end {
                *pos = (*pos as isize + delta).max(0) as usize;
            }
        }
        filled += 1;
    }
    filled
}

fn source_dir(src: &DataSource) -> Option<PathBuf> {
    match &src.kind {
        SourceKind::File { path } => Some(path.parent()?.to_path_buf()),
        _ => None,
    }
}

fn fill_image(out: &mut Document, object_id: ObjectId, value: &str, entry: &MergeEntry, warnings: &mut Vec<RowWarning>) {
    if value.is_empty() {
        warnings.push(merge_warning(entry.original, format!("image field empty for object {}; skipped", object_id.0)));
        return;
    }
    let raw = PathBuf::from(value);
    let path = if raw.is_absolute() {
        raw
    } else {
        match source_dir(&out.merge_sources[entry.source]) {
            Some(dir) => dir.join(raw),
            None => raw,
        }
    };
    let existing = out
        .resources
        .iter()
        .find(|r| r.path.as_ref() == Some(&path) || r.display_name() == value)
        .map(|r| r.id);
    if let Some(resource) = existing {
        if let Err(e) = out.place_image(object_id, resource) {
            warnings.push(merge_warning(entry.original, format!("image place failed for object {}: {}", object_id.0, e.code)));
        }
        return;
    }
    let Some(placed) = out.placed_resource(object_id) else {
        warnings.push(merge_warning(entry.original, format!("object {} has no placed image; skipped", object_id.0)));
        return;
    };
    let Some(mut resource) = out.image_resource(placed).cloned() else {
        warnings.push(merge_warning(entry.original, format!("resource {} missing for object {}; skipped", placed.0, object_id.0)));
        return;
    };
    resource.id = crate::resources::ResourceId(0);
    resource.make_linked(path.clone(), None);
    if resource.status == crate::resources::ResourceStatus::Missing {
        warnings.push(merge_warning(entry.original, format!("image file missing: {}", path.display())));
    }
    let id = out.add_image_resource(resource);
    if let Err(e) = out.place_image(object_id, id) {
        warnings.push(merge_warning(entry.original, format!("image place failed for object {}: {}", object_id.0, e.code)));
    }
}

fn fill_qr(out: &mut Document, object_id: ObjectId, value: &str, entry: &MergeEntry, warnings: &mut Vec<RowWarning>) {
    let Some(index) = out.objects.iter().position(|o| o.id == object_id) else {
        warnings.push(merge_warning(entry.original, format!("QR object {} missing; skipped", object_id.0)));
        return;
    };
    let object = &mut out.objects[index];
    if crate::commands::qr::payload(object).is_none() {
        warnings.push(merge_warning(entry.original, format!("object {} is not a QR code; skipped", object_id.0)));
    } else if crate::commands::qr::encode(value).is_err() {
        warnings.push(merge_warning(entry.original, format!("QR payload for object {} cannot be encoded; skipped", object_id.0)));
    } else {
        object.qr_payload = Some(value.to_owned());
        object.qr_content = None;
    }
}

pub(crate) fn fill_objects(
    doc: &Document,
    out: &mut Document,
    targets: &[(ObjectId, ObjectId)],
    entry: &MergeEntry,
    sequence: usize,
    warnings: &mut Vec<RowWarning>,
) {
    for (_, target) in targets {
        let story = out.get_object(*target).and_then(|o| o.story_id());
        if let Some(story) = story {
            fill_story(out, story, entry, sequence, warnings);
        }
    }
    let Some(src) = doc.merge_sources.get(entry.source) else {
        return;
    };
    for binding in &doc.merge_bindings {
        match binding {
            MergeBinding::Text { .. } => {}
            MergeBinding::Image { object_id, field } => {
                let Some((_, target)) = targets.iter().find(|(from, _)| from == object_id) else {
                    continue;
                };
                if out.get_object(*target).is_none() {
                    warnings.push(merge_warning(entry.original, format!("image object {} missing; skipped", object_id.0)));
                    continue;
                }
                match field_value(src, &entry.row, field) {
                    Some(value) => fill_image(out, *target, &value, entry, warnings),
                    None => warnings.push(merge_warning(entry.original, format!("field {} not in source {}; skipped", field, src.name))),
                }
            }
            MergeBinding::Qr { object_id, field } => {
                let Some((_, target)) = targets.iter().find(|(from, _)| from == object_id) else {
                    continue;
                };
                if out.get_object(*target).is_none() {
                    warnings.push(merge_warning(entry.original, format!("QR object {} missing; skipped", object_id.0)));
                    continue;
                }
                match field_value(src, &entry.row, field) {
                    Some(value) => fill_qr(out, *target, &value, entry, warnings),
                    None => warnings.push(merge_warning(entry.original, format!("field {} not in source {}; skipped", field, src.name))),
                }
            }
        }
    }
}

pub(crate) fn descendants(doc: &Document, root: ObjectId) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for object in &doc.objects {
            if object.parent_id == Some(parent) && !out.contains(&object.id) {
                out.push(object.id);
                frontier.push(object.id);
            }
        }
    }
    out
}

pub(crate) fn clone_cell(out: &mut Document, template: &[ObjectId], offset: [f32; 2]) -> Vec<(ObjectId, ObjectId)> {
    let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
    for old in template {
        map.insert(*old, ObjectId(out.next_object_id));
        out.next_object_id += 1;
    }
    let mut next_story = out.peek_story_id().0;
    let mut pairs = Vec::new();
    for old in template {
        let Some(source) = out.get_object(*old).cloned() else {
            continue;
        };
        let mut object = source;
        object.id = map[old];
        object.pos = [object.pos[0] + offset[0], object.pos[1] + offset[1]];
        if let Some(parent) = object.parent_id {
            if let Some(new_parent) = map.get(&parent) {
                object.parent_id = Some(*new_parent);
            }
        }
        if let Some(story) = object.story_id() {
            if let Some(copy) = out.stories.iter().find(|s| s.id == story).cloned() {
                let new_story = StoryId(next_story);
                next_story += 1;
                out.stories.push(crate::document::Story { id: new_story, ..copy });
                match &mut object.kind {
                    ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => *story = new_story,
                    ObjectKind::PathText(data) => {
                        data.story = new_story;
                        if let Some(path) = map.get(&data.path) {
                            data.path = *path;
                        }
                    }
                    _ => {}
                }
            }
        }
        let placed = out.placed_resource(*old);
        let id = object.id;
        out.objects.push(object);
        if let Some(resource) = placed {
            let _ = out.place_image(id, resource);
        }
        pairs.push((*old, id));
    }
    pairs
}

pub(crate) fn cell_size(layout: &MergeLayout, size: [f32; 2], rows: u32, columns: u32) -> [f32; 2] {
    let auto = |total: f32, count: u32, gutter: f32| (total - gutter * (count as f32 - 1.0)) / count as f32;
    [
        if layout.cell[0] > 0.0 { layout.cell[0] } else { auto(size[0], columns, layout.gutter[0]) },
        if layout.cell[1] > 0.0 { layout.cell[1] } else { auto(size[1], rows, layout.gutter[1]) },
    ]
}

pub(crate) fn run_layout_one_page(doc: &Document, out: &mut Document, layout: &MergeLayout, entries: &[MergeEntry], warnings: &mut Vec<RowWarning>) -> usize {
    let Some(frame) = doc.get_object(layout.object_id) else {
        warnings.push(merge_warning(0, format!("merge layout object {} missing; skipped", layout.object_id.0)));
        return 0;
    };
    let template = descendants(doc, layout.object_id);
    if template.is_empty() {
        warnings.push(merge_warning(0, format!("merge layout {} has no template objects; skipped", layout.object_id.0)));
        return 0;
    }
    let rows = layout.rows.max(1);
    let columns = layout.columns.max(1);
    let cell = cell_size(layout, frame.size, rows, columns);
    let advance = layout.record_advance.max(1) as usize;
    let capacity = (rows * columns) as usize;
    let mut used = 0;
    for slot in 0..capacity {
        let record = layout.record_offset as usize + slot * advance;
        if record >= entries.len() {
            break;
        }
        let (row, column) = if layout.rows_first {
            (slot as u32 / columns, slot as u32 % columns)
        } else {
            (slot as u32 % rows, slot as u32 / rows)
        };
        let (row, column) = match layout.origin {
            Corner::TopLeft => (row, column),
            Corner::TopRight => (row, columns - 1 - column),
            Corner::BottomLeft => (rows - 1 - row, column),
            Corner::BottomRight => (rows - 1 - row, columns - 1 - column),
        };
        let offset = [
            column as f32 * (cell[0] + layout.gutter[0]),
            row as f32 * (cell[1] + layout.gutter[1]),
        ];
        let targets: Vec<(ObjectId, ObjectId)> = if row == 0 && column == 0 {
            template.iter().map(|id| (*id, *id)).collect()
        } else {
            clone_cell(out, &template, offset)
        };
        fill_objects(doc, out, &targets, &entries[record], record, warnings);
        used = used.max(record + 1);
    }
    used
}

pub(crate) fn run_merge_legacy(doc: &Document) -> Result<(Document, Vec<RowWarning>), CommandError> {
    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    for (source, src) in doc.merge_sources.iter().enumerate() {
        if !src.enabled {
            continue;
        }
        let filtered = apply_rules(src)?;
        warnings.extend(filtered.warnings.iter().cloned());
        if !matches!(src.pages, PageRepeat::All) {
            warnings.push(merge_warning(0, format!("source {}: page ranges ignored; pages have no object ownership", src.name)));
        }
        let (records, originals) = select_range(filtered, &src.records_range);
        for (row, original) in records.into_iter().zip(originals) {
            entries.push(MergeEntry { source, row, original });
        }
    }
    if entries.is_empty() {
        return Err(CommandError::new("MERGE_NO_DATA"));
    }
    for binding in &doc.merge_bindings {
        if let MergeBinding::Text { field } = binding {
            let known = doc
                .merge_sources
                .iter()
                .filter(|s| s.enabled)
                .any(|s| field_value(s, &[], field).is_some());
            if !known {
                warnings.push(merge_warning(0, format!("text field {} not in any enabled source", field)));
            }
        }
    }
    let mut out = doc.clone();
    let used = if doc.merge_layouts.is_empty() {
        let targets: Vec<(ObjectId, ObjectId)> = doc.objects.iter().map(|o| (o.id, o.id)).collect();
        fill_objects(doc, &mut out, &targets, &entries[0], 0, &mut warnings);
        1
    } else {
        let mut used = 0;
        for layout in &doc.merge_layouts {
            used = used.max(run::run_layout(doc, &mut out, layout, &entries, &mut warnings));
        }
        used
    };
    if used < entries.len() {
        warnings.push(merge_warning(
            used,
            format!("{} records beyond layout capacity skipped; no pages added", entries.len() - used),
        ));
    }
    Ok((out, warnings))
}

pub fn run_merge(doc: &Document) -> Result<(Document, Vec<RowWarning>), CommandError> {
    run::run_merge(doc)
}
