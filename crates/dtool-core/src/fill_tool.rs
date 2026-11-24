use crate::command::Session;
use crate::object::ObjectId;
use crate::context_toolbar::ToolOptionStore;
use crate::raster_view::RasterViewMut;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};

pub const FILL_TOOL_ID: &str = "tool.flood_fill";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillType {
    #[default]
    Solid,
    Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FillToolOptions {
    pub context_fill: bool,
    pub fill_type: FillType,
    pub colour: [u8; 4],
    pub pigment_blend: f32,
}

impl Default for FillToolOptions {
    fn default() -> Self {
        Self {
            context_fill: false,
            fill_type: FillType::Solid,
            colour: [0, 0, 0, 255],
            pigment_blend: 1.0,
        }
    }
}

impl FillToolOptions {
    pub fn from_store(store: &ToolOptionStore) -> Self {
        let mut out = Self::default();
        if let Some(v) = store.flag(FILL_TOOL_ID, "fill.context_fill") {
            out.context_fill = v;
        }
        if let Some(v) = store.choice(FILL_TOOL_ID, "fill.fill_type") {
            out.fill_type = if v == "context" { FillType::Context } else { FillType::Solid };
        }
        if let Some(v) = store.number(FILL_TOOL_ID, "fill.pigment_blend") {
            out.pigment_blend = v as f32;
        }
        out
    }

    pub fn write_store(&self, store: &mut ToolOptionStore) {
        store.set_flag(FILL_TOOL_ID, "fill.context_fill", self.context_fill);
        let kind = match self.fill_type {
            FillType::Solid => "solid",
            FillType::Context => "context",
        };
        store.set_choice(FILL_TOOL_ID, "fill.fill_type", kind);
        store.set_number(FILL_TOOL_ID, "fill.pigment_blend", f64::from(self.pigment_blend));
    }

    fn uses_context(&self) -> bool {
        self.context_fill || self.fill_type == FillType::Context
    }
}

pub fn execute_fill(
    session: &mut Session,
    node_id: u64,
    options: &FillToolOptions,
    rect: Option<[u32; 4]>,
) -> CommandResult {
    let obj = session.document.find_object_mut(node_id).ok_or_else(CommandError::not_found)?;
    let (width, height) = obj.raster_size.ok_or_else(CommandError::invalid_params)?;
    let data = obj.raster_data.as_mut().ok_or_else(CommandError::invalid_params)?;
    if data.len() != width as usize * height as usize * 4 {
        return Err(CommandError::invalid_params());
    }
    let bounds = clip_rect(rect.unwrap_or([0, 0, width, height]), width, height)
        .ok_or_else(|| CommandError::validation("NO_CHANGE"))?;
    let before = data.clone();
    {
        let mut raster = RasterViewMut::new(width, height, data.as_mut_slice());
        if options.uses_context() {
            apply_context_fill(&mut raster, bounds, options.colour, options.pigment_blend);
        } else {
            apply_solid_fill(&mut raster, bounds, options.colour);
        }
    }
    if *data == before {
        return Err(CommandError::validation("NO_CHANGE"));
    }
    let after = data.clone();
    drop(data);
    drop(obj);
    if let Some(layer) = session.document.pixel_layers.get_mut(ObjectId(node_id)) {
        layer.set_region([0, 0, width, height], &after);
    }
    let restore = crate::commands::fill::FILL_RESTORE;
    session.undo.push(crate::undo::Change {
        name: "fill.tool".to_string(),
        redo: Command::with_params(restore, serde_json::json!({ "node_id": node_id, "data": after })),
        undo: Command::with_params(restore, serde_json::json!({ "node_id": node_id, "data": before })),
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(serde_json::json!({
        "doc_version": session.generation,
        "node_id": node_id,
        "changed_bounds": bounds,
    }))
}

fn clip_rect(rect: [u32; 4], width: u32, height: u32) -> Option<[u32; 4]> {
    let x = rect[0].min(width);
    let y = rect[1].min(height);
    let w = rect[2].min(width - x);
    let h = rect[3].min(height - y);
    if w == 0 || h == 0 {
        None
    } else {
        Some([x, y, w, h])
    }
}

fn apply_solid_fill(raster: &mut RasterViewMut, rect: [u32; 4], colour: [u8; 4]) {
    let [x, y, w, h] = rect;
    for py in y..y + h {
        for px in x..x + w {
            raster.set_pixel(i64::from(px), i64::from(py), colour[0], colour[1], colour[2], colour[3]);
        }
    }
}

fn apply_context_fill(raster: &mut RasterViewMut, rect: [u32; 4], colour: [u8; 4], blend: f32) {
    let [x, y, w, h] = rect;
    let mut out = Vec::with_capacity(w as usize * h as usize);
    for py in y..y + h {
        for px in x..x + w {
            let around = sample_surrounding(raster, i64::from(px), i64::from(py));
            out.push(blend_colours(colour, around, blend));
        }
    }
    let mut index = 0;
    for py in y..y + h {
        for px in x..x + w {
            let c = out[index];
            raster.set_pixel(i64::from(px), i64::from(py), c[0], c[1], c[2], c[3]);
            index += 1;
        }
    }
}

fn sample_surrounding(raster: &RasterViewMut, x: i64, y: i64) -> [u8; 4] {
    let mut sum = [0u32; 4];
    let mut count = 0u32;
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= i64::from(raster.width) || ny >= i64::from(raster.height) {
                continue;
            }
            let c = raster.get_pixel(nx, ny);
            for i in 0..4 {
                sum[i] += u32::from(c[i]);
            }
            count += 1;
        }
    }
    if count == 0 {
        return [0, 0, 0, 0];
    }
    sum.map(|v| (v / count) as u8)
}

fn blend_colours(target: [u8; 4], source: [u8; 4], blend: f32) -> [u8; 4] {
    let blend = blend.clamp(0.0, 1.0);
    let mut out = [0u8; 4];
    for i in 0..4 {
        let v = f32::from(target[i]) * blend + f32::from(source[i]) * (1.0 - blend);
        out[i] = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip_through_store() {
        let mut store = ToolOptionStore::default();
        let options = FillToolOptions { context_fill: true, fill_type: FillType::Context, colour: [1, 2, 3, 255], pigment_blend: 0.25 };
        options.write_store(&mut store);
        let back = FillToolOptions::from_store(&store);
        assert!(back.context_fill);
        assert_eq!(back.fill_type, FillType::Context);
        assert!((back.pigment_blend - 0.25).abs() < 1e-6);
    }

    #[test]
    fn clip_rect_bounds() {
        assert_eq!(clip_rect([2, 2, 10, 10], 4, 4), Some([2, 2, 2, 2]));
        assert_eq!(clip_rect([5, 0, 1, 1], 4, 4), None);
    }

    #[test]
    fn solid_fill_writes_rect() {
        let mut data = vec![0u8; 4 * 4 * 4];
        let mut raster = RasterViewMut::new(4, 4, data.as_mut_slice());
        apply_solid_fill(&mut raster, [1, 1, 2, 2], [9, 8, 7, 255]);
        assert_eq!(raster.get_pixel(1, 1), [9, 8, 7, 255]);
        assert_eq!(raster.get_pixel(0, 0), [0, 0, 0, 0]);
    }
}
