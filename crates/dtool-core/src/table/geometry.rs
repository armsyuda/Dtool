#![allow(dead_code)]

use crate::table::model::{TableCell, TableNode};

#[derive(Clone, Debug)]
pub struct CellRect {
    pub id: u64,
    pub row: u32,
    pub col: u32,
    pub row_span: u32,
    pub col_span: u32,
    pub rect: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct TableGrid {
    pub col_x: Vec<f32>,
    pub row_y: Vec<f32>,
    pub rects: Vec<CellRect>,
}

pub fn table_grid(table: &TableNode) -> TableGrid {
    let mut col_x = vec![0.0];
    for w in &table.col_widths_pt {
        col_x.push(col_x.last().unwrap() + w);
    }

    let mut row_y = vec![0.0];
    for h in &table.row_heights_pt {
        row_y.push(row_y.last().unwrap() + h);
    }

    let mut rects = Vec::new();
    for cell in &table.cells {
        if cell.merged_into.is_some() {
            continue;
        }

        let x0 = col_x[cell.col as usize];
        let x1 = col_x[(cell.col + cell.col_span) as usize];
        let y0 = row_y[cell.row as usize];
        let y1 = row_y[(cell.row + cell.row_span) as usize];

        rects.push(CellRect {
            id: cell.id.0,
            row: cell.row,
            col: cell.col,
            row_span: cell.row_span,
            col_span: cell.col_span,
            rect: [x0, y0, x1, y1],
        });
    }

    TableGrid { col_x, row_y, rects }
}

pub fn cell_at(grid: &TableGrid, pt: [f32; 2]) -> Option<u64> {
    for rect in &grid.rects {
        if pt[0] >= rect.rect[0] && pt[0] < rect.rect[2] && pt[1] >= rect.rect[1] && pt[1] < rect.rect[3] {
            return Some(rect.id);
        }
    }
    None
}

pub fn to_local(matrix: [f32; 6], pt: [f32; 2]) -> Option<[f32; 2]> {
    let column_major = [matrix[0], matrix[2], matrix[1], matrix[3], matrix[4], matrix[5]];
    crate::geometry::invert_matrix(&column_major).map(|inverse| crate::geometry::apply_matrix(&inverse, pt))
}

pub fn edge_owner(grid: &TableGrid, row: u32, col: u32, side: EdgeSide) -> Option<u64> {
    let anchor = grid.rects.iter().find(|r| {
        r.row <= row && row < r.row + r.row_span && r.col <= col && col < r.col + r.col_span
    });

    match side {
        EdgeSide::Right => {
            let next_col = col + 1;
            grid.rects.iter().find(|r| {
                r.row <= row && row < r.row + r.row_span && r.col <= next_col && next_col < r.col + r.col_span
            }).map(|r| r.id)
        }
        EdgeSide::Bottom => {
            let next_row = row + 1;
            grid.rects.iter().find(|r| {
                r.row <= next_row && next_row < r.row + r.row_span && r.col <= col && col < r.col + r.col_span
            }).map(|r| r.id)
        }
        EdgeSide::Left => anchor.map(|r| r.id),
        EdgeSide::Top => anchor.map(|r| r.id),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EdgeSide {
    Left,
    Right,
    Top,
    Bottom,
}
