use crate::document::Document;
use crate::table::model::{
    CellFormat, CellFormatMask, CellRotation, FrameFormatMask, InitialAdvance, Insets, TableCell,
    TableFormatDef, TableFrameFormat, TableNode, TablePattern, VerticalAlign,
};

pub const CELL_FILL: u32 = 1 << 0;
pub const CELL_EDGE_LEFT: u32 = 1 << 1;
pub const CELL_EDGE_TOP: u32 = 1 << 2;
pub const CELL_EDGE_RIGHT: u32 = 1 << 3;
pub const CELL_EDGE_BOTTOM: u32 = 1 << 4;
pub const CELL_INSETS: u32 = 1 << 5;
pub const CELL_VERTICAL_ALIGN: u32 = 1 << 6;
pub const CELL_ROTATION: u32 = 1 << 7;
pub const CELL_INITIAL_ADVANCE: u32 = 1 << 8;
pub const CELL_MINIMUM: u32 = 1 << 9;
pub const CELL_PARAGRAPH_STYLE: u32 = 1 << 10;
pub const CELL_EDGES: [u32; 4] = [CELL_EDGE_LEFT, CELL_EDGE_TOP, CELL_EDGE_RIGHT, CELL_EDGE_BOTTOM];

pub const FRAME_FILL: u32 = 1 << 0;
pub const FRAME_EDGE_LEFT: u32 = 1 << 1;
pub const FRAME_EDGE_TOP: u32 = 1 << 2;
pub const FRAME_EDGE_RIGHT: u32 = 1 << 3;
pub const FRAME_EDGE_BOTTOM: u32 = 1 << 4;
pub const FRAME_INSETS: u32 = 1 << 5;
pub const FRAME_IGNORE_BASELINE_GRID: u32 = 1 << 6;
pub const FRAME_BASELINE: u32 = 1 << 7;
pub const FRAME_EDGES: [u32; 4] = [FRAME_EDGE_LEFT, FRAME_EDGE_TOP, FRAME_EDGE_RIGHT, FRAME_EDGE_BOTTOM];

pub fn cell_mask_has(mask: &CellFormatMask, bit: u32) -> bool {
    mask.0 & bit != 0
}

pub fn frame_mask_has(mask: &FrameFormatMask, bit: u32) -> bool {
    mask.0 & bit != 0
}

pub fn factory_insets() -> Insets {
    Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        linked: false,
    }
}

pub fn factory_cell_format() -> CellFormat {
    CellFormat {
        fill: None,
        edges: [None, None, None, None],
        insets: factory_insets(),
        vertical_align: VerticalAlign::Top,
        rotation: CellRotation::R0,
        initial_advance: InitialAdvance::Default,
        minimum_pt: 0.0,
        paragraph_style: None,
    }
}

pub fn factory_frame_format() -> TableFrameFormat {
    TableFrameFormat {
        fill: None,
        edges: [None, None, None, None],
        insets: factory_insets(),
        ignore_baseline_grid: false,
        baseline: None,
    }
}

pub fn format_chain<'a>(doc: &'a Document, table: &TableNode) -> Vec<&'a TableFormatDef> {
    let mut chain: Vec<&'a TableFormatDef> = Vec::new();
    if let Some(id) = table.format {
        if let Some(def) = doc.table_formats.iter().find(|def| def.id == id) {
            chain.push(def);
        }
    }
    if let Some(id) = doc.default_table_format {
        if let Some(def) = doc.table_formats.iter().find(|def| def.id == id) {
            if !chain.iter().any(|known| known.id == def.id) {
                chain.push(def);
            }
        }
    }
    chain
}

pub fn pattern_index(count: u32, pattern_count: u16, lead_band: u16, trail_band: u16, index: u32) -> Option<usize> {
    if pattern_count == 0 || count == 0 || index >= count {
        return None;
    }
    let pattern_count = pattern_count as u32;
    let lead = (lead_band as u32).min(pattern_count);
    let trail = (trail_band as u32).min(pattern_count - lead);
    let lead_used = lead.min(count);
    let trail_used = trail.min(count - lead_used);

    if index < lead_used {
        return Some(index as usize);
    }
    if index >= count - trail_used {
        let from_end = count - index;
        return Some((pattern_count - from_end) as usize);
    }

    let body = pattern_count - lead - trail;
    if body == 0 {
        if lead > 0 {
            return Some((lead - 1) as usize);
        }
        return Some((pattern_count - trail) as usize);
    }
    let offset = index - lead_used;
    Some((lead + offset % body) as usize)
}

pub fn pattern_slot(pattern: &TablePattern, rows: u32, cols: u32, row: u32, col: u32) -> Option<u16> {
    let pattern_row = pattern_index(rows, pattern.rows, pattern.top_band_rows, pattern.bottom_band_rows, row)?;
    let pattern_col = pattern_index(cols, pattern.cols, pattern.left_band_cols, pattern.right_band_cols, col)?;
    let index = pattern_row * pattern.cols as usize + pattern_col;
    pattern.slots.get(index).copied()
}

pub fn slot_format(def: &TableFormatDef, slot: u16) -> Option<&CellFormat> {
    def.cell_formats
        .iter()
        .find(|(id, _, _)| *id == slot as u64)
        .map(|(_, _, format)| format)
        .or_else(|| def.cell_formats.get(slot as usize).map(|(_, _, format)| format))
}

pub fn table_rows(table: &TableNode) -> u32 {
    table.row_heights_pt.len() as u32
}

pub fn table_cols(table: &TableNode) -> u32 {
    table.col_widths_pt.len() as u32
}

pub fn inherited_cell_format(doc: &Document, table: &TableNode, row: u32, col: u32) -> CellFormat {
    let rows = table_rows(table);
    let cols = table_cols(table);
    for def in format_chain(doc, table) {
        if let Some(slot) = pattern_slot(&def.pattern, rows, cols, row, col) {
            if let Some(format) = slot_format(def, slot) {
                return format.clone();
            }
        }
    }
    factory_cell_format()
}

pub fn inherited_frame_format(doc: &Document, table: &TableNode) -> TableFrameFormat {
    match format_chain(doc, table).first() {
        Some(def) => def.frame.clone(),
        None => factory_frame_format(),
    }
}

pub fn resolve_cell(doc: &Document, table: &TableNode, cell: &TableCell) -> CellFormat {
    let mut resolved = inherited_cell_format(doc, table, cell.row, cell.col);
    let local = &cell.local;
    let mask = &cell.mask;

    if cell_mask_has(mask, CELL_FILL) {
        resolved.fill = local.fill.clone();
    }
    for (side, bit) in CELL_EDGES.iter().enumerate() {
        if cell_mask_has(mask, *bit) {
            resolved.edges[side] = local.edges[side].clone();
        }
    }
    if cell_mask_has(mask, CELL_INSETS) {
        resolved.insets = local.insets.clone();
    }
    if cell_mask_has(mask, CELL_VERTICAL_ALIGN) {
        resolved.vertical_align = local.vertical_align;
    }
    if cell_mask_has(mask, CELL_ROTATION) {
        resolved.rotation = local.rotation;
    }
    if cell_mask_has(mask, CELL_INITIAL_ADVANCE) {
        resolved.initial_advance = local.initial_advance;
    }
    if cell_mask_has(mask, CELL_MINIMUM) {
        resolved.minimum_pt = local.minimum_pt;
    }
    if cell_mask_has(mask, CELL_PARAGRAPH_STYLE) {
        resolved.paragraph_style = local.paragraph_style.clone();
    }
    resolved
}

pub(crate) fn story_cell(doc: &Document, id: crate::StoryId) -> Option<(&TableNode, &TableCell)> {
    doc.objects.iter().find_map(|object| {
        let crate::ObjectKind::Table(table) = &object.kind else { return None };
        table.cells.iter().find(|cell| cell.story == id && cell.merged_into.is_none())
            .map(|cell| (table, cell))
    })
}

/// Resolve named cell typography below direct character and paragraph formatting.
/// Keep inherited runs out of the stored story so changing a format stays live.
pub fn styled_cell_story(doc: &Document, table: &TableNode, story: &crate::Story, format: &CellFormat) -> Option<crate::Story> {
    use crate::text_styles::StyleKind;
    use serde_json::{Value, json};

    let preset = format.paragraph_style.as_deref().and_then(|name| doc.text_styles.resolve(StyleKind::Paragraph, name).ok());
    if preset.is_none() && table.text_defaults.is_none() {
        return None;
    }
    let mut character = serde_json::Map::new();
    let mut paragraph = serde_json::Map::new();
    if let Some(defaults) = &table.text_defaults {
        let mut initial = crate::Story::default();
        crate::text_cmd::style_new_story(&mut initial, &json!({ "text_defaults": defaults }), doc).ok()?;
        character = initial.ranges.pop().and_then(|run| run.as_object().cloned()).unwrap_or_default();
        paragraph = initial.paras.pop().and_then(|run| run.as_object().cloned()).unwrap_or_default();
    }
    let merge_character = |base: &mut serde_json::Map<String, Value>, patch: serde_json::Map<String, Value>| {
        if patch.contains_key("family") && patch.get("family") != base.get("family") {
            for key in ["style", "weight", "italic"] {
                base.remove(key);
            }
        }
        base.extend(patch);
    };
    if let Some(preset) = preset {
        merge_character(&mut character, preset.clone());
        paragraph.extend(preset);
    }
    character.entry("size_pt").or_insert_with(|| json!(crate::text_cmd::DEFAULT_SIZE_PT));
    for fields in [&mut character, &mut paragraph] {
        fields.remove("start");
        fields.remove("end");
    }
    let len = story.text.chars().count();
    let mut styled = story.clone();
    let with_defaults = |runs: &[Value], props: &serde_json::Map<String, Value>, is_character| {
        let merge = |base: &mut serde_json::Map<String, Value>, direct| {
            if is_character { merge_character(base, direct); } else { base.extend(direct); }
        };
        if len == 0 {
            let mut base = props.clone();
            for run in runs.iter().filter_map(Value::as_object)
                .filter(|run| !run.contains_key("frame"))
            {
                merge(&mut base, run.clone());
            }
            base.insert("start".into(), json!(0));
            base.insert("end".into(), json!(0));
            return Some(vec![Value::Object(base)]);
        }
        crate::commands::text_styles_cmd::restyle(runs, 0, len, true, |run| {
            if !run.contains_key("frame") {
                let direct = std::mem::take(run);
                run.clone_from(props);
                merge(run, direct);
            }
            Ok(())
        }).ok()
    };
    styled.ranges = with_defaults(&story.ranges, &character, true)?;
    styled.paras = with_defaults(&story.paras, &paragraph, false)?;
    // Empty paragraphs have no scalar interval, so restyling an interval
    // leaves their direct runs untouched.
    for run in styled.paras.iter_mut().filter_map(Value::as_object_mut) {
        if run.get("start") == run.get("end") {
            for (key, value) in &paragraph {
                run.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
    }
    Some(styled)
}

pub fn effective_story<'a>(doc: &Document, story: &'a crate::Story) -> std::borrow::Cow<'a, crate::Story> {
    use std::borrow::Cow;
    let Some((table, cell)) = story_cell(doc, story.id) else { return Cow::Borrowed(story) };
    if let Some(styled) = styled_cell_story(doc, table, story, &resolve_cell(doc, table, cell)) {
        Cow::Owned(styled)
    } else if story.ranges.is_empty() {
        let mut styled = story.clone();
        crate::text_cmd::set_size_range(&mut styled, crate::text_cmd::DEFAULT_SIZE_PT);
        Cow::Owned(styled)
    } else {
        Cow::Borrowed(story)
    }
}

pub fn resolve_frame(doc: &Document, table: &TableNode) -> TableFrameFormat {
    let mut resolved = inherited_frame_format(doc, table);
    let local = &table.frame_local;
    let mask = &table.frame_mask;

    if frame_mask_has(mask, FRAME_FILL) {
        resolved.fill = local.fill.clone();
    }
    for (side, bit) in FRAME_EDGES.iter().enumerate() {
        if frame_mask_has(mask, *bit) {
            resolved.edges[side] = local.edges[side].clone();
        }
    }
    if frame_mask_has(mask, FRAME_INSETS) {
        resolved.insets = local.insets.clone();
    }
    if frame_mask_has(mask, FRAME_IGNORE_BASELINE_GRID) {
        resolved.ignore_baseline_grid = local.ignore_baseline_grid;
    }
    if frame_mask_has(mask, FRAME_BASELINE) {
        resolved.baseline = local.baseline.clone();
    }
    resolved
}

pub fn text_insets(table: &TableNode, cell: &TableCell, cell_insets: Insets, frame: &TableFrameFormat) -> Insets {
    let mut insets = cell_insets;
    if cell.col == 0 { insets.left += frame.insets.left; }
    if cell.row == 0 { insets.top += frame.insets.top; }
    if cell.col.saturating_add(cell.col_span) == table_cols(table) { insets.right += frame.insets.right; }
    if cell.row.saturating_add(cell.row_span) == table_rows(table) { insets.bottom += frame.insets.bottom; }
    insets
}
