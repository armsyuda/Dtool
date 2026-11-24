use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use std::sync::OnceLock;

pub const FONT_LIST: &str = "font.list";
pub const FONT_FILES: &str = "font.files";
pub const TEXT_FONT_PREVIEW: &str = "text.font.preview";
pub const TEXT_FONT_PREVIEW_CANCEL: &str = "text.font.preview.cancel";
pub const TEXT_FIND: &str = "text.find";
pub const TEXT_REPLACE: &str = "text.replace";
pub const TEXT_REPLACE_ALL: &str = "text.replace_all";
pub const TEXT_SPELL_CHECK: &str = "text.spell.check";
pub const TEXT_SPELL_SUGGEST: &str = "text.spell.suggest";
pub const TEXT_SPELL_REPLACE: &str = "text.spell.replace";
pub const TEXT_SPELL_IGNORE: &str = "text.spell.ignore";
pub const TEXT_SPELL_ADD_WORD: &str = "text.spell.add_word";
pub const TEXT_SPELL_IGNORE_RESTORE: &str = "text.spell.ignore.restore";
pub const NAMES: [&str; 13] = [FONT_LIST, FONT_FILES, TEXT_FONT_PREVIEW, TEXT_FONT_PREVIEW_CANCEL, TEXT_FIND, TEXT_REPLACE, TEXT_REPLACE_ALL, TEXT_SPELL_CHECK, TEXT_SPELL_SUGGEST, TEXT_SPELL_REPLACE, TEXT_SPELL_IGNORE, TEXT_SPELL_ADD_WORD, TEXT_SPELL_IGNORE_RESTORE];

pub type HostRun = fn(&mut Session, &Command) -> Option<CommandResult>;
pub type HostCan = fn(&Session, &Command) -> Option<bool>;

#[derive(Clone, Copy)]
struct Host {
    run: HostRun,
    can: HostCan,
}

static HOST: OnceLock<Host> = OnceLock::new();

pub fn install(run: HostRun, can: HostCan) -> bool {
    HOST.set(Host { run, can }).is_ok() || HOST.get().is_some_and(|host| host.run as usize == run as usize)
}

pub const RECENT_FONTS_MAX: usize = 10;

static RECENT_FONTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn recent_slot() -> std::sync::MutexGuard<'static, Vec<String>> {
    RECENT_FONTS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn push_recent_font(family: &str) {
    let family = family.trim();
    if family.is_empty() { return; }
    let mut list = recent_slot();
    list.retain(|f| !f.eq_ignore_ascii_case(family));
    list.insert(0, family.to_owned());
    list.truncate(RECENT_FONTS_MAX);
}

pub fn recent_fonts() -> Vec<String> {
    recent_slot().clone()
}

pub fn clear_recent_fonts() {
    recent_slot().clear();
}

pub fn installed() -> bool {
    HOST.get().is_some()
}

fn owns(command: &Command) -> bool {
    NAMES.contains(&command.name.as_str())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !owns(command) { return None; }
    Some(HOST.get().and_then(|host| (host.can)(session, command)).unwrap_or(false))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !owns(command) { return None; }
    Some(match HOST.get() {
        Some(host) => (host.run)(session, command).unwrap_or_else(|| Err(CommandError::not_implemented())),
        None => Err(CommandError::not_implemented()),
    })
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    run(session, command).map(|result| result.map(|_| ()))
}
