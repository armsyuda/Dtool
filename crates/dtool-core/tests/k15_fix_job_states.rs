use dtool_core::job::{JobId, JobPin, JobPreview, JobProgress, JobRecord, JobRegistry, JobState};
use dtool_core::object::{Object, ObjectId};
use dtool_core::workspace::ContextId;
use std::time::Duration;

fn record(id: u64) -> JobRecord {
    JobRecord::new(JobPin { job_id: JobId(id), context: ContextId(1), target: Some(ObjectId(7)), source_generation: 3 })
}

fn preview() -> JobPreview {
    JobPreview { objects: vec![Object::default()] }
}

#[test]
fn pins_are_listed_with_their_fields() {
    let mut registry = JobRegistry::default();
    registry.register(record(1)).unwrap();
    registry.register(record(2)).unwrap();
    assert_eq!(registry.register(record(2)).unwrap_err().code, "CONFLICT");
    let pins: Vec<JobPin> = registry.list().map(|job| job.pin.clone()).collect();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[1], JobPin { job_id: JobId(2), context: ContextId(1), target: Some(ObjectId(7)), source_generation: 3 });
    assert!(registry.list().all(|job| job.state == JobState::Queued));
}

#[test]
fn normal_run_goes_queued_running_completed_with_progress() {
    let mut job = record(1);
    assert!(job.start());
    assert!(!job.start());
    job.set_progress(15, 10);
    assert_eq!(job.progress, JobProgress { done: 10, total: 10 });
    job.set_progress(4, 10);
    assert!((job.progress.fraction() - 0.4).abs() < 1e-6);
    assert!(job.complete(preview()));
    assert_eq!(job.state, JobState::Completed);
    assert_eq!(job.progress.done, 10);
    assert!(job.preview.is_some());
    assert_eq!(JobProgress::default().fraction(), 0.0);
}

#[test]
fn a_cancelled_job_stays_cancelled_and_never_completes() {
    let mut job = record(1);
    job.start();
    let token = job.cancel.clone();
    token.cancel();
    assert!(!job.complete(preview()));
    assert_eq!(job.state, JobState::Cancelled);
    assert!(job.preview.is_none());
    assert!(!job.fail());
    assert_eq!(job.state, JobState::Cancelled);
    let mut direct = record(2);
    direct.cancel();
    assert!(!direct.start());
    assert_eq!(direct.state, JobState::Cancelled);
}

#[test]
fn timeout_fails_the_job_cancels_the_token_and_removes_temps() {
    let mut job = record(1);
    job.timeout = Some(Duration::from_millis(50));
    let temp = std::env::temp_dir().join(format!("dtool_k15_fix_{}.tmp", std::process::id()));
    std::fs::write(&temp, b"x").unwrap();
    job.temp_files.push(temp.clone());
    job.start();
    assert!(!job.check_timeout(Duration::from_millis(10)));
    assert_eq!(job.state, JobState::Running);
    assert!(job.check_timeout(Duration::from_millis(50)));
    assert_eq!(job.state, JobState::Failed);
    assert!(job.cancel.is_cancelled());
    assert!(!temp.exists());
    assert!(!job.complete(preview()));
    let mut done = record(2);
    done.timeout = Some(Duration::from_millis(1));
    done.start();
    done.complete(preview());
    assert!(!done.check_timeout(Duration::from_secs(1)));
    assert_eq!(done.state, JobState::Completed);
}
