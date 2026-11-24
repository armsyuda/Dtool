use crate::command::Session;
use dtool_protocol::{Command, CommandError};

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    if command.name != crate::autosave::CMD_AUTOSAVE_SET { return None; }
    Some((|| {
        let mut next = session.autosave.clone();
        let settings = next.run_set(&command.params)?;
        let path = crate::prefs::Prefs::default_path();
        let mut prefs = crate::prefs::Prefs::load(&path);
        prefs.recovery_interval_s = if settings.enabled { settings.recovery_interval_s } else { 0 };
        prefs.save(&path).map_err(|_| CommandError::new("IO_ERROR"))?;
        session.autosave = next;
        serde_json::to_value(settings).map_err(|_| CommandError::new("INTERNAL_ERROR"))
    })())
}

pub fn poll(session: &mut Session, name: &str, own_path: Option<&std::path::Path>) -> Result<Option<std::path::PathBuf>, CommandError> {
    let now = std::time::Instant::now();
    let version = session.document.version;
    if !session.has_document || !session.autosave.due(version, now) { return Ok(None); }
    let body = serde_json::to_string_pretty(&session.document).map_err(|_| CommandError::new("IO_ERROR"))?;
    let written = session.autosave.tick(version, now, name, own_path, || body)?;
    if let Some(target) = written.as_deref() {
        if session.history.save_with_document {
            let history = crate::history_store::HistoryStore::from_undo(&session.undo);
            session.autosave.write_history(&history, target, own_path)?;
        }
    }
    Ok(written)
}
