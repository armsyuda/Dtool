use crate::{Command,CommandError,CommandResult,Session};
use std::sync::{Mutex,OnceLock};
static ACTION:OnceLock<fn(&Command)->CommandResult>=OnceLock::new();
static SUB_DEPTH:Mutex<Option<usize>>=Mutex::new(None);
pub fn register_action(action:fn(&Command)->CommandResult)->bool{ACTION.set(action).is_ok()}

pub fn layout_active() -> bool {
    run_without_document(&Command::new("ui.persona.query"))
        .and_then(Result::ok).is_some_and(|value| value["id"] == "layout")
}

pub(crate) fn run_without_document(command: &Command) -> Option<CommandResult> {
    let action = ACTION.get()?;
    matches!(command.name.as_str(), "ui.persona.set" | "ui.persona.query" | "ui.persona.list")
        .then(|| action(command))
}

fn default_view_mode(persona: &str) -> Option<crate::ViewMode> {
    match persona {
        "vector" | "layout" | "slice" => Some(crate::ViewMode::Normal),
        "pixel" | "ai_image" => Some(crate::ViewMode::Pixel),
        _ => None,
    }
}

fn base_persona(active: &serde_json::Value) -> Option<&str> {
    active["base_id"].as_str().or_else(|| active["id"].as_str())
}

pub(crate) fn current_default_view_mode() -> Option<crate::ViewMode> {
    let active = run_without_document(&Command::new("ui.persona.query"))?.ok()?;
    default_view_mode(base_persona(&active)?)
}

pub(crate) fn apply_default_view_mode(view: &mut crate::view_state::ViewState, mode: crate::ViewMode) {
    view.canvas_flags.view_mode = mode;
    view.xray = false;
}
pub fn can_execute(_session:&Session,command:&Command)->Option<bool>{matches!(command.name.as_str(),"ui.persona.set"|"ui.persona.query"|"ui.persona.list"|"ui.persona.enter_sub"|"ui.persona.leave_sub"|"ui.studio.create"|"ui.studio.update"|"ui.studio.rename"|"ui.studio.delete"|"ui.studio.list"|"ui.studio.export"|"ui.studio.import").then_some(ACTION.get().is_some())}
pub(crate) fn sync_document_mode(session: &mut Session) {
    let Some(Ok(active)) = run_without_document(&Command::new("ui.persona.query")) else { return };
    let target = match (active["id"].as_str(), session.document.layout_artboard_origins.is_some()) {
        (Some("vector"), true) => "layout",
        (Some("layout"), false) => "vector",
        _ => return,
    };
    let _ = select_document_persona(session, target);
}

pub(super) fn select_document_persona(session: &mut Session, target: &str) -> Option<crate::ViewMode> {
    let command = Command::with_params("ui.persona.set", serde_json::json!({"id": target}));
    let value = run_without_document(&command)?.ok()?;
    if let Some(tool) = value["active_tool"].as_str() {
        session.active_tool = crate::context_toolbar::active_id(tool);
    }
    default_view_mode(target)
}

fn switches(name:&str)->bool{matches!(name,"ui.persona.set"|"ui.persona.enter_sub"|"ui.persona.leave_sub")}
fn finish_pending(session:&mut Session,command:&Command)->Result<(),CommandError>{
    let pending=session.text_edit.is_some()||session.text_composing.is_some()||session.edit.is_some();
    if !pending{return Ok(());}
    let mode=match command.params.get("pending_edit"){None|Some(serde_json::Value::Null)=>"commit",Some(serde_json::Value::String(mode))=>mode.as_str(),Some(_)=>return Err(CommandError::invalid_params())};
    let cancel=match mode{"commit"=>false,"cancel"=>true,"abort"=>return Err(CommandError::new("SWITCH_CANCELLED")),_=>return Err(CommandError::invalid_params())};
    if session.edit.is_some(){session.end_edit(if cancel{crate::edit_session::EditEnd::Cancel}else{crate::edit_session::EditEnd::Commit})?;}
    if session.text_edit.is_some()||session.text_composing.is_some(){crate::text_cmd::run(&Command::with_params("text.edit.end",serde_json::json!({"cancel":cancel})),session)?;}
    Ok(())
}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult>{
    can_execute(session,command)?;
    Some((||{
        let action=ACTION.get().ok_or_else(||CommandError::new("NOT_AVAILABLE"))?;
        let previous = if switches(&command.name) {
            finish_pending(session,command)?;
            Some(action(&Command::new("ui.persona.query"))?)
        } else {
            None
        };
        use super::page_artboards::{self, FROM_ARTBOARDS, TO_ARTBOARDS};
        let mut value=action(command)?;
        let bridge = if session.has_document && command.name == "ui.persona.set" {
            match base_persona(&value) {
                Some("layout") => page_artboards::enter_layout(&session.document, true)
                    .map(|(doc, links)| (FROM_ARTBOARDS, doc, links)),
                Some("vector") if session.document.layout_artboard_origins.is_some()
                    || previous.as_ref().and_then(base_persona) == Some("layout") =>
                    page_artboards::enter_vector(&session.document)
                        .map(|(doc, links)| (TO_ARTBOARDS, doc, links)),
                _ => None,
            }
        } else {
            None
        };
        if let Some((name, document, links)) = bridge {
            let change = page_artboards::commit(session, name, document, links)?;
            value["links"] = change["links"].clone();
            value["changes"] = change["changes"].clone();
        }
        if command.name=="ui.persona.enter_sub"{*SUB_DEPTH.lock().unwrap_or_else(|e|e.into_inner())=Some(session.undo.applied_steps());}
        if command.name=="ui.persona.leave_sub"&&value.get("noop").is_none(){
            let depth=SUB_DEPTH.lock().unwrap_or_else(|e|e.into_inner()).take();
            if value.get("applied").and_then(|v|v.as_bool())==Some(false){
                let mut reverted=0;
                if let Some(depth)=depth{while session.undo.applied_steps()>depth{session.run(&Command::new("edit.undo"))?;reverted+=1;}}
                value["reverted"]=serde_json::json!(reverted);
            }
        }
        if command.name=="ui.persona.set"{if let Some(tool)=value.get("active_tool").and_then(|value|value.as_str()){session.active_tool=crate::context_toolbar::active_id(tool);}}
        if let Some(previous) = previous {
            let restored = (command.name == "ui.persona.leave_sub")
                .then(|| action(&Command::new("ui.persona.query"))).transpose()?;
            let active = restored.as_ref().unwrap_or(&value);
            if previous["id"] != active["id"]
                && let Some(mode) = base_persona(active).and_then(default_view_mode)
            {
                apply_default_view_mode(&mut session.view, mode);
                value["view_mode_default"] = serde_json::json!(mode);
            }
        }
        Ok(value)
    })())
}
