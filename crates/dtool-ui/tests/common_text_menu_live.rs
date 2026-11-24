#[path = "support/test_home.rs"] mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::{menus::text_format, shell::AppShell};
use serde_json::{json, Value};
fn row<'a>(tree:&'a Value,id:&str)->Option<&'a Value>{for item in tree.as_array()? {if item["id"]==id{return Some(item)} if let Some(found)=row(&item["children"],id){return Some(found)}} None}
fn call(shell:&mut AppShell,name:&str,p:Value)->Value{shell.workspace.run(Scope::active(),Command::with_params(name,p)).1.unwrap()}
#[test]
fn spacing_is_implemented_but_disabled_without_a_document_not_planned(){
    let tree=dtool_ui::menu_tree::queried_menu_tree(None,"vector",&[]);
    for id in ["text.spacing.use_default","text.spacing.use_none","text.spacing.tighten","text.spacing.loosen","text.spacing.tighten_more","text.spacing.loosen_more"]{
        let r=row(&tree,id).unwrap(); assert_eq!(r["planned"],false,"{r}"); assert_eq!(r["enabled"],false,"{r}"); assert!(!r["label"].as_str().unwrap().contains("예정"));
    }
}
#[test]
fn spacing_menu_preserves_mixed_run_values_and_is_one_undo(){
    let _home=test_home::enter("common_text_menu"); let mut shell=AppShell::new(); shell.workspace.new_document("menus");
    let added=call(&mut shell,"text.art.add",json!({"pos":[0,0],"text":"ABCD"})); let story=added["story"].as_u64().unwrap(); let id=added["id"].as_u64().unwrap();
    for (start,end,value) in [(0,2,10),(2,4,40)] {call(&mut shell,"text.style",json!({"story":story,"range":{"start":start,"end":end},"patch":{"tracking":value}}));}
    call(&mut shell,"selection.set",json!({"ids":[id]}));
    let before=shell.workspace.session().unwrap().document.stories[0].clone(); let depth=shell.workspace.session().unwrap().undo.applied_steps();
    let session=shell.workspace.session_mut().unwrap(); let tree=dtool_ui::menu_tree::queried_menu_tree(Some(session),"vector",&[]); assert_eq!(row(&tree,"text.spacing.tighten").unwrap()["enabled"],true);
    text_format::run(session,"text.spacing.tighten").unwrap();
    let ranges=&session.document.stories[0].ranges; assert_eq!(ranges.iter().find(|r|r["start"]==0).unwrap()["tracking"],-10.0); assert_eq!(ranges.iter().find(|r|r["start"]==2).unwrap()["tracking"],20.0);
    assert_eq!(session.undo.applied_steps(),depth+1); session.run(&Command::new("edit.undo")).unwrap(); assert_eq!(session.document.stories[0],before);
}
#[test]
fn visible_character_menu_actions_change_real_text_and_undo(){
    let _home=test_home::enter("common_text_menu_actions"); let mut shell=AppShell::new(); shell.workspace.new_document("menus");
    let added=call(&mut shell,"text.art.add",json!({"pos":[0,0],"text":"abCd"})); call(&mut shell,"selection.set",json!({"ids":[added["id"]]}));
    for id in ["text.character_traits.bold","text.character_traits.italic","text.character_traits.underline","text.character_traits.strikethrough","text.size.bigger","text.size.precise_smaller","text.spacing.use_none","text.baseline.superscript","text.baseline.raise","text.capitalisation.upper_case","text.capitalisation.small_caps"] {
        let session=shell.workspace.session_mut().unwrap(); let before=session.document.stories[0].clone(); let depth=session.undo.applied_steps(); text_format::run(session,id).unwrap_or_else(|e|panic!("{id}: {e:?}")); assert_ne!(session.document.stories[0],before,"{id}"); assert_eq!(session.undo.applied_steps(),depth+1,"{id}"); session.run(&Command::new("edit.undo")).unwrap(); assert_eq!(session.document.stories[0],before,"{id}");
    }
}
