use crate::document::{Document, StoryId};
use crate::object::{Object, ObjectId, ObjectKind};
use dtool_protocol::{CommandError, CommandResult};

#[derive(Clone, Debug)]
pub struct PathTextPlan {
    pub path_id: ObjectId,
    pub story_id: StoryId,
    pub action: PlanAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlanAction {
    KeepOnFirstPart,
    EndText,
    KeepOnSurvivingPath,
}

#[derive(Clone, Debug)]
pub struct TransactionPlan {
    pub plans: Vec<PathTextPlan>,
    pub undo_data: UndoData,
}

#[derive(Clone, Debug)]
pub struct UndoData {
    pub original_objects: Vec<Object>,
    pub original_stories: Vec<crate::document::Story>,
}

fn find_path_text(doc: &Document, path_id: ObjectId) -> Option<(Object, StoryId)> {
    for obj in &doc.objects {
        if let ObjectKind::PathText(data) = &obj.kind {
            if obj.id == path_id || data.path == path_id {
                return Some((obj.clone(), data.story));
            }
        }
    }
    None
}

fn owns_text(obj: &Object, path_id: ObjectId) -> bool {
    matches!(&obj.kind, ObjectKind::PathText(data) if obj.id == path_id || data.path == path_id)
}

fn collect_path_text_objects(doc: &Document, ids: &[ObjectId]) -> Vec<(Object, StoryId)> {
    let mut result = Vec::new();
    for id in ids {
        if let Some((obj, story_id)) = find_path_text(doc, *id) {
            result.push((obj, story_id));
        }
    }
    result
}

pub fn plan_split(doc: &Document, path_id: ObjectId, split_point: f32) -> Result<TransactionPlan, CommandError> {
    if !split_point.is_finite() || !(0.0..=1.0).contains(&split_point) {
        return Err(CommandError::invalid_params());
    }
    let (path_obj, story_id) = find_path_text(doc, path_id)
        .ok_or_else(|| CommandError::not_found())?;

    let mut plans = Vec::new();
    plans.push(PathTextPlan {
        path_id,
        story_id,
        action: PlanAction::KeepOnFirstPart,
    });

    let undo_data = UndoData {
        original_objects: vec![path_obj],
        original_stories: doc.stories.iter().filter(|s| s.id == story_id).cloned().collect(),
    };

    Ok(TransactionPlan { plans, undo_data })
}

pub fn plan_join(doc: &Document, path_ids: &[ObjectId]) -> Result<TransactionPlan, CommandError> {
    if path_ids.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let mut plans = Vec::new();
    let surviving_id = path_ids[0];

    for &id in path_ids {
        if let Some((_, story_id)) = find_path_text(doc, id) {
            if id == surviving_id {
                plans.push(PathTextPlan {
                    path_id: id,
                    story_id,
                    action: PlanAction::KeepOnSurvivingPath,
                });
            } else {
                plans.push(PathTextPlan {
                    path_id: id,
                    story_id,
                    action: PlanAction::EndText,
                });
            }
        }
    }

    let original_objects = collect_path_text_objects(doc, path_ids).into_iter().map(|(o, _)| o).collect();
    let story_ids: Vec<StoryId> = plans.iter().map(|p| p.story_id).collect();
    let original_stories = doc.stories.iter().filter(|s| story_ids.contains(&s.id)).cloned().collect();

    Ok(TransactionPlan {
        plans,
        undo_data: UndoData {
            original_objects,
            original_stories,
        },
    })
}

pub fn plan_compound(doc: &Document, path_ids: &[ObjectId]) -> Result<TransactionPlan, CommandError> {
    if path_ids.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let mut plans = Vec::new();
    let surviving_id = path_ids[0];

    for &id in path_ids {
        if let Some((_, story_id)) = find_path_text(doc, id) {
            if id == surviving_id {
                plans.push(PathTextPlan {
                    path_id: id,
                    story_id,
                    action: PlanAction::KeepOnSurvivingPath,
                });
            } else {
                plans.push(PathTextPlan {
                    path_id: id,
                    story_id,
                    action: PlanAction::EndText,
                });
            }
        }
    }

    let original_objects = collect_path_text_objects(doc, path_ids).into_iter().map(|(o, _)| o).collect();
    let story_ids: Vec<StoryId> = plans.iter().map(|p| p.story_id).collect();
    let original_stories = doc.stories.iter().filter(|s| story_ids.contains(&s.id)).cloned().collect();

    Ok(TransactionPlan {
        plans,
        undo_data: UndoData {
            original_objects,
            original_stories,
        },
    })
}

pub fn plan_contour(doc: &Document, path_id: ObjectId) -> Result<TransactionPlan, CommandError> {
    let (path_obj, story_id) = find_path_text(doc, path_id)
        .ok_or_else(|| CommandError::not_found())?;

    let plans = vec![PathTextPlan {
        path_id,
        story_id,
        action: PlanAction::KeepOnSurvivingPath,
    }];

    Ok(TransactionPlan {
        plans,
        undo_data: UndoData {
            original_objects: vec![path_obj],
            original_stories: doc.stories.iter().filter(|s| s.id == story_id).cloned().collect(),
        },
    })
}

pub fn plan_affine(doc: &Document, path_ids: &[ObjectId]) -> Result<TransactionPlan, CommandError> {
    let mut plans = Vec::new();
    for &id in path_ids {
        if let Some((_, story_id)) = find_path_text(doc, id) {
            plans.push(PathTextPlan {
                path_id: id,
                story_id,
                action: PlanAction::KeepOnSurvivingPath,
            });
        }
    }

    let original_objects = collect_path_text_objects(doc, path_ids).into_iter().map(|(o, _)| o).collect();
    let story_ids: Vec<StoryId> = plans.iter().map(|p| p.story_id).collect();
    let original_stories = doc.stories.iter().filter(|s| story_ids.contains(&s.id)).cloned().collect();

    Ok(TransactionPlan {
        plans,
        undo_data: UndoData {
            original_objects,
            original_stories,
        },
    })
}

pub fn apply_plan(doc: &mut Document, plan: &TransactionPlan) -> CommandResult {
    for p in &plan.plans {
        match p.action {
            PlanAction::KeepOnFirstPart | PlanAction::KeepOnSurvivingPath => {}
            PlanAction::EndText => {
                doc.objects.retain(|o| !owns_text(o, p.path_id));
                crate::frame::drop_story_if_unowned(doc, p.story_id);
            }
        }
    }
    Ok(serde_json::json!({"ok": true}))
}

pub fn undo_plan(doc: &mut Document, plan: &TransactionPlan) -> CommandResult {
    for obj in &plan.undo_data.original_objects {
        if !doc.objects.iter().any(|o| o.id == obj.id) {
            doc.objects.push(obj.clone());
        }
    }
    for story in &plan.undo_data.original_stories {
        if !doc.stories.iter().any(|s| s.id == story.id) {
            doc.stories.push(story.clone());
        }
    }
    Ok(serde_json::json!({"ok": true}))
}
