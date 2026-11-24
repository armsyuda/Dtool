use crate::table::model::TableNode;
use crate::{Document, ObjectId};
use std::sync::OnceLock;

pub struct CellExtent {
    pub start: usize,
    pub span: usize,
    pub size: f32,
}

type Measure = fn(&Document, &TableNode, bool, &[bool]) -> Vec<CellExtent>;
static MEASURE: OnceLock<Measure> = OnceLock::new();

pub fn register_measure(provider: Measure) -> bool {
    MEASURE.set(provider).is_ok() || MEASURE.get().is_some_and(|known| std::ptr::fn_addr_eq(*known, provider))
}

pub(crate) fn available() -> bool { MEASURE.get().is_some() }

pub(crate) fn fit(doc: &Document, id: ObjectId, rows: bool, tracks: &[f32], indices: &[u32]) -> Option<Vec<f32>> {
    let crate::ObjectKind::Table(table) = &doc.get_object(id)?.kind else { return None; };
    let mut selected = vec![false; tracks.len()];
    for &index in indices {
        *selected.get_mut(index as usize)? = true;
    }
    let minimum = crate::units::units_per_point(doc.units, doc.dpi);
    let mut sizes: Vec<_> = tracks.iter().zip(&selected).map(|(&size, &selected)| {
        if selected { minimum } else { size }
    }).collect();
    let mut extents = MEASURE.get()?(doc, table, rows, &selected);
    extents.sort_by_key(|extent| extent.span);
    for extent in extents {
        if !extent.size.is_finite() { return None; }
        let end = extent.start.checked_add(extent.span)?;
        let selection = selected.get(extent.start..end)?;
        let sizes = sizes.get_mut(extent.start..end)?;
        let current: f64 = sizes.iter().map(|&size| f64::from(size)).sum();
        let remaining = f64::from(extent.size) - current;
        let count = selection.iter().filter(|&&selected| selected).count();
        if remaining <= 0.0 || count == 0 { continue; }
        // A merged cell's other tracks contribute their existing or already fitted sizes.
        let extra = remaining / count as f64;
        for (size, &selected) in sizes.iter_mut().zip(selection) {
            if selected {
                let target = f64::from(*size) + extra;
                let rounded = target as f32;
                *size = if f64::from(rounded) < target { rounded.next_up() } else { rounded };
            }
        }
    }
    Some(sizes)
}
