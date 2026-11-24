use dtool_core::document::Document;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::table::geometry::{table_grid, TableGrid};
use dtool_core::table::ids::CellId;
use dtool_core::table::model::{CellFormat, TableCell, TableNode};
use dtool_core::table::resolve::resolve_cell;

#[derive(Clone, Debug)]
pub enum TablePreflightKind {
    MissingFont,
    Overflow,
    FontApproximate,
}

#[derive(Clone, Debug)]
pub struct TablePreflightItem {
    pub table: ObjectId,
    pub cell: Option<CellId>,
    pub kind: TablePreflightKind,
}

pub fn table_preflight(doc: &Document) -> Vec<TablePreflightItem> {
    let mut items = Vec::new();

    for object in &doc.objects {
        let ObjectKind::Table(table) = &object.kind else {
            continue;
        };
        let grid = table_grid(table);

        check_fonts(doc, object.id, table, &mut items);
        check_overflow(doc, object.id, table, &grid, &mut items);
    }

    items
}

fn cell_id(cell: &TableCell) -> CellId {
    CellId(cell.id.0)
}

fn style_name_is_blank(style_name: &Option<String>) -> bool {
    match style_name {
        Some(name) => name.trim().is_empty(),
        None => false,
    }
}

fn check_fonts(
    doc: &Document,
    table_id: ObjectId,
    table: &TableNode,
    items: &mut Vec<TablePreflightItem>,
) {
    for cell in &table.cells {
        if cell.merged_into.is_some() {
            continue;
        }

        let cell_format = resolve_cell(doc, table, cell);

        if style_name_is_blank(&cell_format.paragraph_style) {
            items.push(TablePreflightItem {
                table: table_id,
                cell: Some(cell_id(cell)),
                kind: TablePreflightKind::MissingFont,
            });
        }
    }
}

fn cell_text<'a>(doc: &'a Document, cell: &TableCell) -> Option<&'a str> {
    doc.stories
        .iter()
        .find(|story| story.id == cell.story)
        .or_else(|| doc.stories.get(cell.story.0 as usize))
        .map(|story| story.text.as_str())
}

fn cell_has_text(doc: &Document, cell: &TableCell) -> bool {
    cell_text(doc, cell)
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false)
}

fn content_box(rect: [f32; 4], format: &CellFormat) -> [f32; 2] {
    let width = rect[2] - rect[0] - format.insets.left - format.insets.right;
    let height = rect[3] - rect[1] - format.insets.top - format.insets.bottom;
    [width, height]
}

fn check_overflow(
    doc: &Document,
    table_id: ObjectId,
    table: &TableNode,
    grid: &TableGrid,
    items: &mut Vec<TablePreflightItem>,
) {
    for cell in &table.cells {
        if cell.merged_into.is_some() {
            continue;
        }
        if !cell_has_text(doc, cell) {
            continue;
        }
        let Some(cell_rect) = grid.rects.iter().find(|rect| rect.id == cell.id.0) else {
            continue;
        };
        let format = resolve_cell(doc, table, cell);
        let [width, height] = content_box(cell_rect.rect, &format);
        let finite = width.is_finite() && height.is_finite();
        let sideways = matches!(format.rotation, dtool_core::table::model::CellRotation::R90 | dtool_core::table::model::CellRotation::R270);
        let block = if sideways { width } else { height };
        let minimum = format.minimum_pt * dtool_core::units::units_per_point(doc.units, doc.dpi);
        let too_small = !finite || !(width > 0.0) || !(height > 0.0) || block < minimum;
        if too_small {
            items.push(TablePreflightItem {
                table: table_id,
                cell: Some(cell_id(cell)),
                kind: TablePreflightKind::Overflow,
            });
        }
    }
}
