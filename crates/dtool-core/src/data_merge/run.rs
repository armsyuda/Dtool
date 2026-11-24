use crate::document::Document;
use crate::object::ObjectId;
use crate::workspace::{ContextId, Workspace};
use crate::CommandError;
use super::layout::{
    capacity, cell_offset, clone_template, ensure_pages, object_page_index, page_origin,
    slot_row_column, template_ids, template_page_indices,
};
use super::{
    apply_rules, descendants, field_value, fill_objects, merge_warning, run_layout_one_page, select_range,
    MergeBinding, MergeEntry, MergeLayout, RowWarning,
};

fn collect_entries(doc: &Document, warnings: &mut Vec<RowWarning>) -> Result<Vec<MergeEntry>, CommandError> {
    let mut entries = Vec::new();
    for (source, src) in doc.merge_sources.iter().enumerate() {
        if !src.enabled {
            continue;
        }
        let filtered = apply_rules(src)?;
        warnings.extend(filtered.warnings.iter().cloned());
        let (records, originals) = select_range(filtered, &src.records_range);
        for (row, original) in records.into_iter().zip(originals) {
            entries.push(MergeEntry { source, row, original });
        }
    }
    if entries.is_empty() {
        return Err(CommandError::new("MERGE_NO_DATA"));
    }
    Ok(entries)
}

pub(super) fn run_layout(
    doc: &Document,
    out: &mut Document,
    layout: &MergeLayout,
    entries: &[MergeEntry],
    warnings: &mut Vec<RowWarning>,
) -> usize {
    let used = run_layout_one_page(doc, out, layout, entries, warnings);
    let Some(frame) = doc.get_object(layout.object_id) else {
        return used;
    };
    let children = descendants(doc, layout.object_id);
    if children.is_empty() {
        return used;
    }
    let cap = capacity(layout);
    if cap == 0 {
        return used;
    }
    let advance = layout.record_advance.max(1) as usize;
    let start = layout.record_offset as usize;
    let records: Vec<usize> = (start..entries.len()).step_by(advance).collect();
    if records.len() <= cap {
        return used;
    }
    let page_ids = template_page_indices(doc);
    let cycle_pages = page_ids.len().max(1);
    let cycles = records.len().div_ceil(cap);
    let template_page = object_page_index(doc, layout.object_id);
    let cycle_index = page_ids.iter().position(|page| *page == template_page).unwrap_or(0);
    ensure_pages(out, (cycles - 1) * cycle_pages + page_ids.iter().copied().max().unwrap_or(0) + 1);
    let frame_size = frame.size;
    let template_page_origin = page_origin(doc, template_page);
    let full_template = template_ids(doc, layout.object_id);
    let mut used = used;
    for cycle in 1..cycles {
        for local in 0..cap {
            let slot = cycle * cap + local;
            if slot >= records.len() {
                break;
            }
            let record = records[slot];
            let (row, column) = slot_row_column(layout, local);
            let cell = cell_offset(layout, local, frame_size);
            let output_page = cycle * cycle_pages + cycle_index;
            let page_shift = [
                page_origin(out, output_page)[0] - template_page_origin[0],
                page_origin(out, output_page)[1] - template_page_origin[1],
            ];
            let offset = [page_shift[0] + cell[0], page_shift[1] + cell[1]];
            let targets: Vec<(ObjectId, ObjectId)> = if row == 0 && column == 0 {
                clone_template(out, &full_template, offset)
                    .into_iter()
                    .filter(|(old, _)| children.contains(old))
                    .collect()
            } else {
                clone_template(out, &children, offset)
            };
            fill_objects(doc, out, &targets, &entries[record], record, warnings);
            used = used.max(record + 1);
        }
    }
    used
}

fn repeat_document(doc: &Document, entries: &[MergeEntry], warnings: &mut Vec<RowWarning>) -> Document {
    let mut out = doc.clone();
    let page_count = doc.pages.len().max(1);
    ensure_pages(&mut out, page_count);
    let template: Vec<ObjectId> = doc.objects.iter().map(|object| object.id).collect();
    let original_targets: Vec<_> = template.iter().map(|&id| (id, id)).collect();
    for (record, entry) in entries.iter().enumerate().skip(1) {
        let start_page = out.pages.len();
        for index in 0..page_count {
            out.push_page(doc.pages.get(index).map_or_else(|| super::layout::page_size(doc), |page| page.size));
        }
        let shift = page_origin(&out, start_page);
        let targets = clone_template(&mut out, &template, shift);
        for &(old, new) in &targets {
            let page = out.pages[start_page + object_page_index(doc, old)].id;
            if let Some(object) = out.find_object_mut(new.0) {
                object.page_id = Some(page);
            }
        }
        fill_objects(doc, &mut out, &targets, entry, record, warnings);
    }
    fill_objects(doc, &mut out, &original_targets, &entries[0], 0, warnings);
    out
}

pub fn run_merge(doc: &Document) -> Result<(Document, Vec<RowWarning>), CommandError> {
    let mut warnings = Vec::new();
    let entries = collect_entries(doc, &mut warnings)?;
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
    if doc.merge_layouts.is_empty() {
        if doc.merge_bindings.iter().any(|binding| matches!(binding, MergeBinding::Qr { .. })) {
            return Ok((repeat_document(doc, &entries, &mut warnings), warnings));
        }
        return super::run_merge_legacy(doc);
    }
    for layout in &doc.merge_layouts {
        run_layout(doc, &mut out, layout, &entries, &mut warnings);
    }
    Ok((out, warnings))
}

pub fn run(workspace: &mut Workspace) -> Result<(ContextId, Vec<RowWarning>), CommandError> {
    let active = workspace.active().ok_or_else(CommandError::no_document)?;
    let ctx = workspace.get(active).ok_or_else(CommandError::context_not_found)?;
    let doc = ctx.session.document().cloned().ok_or_else(CommandError::no_document)?;
    let title = format!("{} merge", ctx.title);
    let (out, warnings) = run_merge(&doc)?;
    let id = workspace.new_document(&title);
    if let Some(ctx) = workspace.get_mut(id) {
        ctx.session.document = out;
        ctx.session.has_document = true;
    }
    Ok((id, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::document::{Document, Story, Units};
    use crate::object::{Object, ObjectId, ObjectKind};
    use crate::table::model::StoryId;
    use crate::data_merge::{Corner, DataSource, FileFormat, MergeLayout, PageRepeat, RecordRange, SourceKind};

    fn sample_source(records: Vec<Vec<String>>) -> DataSource {
        DataSource {
            name: "src".to_string(),
            kind: SourceKind::File { path: PathBuf::from("records.csv") },
            format: FileFormat::Csv { delimiter: ',', quote: Some('"') },
            fields: vec!["name".to_string()],
            records,
            enabled: true,
            include_headers: true,
            numbers_as_text: false,
            case_sensitive: false,
            records_range: RecordRange::All,
            pages: PageRepeat::All,
            rules: Vec::new(),
            scripts: Vec::new(),
            fetched_hash: String::new(),
        }
    }

    fn layout_doc(records: usize) -> Document {
        let mut doc = Document::new([600.0, 400.0], Units::Px, 72.0, 1);
        let mut frame = Object::default();
        frame.id = ObjectId(1);
        frame.size = [600.0, 400.0];
        let mut cell = Object::default();
        cell.id = ObjectId(2);
        cell.parent_id = Some(ObjectId(1));
        cell.size = [100.0, 40.0];
        cell.kind = ObjectKind::FrameText { story: StoryId(1) };
        doc.objects.push(frame);
        doc.objects.push(cell);
        doc.next_object_id = 3;
        doc.stories.push(Story {
            id: StoryId(1),
            text: "name".to_string(),
            ranges: vec![serde_json::json!({"field": {"MergeField": "name"}, "start": 0, "end": 4})],
            specials: Vec::new(),
            paras: Vec::new(),
        });
        doc.merge_sources.push(sample_source((0..records).map(|i| vec![format!("r{i}")]).collect()));
        doc.merge_layouts.push(MergeLayout {
            object_id: ObjectId(1),
            rows: 2,
            columns: 3,
            gutter: [0.0, 0.0],
            cell: [0.0, 0.0],
            preserve_cell_size: false,
            record_offset: 0,
            record_advance: 1,
            origin: Corner::TopLeft,
            rows_first: true,
        });
        doc
    }

    #[test]
    fn two_by_three_eight_records_adds_second_page() {
        let doc = layout_doc(8);
        let (out, warnings) = run_merge(&doc).expect("merge");
        assert_eq!(out.pages.len(), 2, "expected 2 pages (6 + 2), got {} warnings={:?}", out.pages.len(), warnings);
        let filled: Vec<_> = out.stories.iter().filter(|s| s.text.starts_with('r')).collect();
        assert_eq!(filled.len(), 8);
    }
}
