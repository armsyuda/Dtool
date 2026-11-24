use dtool_core::command::Session;
use dtool_core::text_link::{modified_links,text_link_rows,TextLinkRow};
use dtool_core::{Command,CommandResult};
use dtool_strings::text;
use serde_json::json;

pub const IMPORT_FROM_FILES:&str="file.import_content.from_file_s";
pub const IMPORT_LINKED:&str="file.placement_policy.import_text_files_as_linked";
const CHECK_SECONDS:f64=2.0;

pub fn import_text(session:&mut Session,path:&str,linked:Option<bool>)->CommandResult {
    let mut params=json!({"path":path});
    if let Some(linked)=linked {params["linked"]=linked.into();}
    session.run(&Command::with_params("text.import",params))
}

pub fn update_text_link(session:&mut Session,story:u64)->CommandResult {
    session.run(&Command::with_params("text.import.update",json!({"story":story})))
}

pub fn linked_text_rows(session:&Session)->Vec<TextLinkRow> {
    text_link_rows(&session.document)
}

#[derive(Clone,Default)]
struct PromptState {
    checked:Option<f64>,
    pending:Vec<TextLinkRow>,
    later:Vec<(u64,String)>,
    error:Option<String>,
}

fn link_hash(session:&Session,story:u64)->String {
    session.document.text_links.iter().find(|link|link.story.0==story).map(|link|link.hash.clone()).unwrap_or_default()
}

pub fn show_text_link_prompt(ctx:&egui::Context,session:&mut Session) {
    if session.document.text_links.is_empty() {return;}
    let key=egui::Id::new("dtool.text_link.prompt");
    let now=ctx.input(|input|input.time);
    let mut state:PromptState=ctx.data_mut(|data|data.get_temp(key).unwrap_or_default());
    if state.checked.is_none_or(|checked|now-checked>=CHECK_SECONDS) {
        state.checked=Some(now);
        let later=state.later.clone();
        state.pending=modified_links(&session.document).into_iter().filter(|row|!later.contains(&(row.story.0,link_hash(session,row.story.0)))).collect();
    }
    if !state.pending.is_empty() {
        let mut update=Vec::new();
        let mut defer=false;
        egui::Window::new(text("dialog.text_link.title")).collapsible(false).show(ctx,|ui| {
            dtool_design::dialog_frame(ui,|ui| {
                for row in &state.pending {
                    ui.horizontal(|ui| {
                        ui.label(&row.name);
                        if dtool_design::button(ui,text("dialog.text_link.update")).clicked() {update.push(row.story.0);}
                    });
                }
                if let Some(message)=&state.error {ui.label(message);}
                if dtool_design::button(ui,text("dialog.text_link.later")).clicked() {defer=true;}
            });
        });
        for story in update {
            match update_text_link(session,story) {
                Ok(_)=>{state.pending.retain(|row|row.story.0!=story);state.error=None;}
                Err(error)=>state.error=Some(text(&error.message_key).to_string()),
            }
        }
        if defer {
            let pending=std::mem::take(&mut state.pending);
            state.later.extend(pending.iter().map(|row|(row.story.0,link_hash(session,row.story.0))));
            state.error=None;
        }
    }
    ctx.data_mut(|data|data.insert_temp(key,state));
}
