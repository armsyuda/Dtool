use std::collections::{BTreeMap, HashMap};

use dtool_core::table::geometry::{CellRect, TableGrid};
use serde_json::Value;

pub const ERROR_KEY: &str = "error.table_view";

#[derive(Clone, Debug)]
pub struct TableView {
    pub generation: u64,
    pub document_version: u64,
    pub font_generation: Option<u64>,
    pub grid: TableGrid,
    pub rects: Vec<CellRect>,
    pub merged_into: Vec<(u64, u64)>,
    pub layouts: BTreeMap<u64, CellLayoutView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineView {
    pub range: [usize; 2],
    pub baseline: f32,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub glyphs: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellLayoutView {
    pub content_rect: [f32; 4],
    pub rotation_deg: f32,
    pub overflow: bool,
    pub lines: Vec<LineView>,
}

fn bad() -> String {
    ERROR_KEY.to_string()
}

fn uint(value: &Value, key: &str) -> Result<u64, String> {
    value.get(key).and_then(Value::as_u64).ok_or_else(bad)
}

fn sizes(query: &Value, key: &str, count: usize) -> Result<Vec<f32>, String> {
    let list = query.get(key).and_then(Value::as_array).ok_or_else(bad)?;
    if list.len() != count {
        return Err(bad());
    }
    list.iter()
        .map(|item| match item.as_f64() {
            Some(size) if size.is_finite() && size > 0.0 && (size as f32).is_finite() => Ok(size as f32),
            _ => Err(bad()),
        })
        .collect()
}

fn edges(sizes: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(sizes.len() + 1);
    let mut at = 0.0;
    out.push(at);
    for size in sizes {
        at += size;
        out.push(at);
    }
    out
}

fn cell_id(value: &Value) -> Option<u64> {
    value.as_str()?.strip_prefix('c')?.parse::<u64>().ok()
}

fn number(value: &Value, key: &str) -> Result<f32, String> {
    match value.get(key).and_then(Value::as_f64) {
        Some(n) if n.is_finite() && (n as f32).is_finite() => Ok(n as f32),
        _ => Err(bad()),
    }
}

fn extent(value: &Value, key: &str) -> Result<f32, String> {
    number(value, key).and_then(|n| if n >= 0.0 { Ok(n) } else { Err(bad()) })
}

fn count(value: &Value) -> Result<usize, String> {
    value.as_u64().filter(|n| *n <= usize::MAX as u64).map(|n| n as usize).ok_or_else(bad)
}

fn parse_line(line: &Value, after: usize) -> Result<LineView, String> {
    let range = line.get("range").and_then(Value::as_array).ok_or_else(bad)?;
    if range.len() != 2 {
        return Err(bad());
    }
    let (start, end) = (count(&range[0])?, count(&range[1])?);
    if start > end || start < after {
        return Err(bad());
    }
    Ok(LineView {
        range: [start, end],
        baseline: number(line, "baseline")?,
        width: extent(line, "width")?,
        ascent: extent(line, "ascent")?,
        descent: extent(line, "descent")?,
        glyphs: count(line.get("glyphs").ok_or_else(bad)?)?,
    })
}

fn parse_layout(layout: &Value, overflow: Option<bool>) -> Result<CellLayoutView, String> {
    let rect = layout.get("content_rect").and_then(Value::as_array).ok_or_else(bad)?;
    if rect.len() != 4 {
        return Err(bad());
    }
    let mut content_rect = [0.0f32; 4];
    for (slot, item) in content_rect.iter_mut().zip(rect) {
        *slot = match item.as_f64() {
            Some(n) if n.is_finite() && (n as f32).is_finite() => n as f32,
            _ => return Err(bad()),
        };
    }
    let rotation_deg = number(layout, "rotation_deg")?;
    if ![0.0, 90.0, 180.0, 270.0].contains(&rotation_deg) {
        return Err(bad());
    }
    let own = layout.get("overflow").and_then(Value::as_bool).ok_or_else(bad)?;
    if overflow != Some(own) {
        return Err(bad());
    }
    let mut lines = Vec::new();
    let mut after = 0;
    for line in layout.get("lines").and_then(Value::as_array).ok_or_else(bad)? {
        let parsed = parse_line(line, after)?;
        after = parsed.range[1];
        lines.push(parsed);
    }
    Ok(CellLayoutView { content_rect, rotation_deg, overflow: own, lines })
}

fn span(value: &Value, key: &str) -> Result<u32, String> {
    match value.get(key) {
        None => Ok(1),
        Some(item) => item.as_u64().filter(|span| *span >= 1 && *span <= u32::MAX as u64).map(|span| span as u32).ok_or_else(bad),
    }
}

impl TableView {
    pub fn from_query(query: &Value) -> Result<Self, String> {
        let rows = uint(query, "rows")?;
        let cols = uint(query, "cols")?;
        if rows == 0 || cols == 0 || rows > u32::MAX as u64 || cols > u32::MAX as u64 {
            return Err(bad());
        }
        let (rows, cols) = (rows as u32, cols as u32);
        let col_widths = sizes(query, "col_widths_pt", cols as usize)?;
        let row_heights = sizes(query, "row_heights_pt", rows as usize)?;
        let generation = uint(query, "generation")?;
        let document_version = uint(query, "document_version")?;
        let font_generation = match query.get("font_generation") {
            None => None,
            Some(value) => Some(value.as_u64().ok_or_else(bad)?),
        };
        let grid = TableGrid { col_x: edges(&col_widths), row_y: edges(&row_heights), rects: Vec::new() };
        let cells = query.get("cells").and_then(Value::as_array).ok_or_else(bad)?;
        let mut occupied: Vec<Option<u64>> = vec![None; rows as usize * cols as usize];
        let mut rects: Vec<CellRect> = Vec::new();
        let mut covered: Vec<(u64, u32, u32, u64)> = Vec::new();
        let mut seen: HashMap<u64, ()> = HashMap::new();
        let mut layouts = BTreeMap::new();
        for cell in cells {
            let id = cell.get("id").and_then(cell_id).ok_or_else(bad)?;
            if seen.insert(id, ()).is_some() {
                return Err(bad());
            }
            let layout = match cell.get("layout") {
                None | Some(Value::Null) => None,
                Some(layout) => Some(parse_layout(layout, cell.get("overflow").and_then(Value::as_bool))?),
            };
            let row = uint(cell, "row")?;
            let col = uint(cell, "col")?;
            if row >= rows as u64 || col >= cols as u64 {
                return Err(bad());
            }
            let (row, col) = (row as u32, col as u32);
            match cell.get("merged_into") {
                None | Some(Value::Null) => {}
                Some(owner) => {
                    let owner = cell_id(owner).ok_or_else(bad)?;
                    if owner == id || layout.is_some() {
                        return Err(bad());
                    }
                    covered.push((id, row, col, owner));
                    continue;
                }
            }
            let row_span = span(cell, "row_span")?;
            let col_span = span(cell, "col_span")?;
            if row as u64 + row_span as u64 > rows as u64 || col as u64 + col_span as u64 > cols as u64 {
                return Err(bad());
            }
            for r in row..row + row_span {
                for c in col..col + col_span {
                    let slot = &mut occupied[(r * cols + c) as usize];
                    if slot.is_some() {
                        return Err(bad());
                    }
                    *slot = Some(id);
                }
            }
            let rect = [grid.col_x[col as usize], grid.row_y[row as usize], grid.col_x[(col + col_span) as usize], grid.row_y[(row + row_span) as usize]];
            rects.push(CellRect { id, row, col, row_span, col_span, rect });
            if let Some(layout) = layout {
                layouts.insert(id, layout);
            }
        }
        if !layouts.is_empty() && font_generation.is_none() {
            return Err(bad());
        }
        let mut merged_into = Vec::with_capacity(covered.len());
        for (id, row, col, owner) in covered {
            if occupied[(row * cols + col) as usize] != Some(owner) {
                return Err(bad());
            }
            merged_into.push((id, owner));
        }
        Ok(TableView { generation, document_version, font_generation, grid, rects, merged_into, layouts })
    }

    pub fn owner_of(&self, id: u64) -> u64 {
        self.merged_into.iter().find(|(cell, _)| *cell == id).map(|(_, owner)| *owner).unwrap_or(id)
    }

    pub fn layout_of(&self, id: u64) -> Option<&CellLayoutView> {
        self.layouts.get(&self.owner_of(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn query() -> Value {
        json!({
            "rows": 2,
            "cols": 2,
            "col_widths_pt": [40.0, 60.0],
            "row_heights_pt": [10.0, 20.0],
            "generation": 3,
            "document_version": 4,
            "cells": [
                { "id": "c1", "row": 0, "col": 0, "row_span": 1, "col_span": 2, "merged_into": null },
                { "id": "c2", "row": 0, "col": 1, "row_span": 1, "col_span": 1, "merged_into": "c1" },
                { "id": "c3", "row": 1, "col": 0, "row_span": 1, "col_span": 1, "merged_into": null },
                { "id": "c4", "row": 1, "col": 1, "row_span": 1, "col_span": 1, "merged_into": null }
            ]
        })
    }

    #[test]
    fn table_view_parses_merged_cells() {
        let view = TableView::from_query(&query()).expect("view");
        assert_eq!((view.generation, view.document_version, view.font_generation), (3, 4, None));
        assert_eq!(view.rects.len(), 3);
        assert_eq!(view.rects[0].rect, [0.0, 0.0, 100.0, 10.0]);
        assert_eq!(view.owner_of(2), 1);
        assert_eq!(view.owner_of(4), 4);
        let mut q = query();
        q["font_generation"] = json!(9);
        assert_eq!(TableView::from_query(&q).expect("view").font_generation, Some(9));
        q["font_generation"] = json!("x");
        assert!(TableView::from_query(&q).is_err());
    }

    fn with_lines() -> Value {
        let mut q = query();
        q["font_generation"] = json!(5);
        q["cells"][0]["overflow"] = json!(false);
        q["cells"][0]["layout"] = json!({
            "content_rect": [2.0, 2.0, 98.0, 8.0],
            "rotation_deg": 0.0,
            "overflow": false,
            "lines": [
                { "range": [0, 4], "baseline": 6.0, "width": 20.0, "ascent": 4.0, "descent": 1.0, "glyphs": 4 },
                { "range": [5, 9], "baseline": 12.0, "width": 18.0, "ascent": 4.0, "descent": 1.0, "glyphs": 4 }
            ]
        });
        q["cells"][1]["layout"] = Value::Null;
        q
    }

    #[test]
    fn table_view_parses_layout_lines() {
        let view = TableView::from_query(&with_lines()).expect("view");
        assert_eq!(view.font_generation, Some(5));
        let layout = view.layout_of(2).expect("covered cell follows owner");
        assert_eq!(layout, view.layouts.get(&1).unwrap());
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[1].range, [5, 9]);
        assert_eq!(layout.content_rect, [2.0, 2.0, 98.0, 8.0]);
        assert!(view.layout_of(3).is_none());
    }

    #[test]
    fn table_view_rejects_bad_layout_lines() {
        let cases: Vec<(&str, fn(&mut Value))> = vec![
            ("no font_generation", |q| { q.as_object_mut().unwrap().remove("font_generation"); }),
            ("covered layout", |q| { q["cells"][1]["layout"] = q["cells"][0]["layout"].clone(); }),
            ("overflow mismatch", |q| { q["cells"][0]["overflow"] = json!(true); }),
            ("cell overflow missing", |q| { q["cells"][0].as_object_mut().unwrap().remove("overflow"); }),
            ("short rect", |q| { q["cells"][0]["layout"]["content_rect"] = json!([0.0, 0.0, 1.0]); }),
            ("rect string", |q| { q["cells"][0]["layout"]["content_rect"][2] = json!("x"); }),
            ("rotation", |q| { q["cells"][0]["layout"]["rotation_deg"] = json!(45.0); }),
            ("negative width", |q| { q["cells"][0]["layout"]["lines"][0]["width"] = json!(-1.0); }),
            ("huge baseline", |q| { q["cells"][0]["layout"]["lines"][0]["baseline"] = json!(1e300); }),
            ("range reversed", |q| { q["cells"][0]["layout"]["lines"][0]["range"] = json!([4, 0]); }),
            ("range overlap", |q| { q["cells"][0]["layout"]["lines"][1]["range"] = json!([3, 9]); }),
            ("glyphs missing", |q| { q["cells"][0]["layout"]["lines"][0].as_object_mut().unwrap().remove("glyphs"); }),
            ("lines missing", |q| { q["cells"][0]["layout"].as_object_mut().unwrap().remove("lines"); }),
            ("layout not object", |q| { q["cells"][0]["layout"] = json!(3); }),
        ];
        for (name, change) in cases {
            let mut q = with_lines();
            change(&mut q);
            assert_eq!(TableView::from_query(&q).unwrap_err(), ERROR_KEY, "{name}");
        }
    }

    #[test]
    fn table_view_rejects_bad_span() {
        let mut q = query();
        q["cells"][0]["col_span"] = json!(3);
        assert_eq!(TableView::from_query(&q).unwrap_err(), ERROR_KEY);
        let mut q = query();
        q["cells"][0]["col_span"] = json!(0);
        assert!(TableView::from_query(&q).is_err());
        let mut q = query();
        q["cells"][2]["col_span"] = json!(2);
        assert!(TableView::from_query(&q).is_err());
        let mut q = query();
        q["cells"][1]["merged_into"] = json!("c3");
        assert!(TableView::from_query(&q).is_err());
        let mut q = query();
        q["cells"][3]["id"] = json!("c3");
        assert!(TableView::from_query(&q).is_err());
    }

    #[test]
    fn table_view_rejects_missing_version() {
        for key in ["generation", "document_version"] {
            let mut q = query();
            q.as_object_mut().unwrap().remove(key);
            assert_eq!(TableView::from_query(&q).unwrap_err(), ERROR_KEY, "{key}");
        }
    }

    #[test]
    fn table_view_rejects_nonfinite_width() {
        for bad in [json!(0.0), json!(-1.0), json!(1e300), json!("w"), json!(null)] {
            let mut q = query();
            q["col_widths_pt"][1] = bad.clone();
            assert!(TableView::from_query(&q).is_err(), "{bad}");
            let mut q = query();
            q["row_heights_pt"][0] = bad.clone();
            assert!(TableView::from_query(&q).is_err(), "{bad}");
        }
    }
}