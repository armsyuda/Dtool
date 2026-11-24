use crate::object::ObjectId;

pub const NUDGE_STEP_PX: f32 = 1.0;
pub const NUDGE_SHIFT_MULTIPLIER: f32 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionNudgeArgs {
    pub dx: f32,
    pub dy: f32,
    pub ids: Option<Vec<ObjectId>>,
}
