use dtool_core::job::{JobId, JobPin, JobPreview, JobState};
use dtool_core::object::Object;
use dtool_core::workspace::{CloseAnswer, SharedServices, Workspace};
use std::sync::Arc;

fn workspace() -> Workspace {
    Workspace::new(Arc::new(SharedServices::default()))
}

fn add_object(ws: &mut Workspace, ctx: dtool_core::workspace::ContextId) -> dtool_core::object::ObjectId {
    ws.get_mut(ctx).unwrap().session.document.create_object(Object { pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() })
}

fn pin(ws: &Workspace, job: u64, ctx: dtool_core::workspace::ContextId, target: dtool_core::object::ObjectId) -> JobPin {
    JobPin { job_id: JobId(job), context: ctx, target: Some(target), source_generation: ws.get(ctx).unwrap().session.generation }
}

fn complete(ws: &mut Workspace, job: u64, target: dtool_core::object::ObjectId) {
    let record = ws.jobs_mut().get_mut(JobId(job)).unwrap();
    let mut object = Object { pos: [5.0, 5.0], size: [20.0, 20.0], ..Object::default() };
    object.id = target;
    record.preview = Some(JobPreview { objects: vec![object] });
    record.state = JobState::Completed;
}

#[test]
fn closing_a_context_cancels_its_jobs_and_removes_temps() {
    let mut ws = workspace();
    let a = ws.new_document("a");
    let b = ws.new_document("b");
    let target = add_object(&mut ws, a);
    let token = ws.register_job(pin(&ws, 1, a, target)).unwrap();
    let temp = std::env::temp_dir().join(format!("k15_job_temp_{}", std::process::id()));
    std::fs::write(&temp, b"x").unwrap();
    ws.jobs_mut().get_mut(JobId(1)).unwrap().temp_files.push(temp.clone());
    ws.jobs_mut().get_mut(JobId(1)).unwrap().state = JobState::Running;
    ws.close_with(a, CloseAnswer::No).unwrap();
    assert!(token.is_cancelled());
    assert_eq!(ws.jobs().get(JobId(1)).unwrap().state, JobState::Cancelled);
    assert!(!temp.exists());
    assert!(ws.apply_job(JobId(1)).is_err());
    assert!(ws.get(b).unwrap().session.document.objects.is_empty());
}

#[test]
fn register_requires_live_context() {
    let mut ws = workspace();
    let a = ws.new_document("a");
    let target = add_object(&mut ws, a);
    let mut stale = pin(&ws, 2, a, target);
    stale.context = dtool_core::workspace::ContextId(999);
    assert_eq!(ws.register_job(stale).unwrap_err().code, "CONTEXT_NOT_FOUND");
}

#[test]
fn apply_is_one_commit_and_stale_is_rejected() {
    let mut ws = workspace();
    let a = ws.new_document("a");
    let target = add_object(&mut ws, a);
    ws.register_job(pin(&ws, 3, a, target)).unwrap();
    complete(&mut ws, 3, target);
    let generation = ws.get(a).unwrap().session.generation;
    ws.apply_job(JobId(3)).unwrap();
    let session = &ws.get(a).unwrap().session;
    assert_eq!(session.generation, generation + 1);
    assert_eq!(session.document.get_object(target).unwrap().size, [20.0, 20.0]);

    let target2 = add_object(&mut ws, a);
    ws.register_job(pin(&ws, 4, a, target2)).unwrap();
    ws.get_mut(a).unwrap().session.run_named("object.update", serde_json::json!({"id": target2.0, "pos": [1.0, 1.0]})).unwrap();
    complete(&mut ws, 4, target2);
    let before = serde_json::to_string(&ws.get(a).unwrap().session.document).unwrap();
    assert_eq!(ws.apply_job(JobId(4)).unwrap_err().code, "STALE");
    assert_eq!(serde_json::to_string(&ws.get(a).unwrap().session.document).unwrap(), before);
}

#[test]
fn cancelled_job_is_not_completed() {
    let mut ws = workspace();
    let a = ws.new_document("a");
    let target = add_object(&mut ws, a);
    ws.register_job(pin(&ws, 5, a, target)).unwrap();
    ws.jobs_mut().get_mut(JobId(5)).unwrap().cancel();
    assert_eq!(ws.jobs().get(JobId(5)).unwrap().state, JobState::Cancelled);
    assert_eq!(ws.apply_job(JobId(5)).unwrap_err().code, "CANCELLED");
    assert_eq!(ws.jobs().list().count(), 1);
}
