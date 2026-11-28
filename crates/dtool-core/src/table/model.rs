use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct TableFormatId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableFormatDef {
    pub id: TableFormatId,
    pub name: String,
    pub frame: TableFrameFormat,
    pub cell_formats: Vec<(u64, String, CellFormat)>,
    pub pattern: TablePattern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableFrameFormat {
    #[serde(default, deserialize_with = "crate::paint::legacy_fill::deserialize")]
    pub fill: Option<crate::paint::Paint>,
    pub edges: [Option<EdgeStyle>; 4],
    pub insets: Insets,
    pub ignore_baseline_grid: bool,
    pub baseline: Option<BaselineGrid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellFormat {
    #[serde(default, deserialize_with = "crate::paint::legacy_fill::deserialize")]
    pub fill: Option<crate::paint::Paint>,
    pub edges: [Option<EdgeStyle>; 4],
    pub insets: Insets,
    pub vertical_align: VerticalAlign,
    pub rotation: CellRotation,
    pub initial_advance: InitialAdvance,
    pub minimum_pt: f32,
    pub paragraph_style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TablePattern {
    pub rows: u16,
    pub cols: u16,
    pub top_band_rows: u16,
    pub bottom_band_rows: u16,
    pub left_band_cols: u16,
    pub right_band_cols: u16,
    pub slots: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeStyle {
    #[serde(default, deserialize_with = "crate::paint::legacy_paint::deserialize")]
    pub paint: crate::paint::Paint,
    pub width_pt: f32,
    #[serde(default, skip_serializing_if = "crate::paint::LineStyle::is_solid")]
    pub line_style: crate::paint::LineStyle,
    #[serde(default, skip_serializing_if = "edge_is_centred")]
    pub align: crate::paint::StrokeAlign,
}

fn edge_is_centred(align: &crate::paint::StrokeAlign) -> bool {
    *align == crate::paint::StrokeAlign::Centre
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub linked: bool,
}

impl Insets {
    /// Resize one inset, preserving the ratios of linked insets when possible.
    pub fn with_side(self, index: usize, value: f32) -> Self {
        let mut sides = [self.left, self.top, self.right, self.bottom];
        if self.linked {
            let previous = sides[index];
            if previous > 0.0 {
                let ratio = f64::from(value) / f64::from(previous);
                sides = sides.map(|side| (f64::from(side) * ratio).min(f64::from(f32::MAX)) as f32);
            } else {
                // A zero inset has no ratio. Grow all sides by the same amount.
                sides = sides.map(|side| (f64::from(side) + f64::from(value)).min(f64::from(f32::MAX)) as f32);
            }
        }
        sides[index] = value;
        let [left, top, right, bottom] = sides;
        Self { left, top, right, bottom, ..self }
    }
}

impl Default for Insets {
    fn default() -> Self {
        Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            linked: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineGrid {
    pub enabled: bool,
    pub start_pt: f32,
    pub relative_to: BaselineOrigin,
    pub spacing_pt: f32,
    pub color: String,
}

impl Default for BaselineGrid {
    fn default() -> Self {
        Self { enabled: false, start_pt: 0.0, relative_to: BaselineOrigin::Frame, spacing_pt: 12.0, color: "#00c0c080".into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BaselineOrigin {
    #[default]
    Frame,
    Page,
    Spread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VerticalAlign {
    #[default]
    Top,
    Center,
    Bottom,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CellRotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum InitialAdvance {
    #[default]
    Default,
    Ascent,
    CapHeight,
    XHeight,
    Leading,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct StoryId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCell {
    pub id: CellId,
    pub row: u32,
    pub col: u32,
    pub row_span: u32,
    pub col_span: u32,
    pub merged_into: Option<CellId>,
    pub story: StoryId,
    #[serde(default)]
    pub local: CellFormat,
    #[serde(default)]
    pub mask: CellFormatMask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "TableNodeWire")]
pub struct TableNode {
    pub alloc: CellIdAllocator,
    pub col_widths_pt: Vec<f32>,
    pub row_heights_pt: Vec<f32>,
    pub cells: Vec<TableCell>,
    pub header_rows: u16,
    pub footer_rows: u16,
    pub format: Option<TableFormatId>,
    pub frame_local: TableFrameFormat,
    pub frame_mask: FrameFormatMask,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_defaults: Option<crate::defaults::TextDefaults>,
}

#[derive(Deserialize)]
struct TableNodeWire {
    #[serde(default)]
    alloc: CellIdAllocator,
    col_widths_pt: Vec<f32>,
    row_heights_pt: Vec<f32>,
    cells: Vec<TableCell>,
    #[serde(default)]
    header_rows: u16,
    #[serde(default)]
    footer_rows: u16,
    #[serde(default)]
    format: Option<TableFormatId>,
    #[serde(default)]
    frame_local: TableFrameFormat,
    #[serde(default)]
    frame_mask: FrameFormatMask,
    #[serde(default)]
    text_defaults: Option<crate::defaults::TextDefaults>,
}

impl TryFrom<TableNodeWire> for TableNode {
    type Error = String;

    fn try_from(wire: TableNodeWire) -> Result<Self, Self::Error> {
        let ids: Vec<CellId> = wire.cells.iter().map(|cell| cell.id).collect();
        let alloc = CellIdAllocator::hydrate_with_saved(&ids, wire.alloc.high_water)
            .map_err(|err| err.code.clone())?;
        Ok(Self {
            alloc,
            col_widths_pt: wire.col_widths_pt,
            row_heights_pt: wire.row_heights_pt,
            cells: wire.cells,
            header_rows: wire.header_rows,
            footer_rows: wire.footer_rows,
            format: wire.format,
            frame_local: wire.frame_local,
            frame_mask: wire.frame_mask,
            text_defaults: wire.text_defaults,
        })
    }
}

impl TableNode {
    pub fn hydrate_cell_ids(&mut self) -> Result<(), dtool_protocol::CommandError> {
        let ids: Vec<CellId> = self.cells.iter().map(|cell| cell.id).collect();
        self.alloc = CellIdAllocator::hydrate_with_saved(&ids, self.alloc.high_water)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CellId(pub u64);

impl CellId {
    pub fn text(&self) -> String {
        format!("c{}", self.0)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellIdAllocator {
    #[serde(default)]
    pub high_water: u64,
}

impl CellIdAllocator {
    pub fn next(&mut self) -> CellId {
        match self.try_next() {
            Ok(id) => id,
            Err(_) => CellId(self.high_water),
        }
    }

    pub fn try_next(&mut self) -> Result<CellId, dtool_protocol::CommandError> {
        if self.high_water == u64::MAX {
            return Err(dtool_protocol::CommandError::out_of_range());
        }
        self.high_water += 1;
        Ok(CellId(self.high_water))
    }

    pub fn hydrate(ids: &[CellId]) -> Result<Self, dtool_protocol::CommandError> {
        Self::hydrate_with_saved(ids, 0)
    }

    pub fn hydrate_with_saved(ids: &[CellId], saved: u64) -> Result<Self, dtool_protocol::CommandError> {
        let mut max_id: u64 = 0;
        let mut seen = std::collections::HashSet::new();
        for cell in ids {
            if !seen.insert(cell.0) {
                return Err(dtool_protocol::CommandError::validation("TableIdConflict"));
            }
            if cell.0 > max_id {
                max_id = cell.0;
            }
        }
        Ok(CellIdAllocator { high_water: saved.max(max_id) })
    }

    pub fn parse_legacy(s: &str) -> Option<CellId> {
        let num_part = s.strip_prefix('c')?;
        num_part.parse::<u64>().ok().map(CellId)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CellFormatMask(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct FrameFormatMask(pub u32);


impl Default for CellFormat {
    fn default() -> Self {
        Self {
            fill: None,
            edges: [None, None, None, None],
            insets: Insets::default(),
            vertical_align: VerticalAlign::default(),
            rotation: CellRotation::default(),
            initial_advance: InitialAdvance::default(),
            minimum_pt: 0.0,
            paragraph_style: None,
        }
    }
}

impl Default for TableFrameFormat {
    fn default() -> Self {
        Self {
            fill: None,
            edges: [None, None, None, None],
            insets: Insets::default(),
            ignore_baseline_grid: false,
            baseline: None,
        }
    }
}

impl CellFormat {
    pub fn fill_str(&self) -> Option<String> {
        self.fill.as_ref().and_then(crate::paint::paint_to_legacy_str)
    }

    pub fn set_fill_str(&mut self, value: Option<&str>) {
        self.fill = value.and_then(crate::paint::paint_from_legacy_str);
    }
}

impl TableFrameFormat {
    pub fn fill_str(&self) -> Option<String> {
        self.fill.as_ref().and_then(crate::paint::paint_to_legacy_str)
    }

    pub fn set_fill_str(&mut self, value: Option<&str>) {
        self.fill = value.and_then(crate::paint::paint_from_legacy_str);
    }
}

impl EdgeStyle {
    pub fn paint_str(&self) -> String {
        crate::paint::paint_to_legacy_str(&self.paint).unwrap_or_default()
    }

    pub fn set_paint_str(&mut self, value: &str) {
        self.paint = crate::paint::paint_from_legacy_str(value).unwrap_or(crate::paint::Paint::None);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableEdgeMask(pub u8);

impl TableEdgeMask {
    pub const LEFT: TableEdgeMask = TableEdgeMask(1);
    pub const RIGHT: TableEdgeMask = TableEdgeMask(2);
    pub const INSIDE_V: TableEdgeMask = TableEdgeMask(4);
    pub const TOP: TableEdgeMask = TableEdgeMask(8);
    pub const BOTTOM: TableEdgeMask = TableEdgeMask(16);
    pub const INSIDE_H: TableEdgeMask = TableEdgeMask(32);
    pub const OUTSIDE: TableEdgeMask = TableEdgeMask(27);
    pub const INSIDE: TableEdgeMask = TableEdgeMask(36);
    pub const ALL: TableEdgeMask = TableEdgeMask(63);
    pub const OUTSIDE_V: TableEdgeMask = TableEdgeMask(3);
    pub const ALL_V: TableEdgeMask = TableEdgeMask(7);
    pub const OUTSIDE_H: TableEdgeMask = TableEdgeMask(24);
    pub const ALL_H: TableEdgeMask = TableEdgeMask(56);
}
