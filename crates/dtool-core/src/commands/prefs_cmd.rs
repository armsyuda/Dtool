use crate::prefs::{Prefs, CMD_PERFORMANCE_SET, CMD_QUERY, CMD_SET};
use crate::{Command, CommandError, CommandResult, Session};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

struct Store {
    path: PathBuf,
    prefs: Prefs,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

pub fn install(path: PathBuf) -> bool {
    let prefs = Prefs::load(&path);
    STORE.set(Mutex::new(Store { path, prefs })).is_ok()
}

pub fn installed() -> bool {
    STORE.get().is_some()
}

pub fn snapshot() -> Option<Prefs> {
    STORE.get()?.lock().ok().map(|store| store.prefs.clone())
}

pub fn replace(prefs: Prefs) -> bool {
    let Some(store) = STORE.get() else { return false };
    let Ok(mut store) = store.lock() else { return false };
    crate::commands::recent::set_cap(prefs.recent_cap());
    store.prefs = prefs;
    true
}

pub fn reload_from_disk() -> Result<Prefs, CommandError> {
    let store = STORE.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?;
    let mut store = store.lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?;
    let prefs = Prefs::load(&store.path);
    crate::commands::recent::set_cap(prefs.recent_cap());
    store.prefs = prefs.clone();
    Ok(prefs)
}

pub fn can_execute(_session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), CMD_QUERY | CMD_SET | CMD_PERFORMANCE_SET).then_some(installed())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command)?;
    Some((|| {
        let (reply, prefs) = {
            let store = STORE.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?;
            let mut store = store.lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?;
            let path = store.path.clone();
            let reply = store.prefs.run(&command.name, &command.params, &path)?;
            (reply, store.prefs.clone())
        };
        if command.name != CMD_QUERY {
            apply_to_session(session, &prefs);
        }
        Ok(reply)
    })())
}

pub fn undo_cap(prefs: &Prefs) -> usize {
    prefs.undo_limit.clamp(1, crate::commands::history_menu::MAX_HISTORY_STEPS) as usize
}

pub fn apply_to_session(session: &mut Session, prefs: &Prefs) {
    session.undo.set_limit(undo_cap(prefs));
    session.autosave.set(crate::autosave::AutosaveSetArgs {
        enabled: Some(prefs.recovery_interval_s > 0),
        interval_s: Some(prefs.recovery_interval_s),
    });
}
