use crate::command::Session;
use crate::document::Document;
use crate::resources::{ImageResource,ResourceId,ResourceStatus};
use crate::text_cmd::{param_u64,push_change};
use dtool_protocol::{Command,CommandError,CommandResult};
use serde_json::{json,Value};
use std::path::PathBuf;

pub const EMBED:&str="resource.embed";
pub const MAKE_LINKED:&str="resource.make_linked";
pub const UPDATE:&str="resource.update";
pub const RELINK:&str="resource.relink";
pub const UPDATE_ALL:&str="resource.update_all";
pub const RESTORE:&str="resource.restore";
pub const QUERY:&str="resource.query";
pub const REPLACE:&str="resource.replace";
pub const COLLECT: &str = "resource.collect";

pub fn handles(command:&Command)->bool {matches!(command.name.as_str(),EMBED|MAKE_LINKED|UPDATE|RELINK|UPDATE_ALL|RESTORE|QUERY|REPLACE|COLLECT)}

fn doc_dir(params:&Value)->Option<PathBuf> {
    params.get("doc_dir").and_then(Value::as_str).map(str::trim).filter(|dir|!dir.is_empty()).map(PathBuf::from)
}

fn path_param(params:&Value,key:&str)->Result<PathBuf,CommandError> {
    params.get(key).and_then(Value::as_str).map(str::trim).filter(|path|!path.is_empty()).map(PathBuf::from).ok_or_else(CommandError::invalid_params)
}

fn resource_id(params:&Value)->Result<ResourceId,CommandError> {Ok(ResourceId(param_u64(params,"id")?))}

fn collect_ids(params: &Value) -> Result<Vec<ResourceId>, CommandError> {
    let Some(ids) = params.get("ids") else {
        return Ok(vec![resource_id(params)?]);
    };
    let ids = ids.as_array().filter(|ids| !ids.is_empty()).ok_or_else(CommandError::invalid_params)?;
    let mut unique = std::collections::HashSet::new();
    ids.iter()
        .map(|value| value.as_u64().map(ResourceId).ok_or_else(CommandError::invalid_params))
        .filter(|id| id.as_ref().map_or(true, |id| unique.insert(*id)))
        .collect()
}

fn object_id(params:&Value)->Result<crate::object::ObjectId,CommandError> {
    for key in ["object_id","object","node","id"] {
        if let Some(value)=params.get(key) {
            if let Some(id)=value.as_u64() { return Ok(crate::object::ObjectId(id)); }
            if let Some(id)=value.as_str().and_then(|text|text.parse::<u64>().ok()) { return Ok(crate::object::ObjectId(id)); }
        }
    }
    Err(CommandError::invalid_params())
}

fn replace(session:&mut Session,params:&Value)->CommandResult {
    if let Some(value) = params.get("resource_id") {
        let id: ResourceId = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        let path = path_param(params, "path")?;
        let previous = session.document.image_resource(id).ok_or_else(CommandError::not_found)?;
        let decoded = crate::commands::image_place::decode_path(&path)?;
        let replacement = if previous.is_linked() {
            crate::resources::as_linked_resource(decoded.resource, path)
        } else {
            decoded.resource
        };
        let mut document = session.document.clone();
        crate::resources::replace_shared_image(&mut document, id, replacement, &decoded.rgba)?;
        let selection = session.selection.ids.clone();
        return super::document_change::commit(session, REPLACE, document, selection, json!({"resource": id.0}));
    }
    let object=object_id(params)?;
    let path=path_param(params,"path")?;
    let link=match params.get("link") {
        None|Some(Value::Null)=>session.document.placed_resource(object).and_then(|id|session.document.image_resource(id)).is_some_and(|resource|resource.is_linked()),
        Some(value)=>value.as_bool().ok_or_else(CommandError::invalid_params)?,
    };
    let decoded=crate::commands::image_place::decode_path(&path)?;
    let mut document=session.document.clone();
    let resource_id=crate::resources::replace_placement(&mut document,object,decoded.resource,decoded.rgba,link,Some(path))?;
    super::document_change::commit(session,REPLACE,document,vec![object],json!({"id":object.0,"object_id":object.0,"resource":resource_id.0}))
}


pub fn modified_images(doc:&Document,dir:Option<&std::path::Path>)->Vec<ResourceId> {
    doc.resources.iter().filter(|resource|resource.check_link(dir)==ResourceStatus::Modified).map(|resource|resource.id).collect()
}

pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    if !handles(command) {return None;}
    let doc=&session.document;
    Some(session.has_document && match command.name.as_str() {
        EMBED|MAKE_LINKED|UPDATE=>resource_id(&command.params).is_ok_and(|id|doc.resources.iter().any(|resource|resource.id==id)),
        RELINK => relink_ids(&command.params).is_ok_and(|ids| ids.iter().all(|&id| {
            doc.image_resource(id).is_some_and(|resource| ids.len() == 1 || resource.is_linked())
        })),
        UPDATE_ALL=>!modified_images(doc,doc_dir(&command.params).as_deref()).is_empty(),
        COLLECT => collect_ids(&command.params).is_ok_and(|ids| {
            ids.into_iter().all(|id| doc.image_resource(id).is_some_and(ImageResource::is_linked))
        }),
        REPLACE=>match command.params.get("resource_id") {
            Some(value) => serde_json::from_value::<ResourceId>(value.clone())
                .is_ok_and(|id| doc.image_resource(id).is_some()),
            None => object_id(&command.params).is_ok_and(|id| doc.get_object(id).is_some()),
        },
        _=>true,
    })
}

fn snapshot(doc:&Document,ids:&[ResourceId])->Result<Value,CommandError> {
    let kept:Vec<&ImageResource>=doc.resources.iter().filter(|resource|ids.contains(&resource.id)).collect();
    serde_json::to_value(kept).map_err(|_|CommandError::invalid_params())
}

fn run_change(session:&mut Session,name:&str,ids:Vec<ResourceId>,change:impl FnOnce(&mut Document)->Result<(),CommandError>)->CommandResult {
    let before=snapshot(&session.document,&ids)?;
    change(&mut session.document)?;
    let after=snapshot(&session.document,&ids)?;
    push_change(session,name,Command::with_params(RESTORE,json!({"resources":after})),Command::with_params(RESTORE,json!({"resources":before})));
    Ok(json!({"doc_version":session.generation,"ids":ids.iter().map(|id|id.0).collect::<Vec<u64>>()}))
}

fn query(session:&Session,params:&Value)->CommandResult {
    let dir=doc_dir(params);
    let images=crate::resources::resource_rows(&session.document,dir.as_deref());
    let texts=crate::text_link::text_link_rows(&session.document);
    Ok(json!({
        "images":serde_json::to_value(images).map_err(|_|CommandError::invalid_params())?,
        "texts":serde_json::to_value(texts).map_err(|_|CommandError::invalid_params())?,
    }))
}

fn reload_links(session: &mut Session, command: &Command, dir: Option<&std::path::Path>) -> CommandResult {
    let ids = match command.name.as_str() {
        UPDATE_ALL => modified_images(&session.document, dir),
        RELINK => relink_ids(&command.params)?,
        _ => vec![resource_id(&command.params)?],
    };
    if ids.is_empty() {
        return Err(CommandError::invalid_params());
    }

    let mut document = session.document.clone();
    if command.name == RELINK {
        let path = std::path::absolute(path_param(&command.params, "path")?)
            .map_err(|_| CommandError::invalid_params())?;
        if ids.len() > 1 {
            validate_relink_folder(&document, &ids, &path, dir)?;
        }
        for &id in &ids {
            crate::resources::relink_resource(&mut document, id, &path)?;
        }
    } else {
        for &id in &ids {
            crate::resources::update_resource(&mut document, id, dir)?;
        }
    }

    let selection = session.selection.ids.clone();
    super::document_change::commit(session, &command.name, document, selection, json!({"ids": ids}))
}

fn relink_ids(params: &Value) -> Result<Vec<ResourceId>, CommandError> {
    if params.get("id").is_some() && params.get("ids").is_some() {
        return Err(CommandError::invalid_params());
    }
    collect_ids(params)
}

fn validate_relink_folder(doc: &Document, ids: &[ResourceId], folder: &std::path::Path, dir: Option<&std::path::Path>) -> Result<(), CommandError> {
    if !folder.is_dir() {
        return Err(CommandError::validation("RELINK_FOLDER_REQUIRED"));
    }
    let mut sources = std::collections::HashMap::new();
    for &id in ids {
        let path = doc.image_resource(id).filter(|resource| resource.is_linked())
            .and_then(|resource| resource.resolved_path(dir)).ok_or_else(CommandError::invalid_params)?;
        let name = path.file_name().ok_or_else(CommandError::invalid_params)?;
        let target = std::fs::canonicalize(folder.join(name)).map_err(|_| CommandError::not_found())?;
        if let Some(previous) = sources.insert(target, path.clone()) && previous != path {
            return Err(CommandError::validation("RELINK_NAME_CONFLICT"));
        }
    }
    Ok(())
}

fn run_inner(session:&mut Session,command:&Command)->CommandResult {
    let params=&command.params;
    let dir=doc_dir(params);
    match command.name.as_str() {
        EMBED=>{let id=resource_id(params)?;run_change(session,EMBED,vec![id],|doc|crate::resources::embed_resource(doc,id,dir.as_deref()))}
        MAKE_LINKED=>{
            let id=resource_id(params)?;
            let folder=match path_param(params,"folder") {Ok(folder)=>folder,Err(error)=>dir.map(|dir|dir.join("Links")).ok_or(error)?};
            run_change(session,MAKE_LINKED,vec![id],|doc|crate::resources::make_linked_resource(doc,id,&folder))
        }
        UPDATE | RELINK | UPDATE_ALL => reload_links(session, command, dir.as_deref()),
        COLLECT => {
            let ids = collect_ids(params)?;
            let folder = path_param(params, "folder")?;
            run_change(session, COLLECT, ids.clone(), |doc| {
                crate::resources::collect_linked_resources(doc, &ids, &folder, dir.as_deref())
            })
        }
        REPLACE=>replace(session,params),
        QUERY=>query(session,params),
        _=>apply_doc(&mut session.document,command).unwrap_or_else(||Err(CommandError::invalid_params())).map(|_|json!({"doc_version":session.generation})),
    }
}

pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    if !handles(command) {return None;}
    Some(if session.has_document {run_inner(session,command)} else {Err(CommandError::no_document())})
}

pub fn apply_doc(doc:&mut Document,command:&Command)->Option<Result<(),CommandError>> {
    if command.name!=RESTORE {return None;}
    Some(restore(doc,&command.params))
}

fn restore(doc:&mut Document,params:&Value)->Result<(),CommandError> {
    let resources:Vec<ImageResource>=serde_json::from_value(params.get("resources").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
    for resource in resources {
        match doc.resources.iter_mut().find(|slot|slot.id==resource.id) {Some(slot)=>*slot=resource,None=>doc.resources.push(resource)}
    }
    Ok(())
}

pub fn apply(session:&mut Session,command:&Command)->Option<Result<(),CommandError>> {
    apply_doc(&mut session.document,command)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn make_linked_is_one_undo_step_and_query_lists_it() {
        let folder=std::env::temp_dir().join(format!("dtool_f1c_{}",std::process::id()));
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let mut resource=ImageResource::new_embedded(Default::default(),[1,1],72.0,vec![1,2,3]);
        resource.id=ResourceId(7);
        session.document.resources.push(resource);
        assert!(session.can_execute(&Command::with_params(EMBED,json!({"id":7}))));
        assert!(!session.can_execute(&Command::with_params(EMBED,json!({"id":8}))));
        session.run(&Command::with_params(MAKE_LINKED,json!({"id":7,"folder":folder.to_string_lossy()}))).unwrap();
        assert!(session.document.resources[0].is_linked());
        let rows=session.run(&Command::with_params(QUERY,json!({}))).unwrap();
        assert_eq!(rows["images"].as_array().unwrap().len(),1);
        assert!(rows["texts"].as_array().unwrap().is_empty());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(!session.document.resources[0].is_linked());
        session.run(&Command::new("edit.redo")).unwrap();
        assert!(session.document.resources[0].is_linked());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
