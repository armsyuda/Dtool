use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const DOC_OPEN_RECENT: &str = "doc.open_recent";
pub const DOC_RECENT_LIST: &str = "doc.recent.list";
pub const DOC_RECENT_CLEAR: &str = "doc.recent.clear";
pub const DOC_RECENT_REMOVE: &str = "doc.recent.remove";
pub const RECENT_MAX: usize = 10;

static RECENT: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
static CAP: AtomicUsize = AtomicUsize::new(0);

pub fn cap() -> usize {
    match CAP.load(Ordering::Relaxed) {
        0 => {
            let loaded = crate::prefs::Prefs::load(&crate::prefs::Prefs::default_path()).recent_cap();
            let _ = CAP.compare_exchange(0, loaded, Ordering::Relaxed, Ordering::Relaxed);
            CAP.load(Ordering::Relaxed)
        }
        cap => cap,
    }
}

pub fn set_cap(count: usize) -> usize {
    let cap = count.max(1);
    CAP.store(cap, Ordering::Relaxed);
    recent().truncate(cap);
    cap
}

fn recent() -> MutexGuard<'static, Vec<PathBuf>> {
    let lock = RECENT.get_or_init(|| Mutex::new(Vec::new()));
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn push(path: &Path) {
    let mut list = recent();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(cap());
}

pub fn list() -> Vec<PathBuf> {
    recent().clone()
}

fn list_json() -> serde_json::Value {
    serde_json::json!(list().iter().map(|p| p.display().to_string()).collect::<Vec<_>>())
}

pub fn can_execute(_session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        DOC_OPEN_RECENT => Some(command.path_param().map_or(false, |p| p.exists())),
        DOC_RECENT_LIST => Some(true),
        DOC_RECENT_CLEAR => Some(true),
        DOC_RECENT_REMOVE => Some(command.path_param().is_some()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        DOC_RECENT_LIST => Some(Ok(serde_json::json!({ "recent": list_json() }))),
        DOC_RECENT_CLEAR => {
            recent().clear();
            Some(Ok(serde_json::json!({ "recent": [] })))
        }
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != DOC_RECENT_LIST && command.name != DOC_RECENT_CLEAR {
        return None;
    }
    run(session, command).map(|result| result.map(|_| ()))
}
