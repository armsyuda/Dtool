use crate::menu_tree::{MenuId, MenuItem, MenuTree, command_menu_item, command_menu_tooltip, context_command_menu};
use dtool_core::{CommandError,Session};
use serde_json::Value;

pub const ADD_TO_SWATCHES:&str="swatch.add_from_selection";
pub const DIALOG_ROWS:&[&str]=&[crate::menu_tree::LAYER_EFFECTS_MENU_ID, crate::dialogs::tone_blend::MENU_ID];

pub fn dialog_of(result:&Value)->Option<&str>{result.get("dialog").and_then(Value::as_str)}

fn mark_dialog_rows(rows:&mut Value,session:&Session){
    let Some(items)=rows.as_array_mut() else {return};
    for item in items {
        let id=item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        if DIALOG_ROWS.contains(&id.as_str()) || id == crate::menu_tree::FIND_LAYER_MENU_ID {
            let enabled = if id == crate::menu_tree::FIND_LAYER_MENU_ID {
                crate::panels::layers::reveal_target(session).is_some()
            } else if id == crate::dialogs::tone_blend::MENU_ID {
                session.can_execute_named(dtool_core::commands::tone_blend::CREATE)
            } else {
                crate::mask_actions::target(session).is_some()
            };
            item["dialog"]=Value::String(id);
            item["planned"]=Value::Bool(false);
            item["planned_key"]=Value::Null;
            item["error_code"]=Value::Null;
            item["enabled"]=Value::Bool(session.has_document && enabled);
        }
        if let Some(children)=item.get_mut("children"){mark_dialog_rows(children,session);}
    }
}

pub fn install()->bool {dtool_core::commands::context_menu::register_provider(query)}

pub fn query(persona:&str,target:&str)->Value {
    let tree=MenuTree::for_persona(persona);
    let ids:&[&'static str]=if target.eq_ignore_ascii_case("TextEditing") {EDITING}else{OBJECT};
    Value::Array(ids.iter().map(|id| {
        let item=tree.find(id).cloned().unwrap_or_else(||custom(id));
        let mut value=item.to_json();
        if *id=="edit.paste.paste"{value["command"]=Value::String("edit.paste".into());}
        if *id=="context.add_to_swatches"{value["command"]=Value::String(ADD_TO_SWATCHES.into());value["planned"]=Value::Bool(false);}
        value
    }).collect())
}

fn custom(id:&'static str)->MenuItem {
    let key=match id {
        "context.develop"=>"menu.context.develop",
        "context.liquify"=>"menu.context.liquify",
        "context.move_selection_to_new_layer"=>"menu.context.move_selection_to_new_layer",
        "context.create_style"=>"menu.context.create_style",
        "context.preflight_comment"=>"menu.context.preflight_comment",
        "context.add_to_swatches"=>"menu.context.add_to_swatches",
        "context.convert_text_to_field"=>"menu.context.convert_text_to_field",
        "context.insert_cross_reference"=>"menu.context.insert_cross_reference",
        _=>"menu.planned.suffix",
    };
    MenuItem{id:MenuId(id),key,command:None,children:Vec::new(),separator:false,planned:true,checkable:false}
}

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum SpellChoice {Replace(String),Ignore,AddWord}

pub fn spell_choice(session:&mut Session,choice:&SpellChoice)->Option<Result<Value,CommandError>>{
    let (story,mark,_)=crate::canvas::frame_text_input::spell_at_caret(session)?;
    let result=match choice {
        SpellChoice::Replace(with)=>session.run_named("text.spell.replace",serde_json::json!({"story":story.0,"start":mark.start,"end":mark.end,"word":mark.word,"with":with})),
        SpellChoice::Ignore=>session.run_named("text.spell.ignore",serde_json::json!({"word":mark.word})),
        SpellChoice::AddWord=>session.run_named("text.spell.add_word",serde_json::json!({"word":mark.word})),
    };
    let ignored=matches!(choice,SpellChoice::AddWord).then_some(mark.word.as_str());
    crate::canvas::frame_text_input::refresh_spelling(session,ignored);
    Some(result)
}

fn show_spelling(ui:&mut egui::Ui,session:&mut Session)->Option<Result<Value,CommandError>>{
    let (_,_,suggestions)=crate::canvas::frame_text_input::spell_at_caret(session)?;
    let mut choice=None;
    for word in suggestions.iter().take(SPELL_MENU_SUGGESTIONS) {
        if command_menu_item(ui,word,true).clicked(){choice=Some(SpellChoice::Replace(word.clone()));}
    }
    if command_menu_item(ui,dtool_strings::text("menu.text.spelling.ignore_misspelled_word"),true).clicked(){choice=Some(SpellChoice::Ignore);}
    if command_menu_item(ui,dtool_strings::text("panel.spelling.add_word"),true).clicked(){choice=Some(SpellChoice::AddWord);}
    ui.separator();
    let result=choice.and_then(|choice|spell_choice(session,&choice));
    if result.is_some(){ui.close();}
    result
}

pub const SPELL_MENU_SUGGESTIONS:usize=5;

pub fn show(ui:&mut egui::Ui,session:&mut Session,persona:&str,target:&str)->Option<Result<Value,CommandError>>{
    let text_editing = target.eq_ignore_ascii_case("TextEditing");
    if text_editing {
        if let Some(result)=show_spelling(ui,session){return Some(result);}
    }
    let mut rows=query(persona,target);
    dtool_core::commands::menus::resolve_items(&mut rows,session);
    mark_dialog_rows(&mut rows,session);
    let items = rows.as_array_mut()?;
    if text_editing {
        return show_rows(ui, session, items, false);
    }
    for item in items.iter_mut().filter(|item| item["id"] == "layer.arrange") {
        if let Some(children) = item["children"].as_array_mut() {
            children.retain(|child| !matches!(child["id"].as_str(), Some("layer.arrange.move" | "layer.arrange.align")));
        }
    }
    let split = items.iter().position(|item| item["id"] == "layer.layer_effects").unwrap_or(items.len() / 2);
    let (left, right) = items.split_at(split);
    ui.spacing_mut().item_spacing.y = 1.0;
    if ui.available_width() < 360.0 {
        return show_rows(ui, session, left, true).or_else(|| {
            ui.separator();
            show_rows(ui, session, right, true)
        });
    }
    ui.columns(2, |columns| {
        let left_action = show_rows(&mut columns[0], session, left, true);
        let right_action = show_rows(&mut columns[1], session, right, true);
        left_action.or(right_action)
    })
}

fn show_rows(ui:&mut egui::Ui,session:&mut Session,rows:&[Value], grouped:bool)->Option<Result<Value,CommandError>> {
    let mut action=None;
    for (index, item) in rows.iter().enumerate() {
        if item.get("separator").and_then(Value::as_bool).unwrap_or(false){ui.separator();continue;}
        if grouped && index > 0 && matches!(item["id"].as_str(), Some(
            "layer.group" | "layer.select.next_layer" | "layer.arrange.move" | "vector.convert_to_curves"
            | "layer.select.set_selection_box" | "pixel.rasterise.rasterise_and_trim" | "context.develop"
            | "context.create_style" | "layer.find_in_layers_panel"
        )) {
            ui.separator();
        }
        let label=item.get("label").and_then(Value::as_str).unwrap_or("");
        if let Some(children) = item["children"].as_array().filter(|children| !children.is_empty()) {
            let submenu = ui.menu_button(label, |ui| {
                if let Some(result) = context_command_menu(ui, |ui| show_rows(ui,session,children,false)) {
                    action = Some(result);
                }
            });
            command_menu_tooltip(ui, submenu.response, label, true, None);
        } else {
            let enabled=item.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            if command_menu_item(ui,label,enabled).clicked(){
                if let Some(dialog)=item.get("dialog").and_then(Value::as_str){action=Some(Ok(serde_json::json!({"dialog":dialog})));ui.close();}
                else if let Some(command)=item.get("command").and_then(Value::as_str){action=Some(session.run_named(command,serde_json::json!({})));ui.close();}
            }
        }
    }
    action
}

const OBJECT:&[&str]=&["edit.cut", "edit.copy", "edit.paste.paste", "layer.group", "layer.ungroup", "layer.ungroup_all", "pixel.live_tone_blend_group", "layer.select.next_layer", "layer.select.previous_layer", "layer.select.parent_layer", "layer.select.top_layer", "layer.select.bottom_layer", "layer.arrange.move", "layer.arrange.align", "vector.geometry", "layer.arrange", "layer.layer_effects", "vector.convert_to_curves", "layer.convert.to_picture_frame", "layer.convert.to_image_resource", "layer.convert.to_text_frame", "layer.convert.to_art_text", "vector.expand_stroke", "layer.select.set_selection_box", "layer.create_clipping_mask", "pixel.rasterise.rasterise_and_trim", "pixel.rasterise.rasterise", "pixel.rasterise.rasterise_to_mask", "context.develop", "context.liquify", "pixel.filters.tone_map", "context.move_selection_to_new_layer", "context.create_style", "context.preflight_comment", "context.add_to_swatches", "layer.find_in_layers_panel", "text.interactive"];
const EDITING:&[&str]=&["context.convert_text_to_field", "edit.cut", "edit.copy", "edit.paste.paste", "text.capitalisation", "edit.find.find", "text.insert.insert_filler_text", "text.notes", "text.insert", "text.insert.fields", "text.interactive.insert_anchor", "text.interactive.insert_hyperlink", "context.insert_cross_reference", "layer.find_in_layers_panel"];
