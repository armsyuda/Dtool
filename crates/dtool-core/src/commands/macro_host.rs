use crate::command::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use std::sync::OnceLock;

pub const NAMES: [&str; 7] = ["macro.record.start", "macro.record.stop", "macro.play", "macro.reset", "macro.library.add", "macro.export", "macro.import"];

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