use crate::inpaint::CancelToken;
use crate::object::{Object, ObjectId};
use crate::workspace::ContextId;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Cancelled,
    Failed,
    Completed,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Cancelled | JobState::Failed | JobState::Completed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobProgress {
    pub done: u64,
    pub total: u64,
}

impl JobProgress {
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        self.done.min(self.total) as f32 / self.total as f32
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobPin {
    pub job_id: JobId,
    pub context: ContextId,
    pub target: Option<ObjectId>,
    pub source_generation: u64,
}

#[derive(Debug, Clone)]
pub struct JobPreview {
    pub objects: Vec<Object>,
}

pub struct JobRecord {
    pub pin: JobPin,
    pub state: JobState,
    pub progress: JobProgress,
    pub cancel: Arc<CancelToken>,
    pub timeout: Option<std::time::Duration>,
    pub temp_files: Vec<PathBuf>,
    pub preview: Option<JobPreview>,
}

impl JobRecord {
    pub fn new(pin: JobPin) -> Self {
        JobRecord { pin, state: JobState::Queued, progress: JobProgress::default(), cancel: Arc::new(CancelToken::new()), timeout: None, temp_files: Vec::new(), preview: None }
    }

    pub fn cancel(&mut self) {
        self.cancel.cancel();
        if !self.state.is_terminal() {
            self.state = JobState::Cancelled;
        }
        self.drop_outputs();
    }

    fn drop_outputs(&mut self) {
        for path in self.temp_files.drain(..) {
            let _ = std::fs::remove_file(path);
        }
        self.preview = None;
    }

    fn advance(&mut self, next: JobState) -> bool {
        if self.state.is_terminal() {
            return false;
        }
        if self.cancel.is_cancelled() {
            self.cancel();
            return false;
        }
        if next == JobState::Running && self.state != JobState::Queued {
            return false;
        }
        self.state = next;
        true
    }

    pub fn start(&mut self) -> bool {
        self.advance(JobState::Running)
    }

    pub fn complete(&mut self, preview: JobPreview) -> bool {
        if !self.advance(JobState::Completed) {
            return false;
        }
        self.progress.done = self.progress.total;
        self.preview = Some(preview);
        true
    }

    pub fn fail(&mut self) -> bool {
        if !self.advance(JobState::Failed) {
            return false;
        }
        self.drop_outputs();
        true
    }

    pub fn set_progress(&mut self, done: u64, total: u64) {
        if !self.state.is_terminal() {
            self.progress = JobProgress { done: done.min(total), total };
        }
    }

    pub fn check_timeout(&mut self, elapsed: std::time::Duration) -> bool {
        match self.timeout {
            Some(limit) if elapsed >= limit && !self.state.is_terminal() => {
                self.cancel.cancel();
                self.state = JobState::Failed;
                self.drop_outputs();
                true
            }
            _ => false,
        }
    }
}

#[derive(Default)]
pub struct JobRegistry {
    jobs: Vec<JobRecord>,
}

impl JobRegistry {
    pub fn register(&mut self, record: JobRecord) -> Result<(), dtool_protocol::CommandError> {
        if self.jobs.iter().any(|job| job.pin.job_id == record.pin.job_id) {
            return Err(dtool_protocol::CommandError::new("CONFLICT"));
        }
        self.jobs.push(record);
        Ok(())
    }

    pub fn get(&self, id: JobId) -> Option<&JobRecord> {
        self.jobs.iter().find(|job| job.pin.job_id == id)
    }

    pub fn get_mut(&mut self, id: JobId) -> Option<&mut JobRecord> {
        self.jobs.iter_mut().find(|job| job.pin.job_id == id)
    }

    pub fn list(&self) -> impl Iterator<Item = &JobRecord> {
        self.jobs.iter()
    }

    pub fn cancel_for_context(&mut self, context: ContextId) {
        for job in self.jobs.iter_mut().filter(|job| job.pin.context == context) {
            job.cancel();
        }
    }
}

impl std::fmt::Debug for JobRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.jobs.iter().map(|job| (job.pin.job_id, job.pin.context, job.state))).finish()
    }
}
