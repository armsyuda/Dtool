use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathTextData};
use dtool_core::path_ops::{apply_plan, plan_affine, plan_join, plan_split, undo_plan, PlanAction};

fn owned_document() -> Document {
    let mut doc = Document::new([200.0, 100.0], Units::Pt, 72.0, 1);
    for (source, text, story) in [(1, 2, 7), (3, 4, 8)] {
        doc.objects.push(Object { id: ObjectId(source), name: format!("path-{source}"), size: [56.0, 16.0], ..Object::default() });
        doc.objects.push(Object { id: ObjectId(text), name: format!("text-{text}"), kind: ObjectKind::PathText(PathTextData { path: ObjectId(source), story: StoryId(story), ..PathTextData::default() }), ..Object::default() });
        doc.stories.push(Story { id: StoryId(story), text: format!("story-{story}"), ..Story::default() });
    }
    doc.next_object_id = 5;
    doc
}

fn state(doc: &Document) -> (Vec<u64>, Vec<u64>) {
    let mut objects: Vec<u64> = doc.objects.iter().map(|object| object.id.0).collect();
    let mut stories: Vec<u64> = doc.stories.iter().map(|story| story.id.0).collect();
    objects.sort();
    stories.sort();
    (objects, stories)
}

fn no_dangling_text(doc: &Document) {
    for object in &doc.objects {
        if let ObjectKind::PathText(data) = &object.kind {
            assert!(doc.objects.iter().any(|source| source.id == data.path));
            assert!(doc.stories.iter().any(|story| story.id == data.story));
        }
    }
}

#[test]
fn split_plan_keeps_text_on_first_part_and_undo_restores() {
    let mut doc = owned_document();
    let before = state(&doc);
    let plan = plan_split(&doc, ObjectId(1), 0.5).unwrap();
    assert_eq!(state(&doc), before);
    assert!(plan.plans.iter().any(|entry| entry.story_id == StoryId(7) && entry.action == PlanAction::KeepOnFirstPart));
    assert!(apply_plan(&mut doc, &plan).is_ok());
    no_dangling_text(&doc);
    assert_eq!(state(&doc), before);
    assert!(undo_plan(&mut doc, &plan).is_ok());
    assert_eq!(state(&doc), before);
}

#[test]
fn split_plan_rejects_out_of_range_point_and_unknown_path() {
    let doc = owned_document();
    assert_eq!(plan_split(&doc, ObjectId(1), 1.5).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(plan_split(&doc, ObjectId(1), f32::NAN).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(plan_split(&doc, ObjectId(99), 0.5).unwrap_err().code, "NOT_FOUND");
}

#[test]
fn join_plan_ends_other_text_and_undo_restores_exact_ownership() {
    let mut doc = owned_document();
    let before = state(&doc);
    let plan = plan_join(&doc, &[ObjectId(1), ObjectId(3)]).unwrap();
    assert_eq!(state(&doc), before);
    assert!(plan.plans.iter().any(|entry| entry.story_id == StoryId(7) && entry.action == PlanAction::KeepOnSurvivingPath));
    assert!(plan.plans.iter().any(|entry| entry.story_id == StoryId(8) && entry.action == PlanAction::EndText));
    assert!(apply_plan(&mut doc, &plan).is_ok());
    no_dangling_text(&doc);
    assert!(!doc.objects.iter().any(|object| object.id == ObjectId(4)));
    assert!(!doc.stories.iter().any(|story| story.id == StoryId(8)));
    assert!(doc.objects.iter().any(|object| object.id == ObjectId(3)));
    assert!(undo_plan(&mut doc, &plan).is_ok());
    assert_eq!(state(&doc), before);
    assert_eq!(plan_join(&doc, &[]).unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn affine_plan_keeps_both_owners_and_does_not_mutate() {
    let doc = owned_document();
    let before = state(&doc);
    let plan = plan_affine(&doc, &[ObjectId(1), ObjectId(3)]).unwrap();
    assert_eq!(state(&doc), before);
    assert_eq!(plan.plans.len(), 2);
    assert!(plan.plans.iter().all(|entry| entry.action == PlanAction::KeepOnSurvivingPath));
    assert_eq!(plan.undo_data.original_objects.len(), 2);
    assert_eq!(plan.undo_data.original_stories.len(), 2);
}