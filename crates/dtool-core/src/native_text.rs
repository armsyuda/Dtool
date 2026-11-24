use crate::document::{Document, StoryId};
use crate::object::ObjectId;
use dtool_protocol::{CommandError, CommandResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathTextDto {
    pub path: ObjectId,
    pub story: StoryId,
    pub flows: Vec<PathFlowDto>,
    pub source_paint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathFlowDto {
    pub start_pct: f32,
    pub end_pct: f32,
    pub baseline_pct: f32,
    pub reversed: bool,
    pub flipped: bool,
}

pub fn validate_path_text_dto(doc: &Document, dto: &PathTextDto) -> CommandResult {
    if !doc.stories.iter().any(|s| s.id == dto.story) {
        return Err(CommandError::validation("STORY_NOT_FOUND"));
    }

    if !doc.objects.iter().any(|o| o.id == dto.path) {
        return Err(CommandError::validation("OBJECT_NOT_FOUND"));
    }

    if let Some(ref paint_name) = dto.source_paint {
        if !doc.paints.contains_key(paint_name) {
            return Err(CommandError::validation("PAINT_NOT_FOUND"));
        }
    }

    Ok(CommandResult::ok(()))
}
