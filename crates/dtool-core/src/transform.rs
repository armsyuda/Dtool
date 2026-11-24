use crate::document::Units;
use crate::object::Object;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Anchor {
    #[default]
    Centre,
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub rotation_deg: f32,
    pub shear_deg: f32,
    pub anchor: Anchor,
    pub aspect_locked: bool,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            pos: [0.0, 0.0],
            size: [100.0, 100.0],
            rotation_deg: 0.0,
            shear_deg: 0.0,
            anchor: Anchor::Centre,
            aspect_locked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlipAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RotateDirection {
    Left,
    Right,
}

fn anchor_offset(anchor: Anchor, size: [f32; 2]) -> [f32; 2] {
    let half_w = size[0].abs() * 0.5;
    let half_h = size[1].abs() * 0.5;
    match anchor {
        Anchor::TopLeft => [-half_w, -half_h],
        Anchor::Top => [0.0, -half_h],
        Anchor::TopRight => [half_w, -half_h],
        Anchor::Left => [-half_w, 0.0],
        Anchor::Centre => [0.0, 0.0],
        Anchor::Right => [half_w, 0.0],
        Anchor::BottomLeft => [-half_w, half_h],
        Anchor::Bottom => [0.0, half_h],
        Anchor::BottomRight => [half_w, half_h],
    }
}

pub fn apply_transform(obj: &mut Object, t: Transform, anchor: Anchor) {
    let old_size = obj.size;
    let old_pos = obj.pos;

    let old_anchor_offset = anchor_offset(anchor, old_size);
    let new_anchor_offset = anchor_offset(anchor, t.size);

    let pivot_x = old_pos[0] + old_anchor_offset[0];
    let pivot_y = old_pos[1] + old_anchor_offset[1];

    obj.pos = [pivot_x - new_anchor_offset[0], pivot_y - new_anchor_offset[1]];
    obj.size = t.size;
    obj.rotation_deg = t.rotation_deg;
    obj.shear_deg = t.shear_deg;
}

pub fn flip(obj: &mut Object, axis: FlipAxis, _anchor: Anchor) {
    match axis {
        FlipAxis::Horizontal => obj.size[0] = -obj.size[0],
        FlipAxis::Vertical => obj.size[1] = -obj.size[1],
    }
}

pub fn rotate_90(obj: &mut Object, direction: RotateDirection, _anchor: Anchor) {
    let turn = match direction {
        RotateDirection::Left => -90.0,
        RotateDirection::Right => 90.0,
    };
    obj.rotation_deg = crate::geometry::wrap_degrees(obj.rotation_deg + turn);
}

pub fn parse_field(
    input: &str,
    unit: Units,
    dpi: f32,
) -> Result<f32, dtool_protocol::CommandError> {
    crate::units::parse_length(input, unit, dpi).or_else(|error| suffixed_expression(input, dpi).ok_or(error))
}

fn suffixed_expression(input: &str, dpi: f32) -> Option<f32> {
    let trimmed = input.trim_end();
    let split = trimmed.char_indices().rev().take_while(|(_, c)| c.is_alphabetic() || *c == '"').last().map(|(i, _)| i)?;
    let (body, suffix) = trimmed.split_at(split);
    let token = crate::units::parse_unit(suffix)?;
    let value = crate::units::parse_number(body).ok()?;
    if body.trim_end().ends_with('%') || body.trim_end().ends_with('°') { return None; }
    crate::units::token_to_px(value, token, dpi).filter(|v| v.is_finite())
}

pub fn scrub_delta(pixels: f32, shift: bool) -> f32 {
    if shift {
        pixels * 0.1
    } else {
        pixels
    }
}
