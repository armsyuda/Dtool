use crate::dock::Layout;
use crate::panels::{run_ui_dock_command, PanelRegistry};
use crate::presets::PresetStore;
use crate::ui_host::is_dock_command;
use dtool_core::commands::palettes::{is_swatches_set, UI_PANEL_SET};
use dtool_core::workspace::{register_ui_session_route, ui_session_route_ready, UiSessionSlot, Workspace};
use dtool_protocol::{Command, CommandResult};
use std::path::PathBuf;

#[derive(Default)]
pub struct SessionDock {
    pub layout: Layout,
    pub store: PresetStore,
    pub persist_path: Option<PathBuf>,
    pub revision: u64,
}

thread_local! {
    static REGISTRY: PanelRegistry = PanelRegistry::with_existing_bodies();
}

pub fn is_session_routed(command: &Command) -> bool {
    is_dock_command(&command.name) && !(command.name == UI_PANEL_SET && is_swatches_set(&command.params))
}

fn slot_dock(slot: &mut UiSessionSlot) -> Option<&mut SessionDock> {
    slot.get_or_insert_with(|| Box::new(SessionDock::default())).downcast_mut::<SessionDock>()
}

pub fn route(slot: &mut UiSessionSlot, command: &Command) -> Option<CommandResult> {
    if !is_session_routed(command) {
        return None;
    }
    let dock = slot_dock(slot)?;
    let result = REGISTRY.with(|registry| run_ui_dock_command(&mut dock.layout, registry, &mut dock.store, &command.name, &command.params, dock.persist_path.as_deref()));
    if result.is_some() {
        dock.revision = dock.revision.wrapping_add(1);
    }
    result
}

pub fn install() -> bool {
    register_ui_session_route(route)
}

pub fn ready() -> bool {
    ui_session_route_ready()
}

pub fn dock(workspace: &Workspace) -> Option<&SessionDock> {
    workspace.ui_session().0.as_ref()?.downcast_ref::<SessionDock>()
}

pub fn dock_mut(workspace: &mut Workspace) -> Option<&mut SessionDock> {
    slot_dock(&mut workspace.ui_session_mut().0)
}