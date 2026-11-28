use crate::command::Session;
use crate::document::{Document,Story};
use crate::frame::{frame_story,FrameChain};
use crate::object::{Object,ObjectId,ObjectKind};
use crate::table::model::StoryId;
use crate::text_cmd::{optional_u64,param_u64,next_story_id,param_text,conflict_error,set_size_range,apply_art_undo_add,push_change,bump,DEFAULT_SIZE_PT};
use dtool_protocol::{Command,CommandError,CommandResult};
use serde_json::{json,Value};

pub fn handles(command:&Command)->bool {matches!(command.name.as_str(),"text.frame.add"|"text.shape_text.add"|"text.frame.set"|"text.frame.undo_add"|"text.shape_text.undo_add"|"text.link"|"text.unlink"|"text.link.undo"|CONVERT_TO_FRAME|CONVERT_TO_ART|CONVERT_UNDO|FILLER|FILLER_MENU|IMPORT|IMPORT_UPDATE|IMPORT_UNDO|IMPORT_LINKED|LINKS_QUERY)}
pub const FILLER:&str="text.filler";
pub const FILLER_MENU:&str="text.insert.insert_filler_text";
pub fn filler_target(session:&Session,params:&Value)->Option<ObjectId> {
    let frame=|id:ObjectId|matches!(session.document.get_object(id).map(|object|&object.kind),Some(ObjectKind::FrameText{..})).then_some(id);
    match params.get("id").and_then(Value::as_u64) {
        Some(id)=>frame(ObjectId(id)),
        None=>session.text_edit.as_ref().and_then(|edit|edit.object).and_then(frame).or_else(||session.selection.ids.iter().copied().find_map(frame)),
    }
}
fn filler_source(params:&Value)->String {
    match params.get("text").and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty()=>text.to_string(),
        _=>{
            let prefs=crate::prefs::Prefs::load(&crate::prefs::Prefs::default_path());
            if prefs.filler_text.trim().is_empty() {crate::prefs::Prefs::factory_filler()} else {prefs.filler_text}
        }
    }
}
fn run_filler(cmd:&Command,session:&mut Session)->CommandResult {
    let id=filler_target(session,&cmd.params).ok_or_else(CommandError::invalid_params)?;
    let story=frame_story(&session.document,id).ok_or_else(CommandError::not_found)?;
    let fill=crate::text_cmd::fit_filler(&session.document,story,&filler_source(&cmd.params));
    let undo=flow_snapshot(&session.document,&[story],&[],&[])?;
    let redo=Command::with_params(FILLER,json!({"story":story.0,"fill":fill}));
    apply_filler(&redo,&mut session.document)?;
    if session.text_edit.as_ref().is_some_and(|edit|edit.story==story) {session.text_edit=None;}
    push_change(session,FILLER,redo,undo);
    Ok(json!({"doc_version":session.generation,"id":id.0,"story":story.0,"length":fill.chars().count()}))
}
fn apply_filler(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let story=StoryId(param_u64(&cmd.params,"story")?);
    let fill=cmd.params.get("fill").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?.to_string();
    let target=doc.stories.iter_mut().find(|s|s.id==story).ok_or_else(CommandError::not_found)?;
    crate::text_cmd::fill_story(target,fill);
    Ok(())
}
pub const IMPORT:&str="text.import";
pub const IMPORT_UPDATE:&str="text.import.update";
pub const IMPORT_UNDO:&str="text.import.undo";
pub const IMPORT_LINKED:&str="file.placement_policy.import_text_files_as_linked";
pub const LINKS_QUERY:&str="text.links.query";
fn json_of<T:serde::Serialize>(value:&T)->Result<Value,CommandError> {serde_json::to_value(value).map_err(|_|CommandError::invalid_params())}
fn import_snapshot(doc:&Document,story:StoryId,drop:Option<ObjectId>)->Result<Command,CommandError> {
    let before=doc.stories.iter().find(|s|s.id==story);
    Ok(Command::with_params(IMPORT_UNDO,json!({"story":story.0,"before":json_of(&before)?,"drop":drop.map(|id|id.0),"links":json_of(&doc.text_links)?})))
}
fn import_update_ready(session:&Session,params:&Value)->bool {
    params.get("story").and_then(Value::as_u64).is_some_and(|story|crate::text_link::text_link(&session.document,StoryId(story)).is_some())
}
fn finish_import(session:&mut Session,name:&str,redo:Command,undo:Command,story:StoryId)->Result<(),CommandError> {
    apply_import(&redo,&mut session.document)?;
    if session.text_edit.as_ref().is_some_and(|edit|edit.story==story) {session.text_edit=None;}
    push_change(session,name,redo,undo);
    Ok(())
}
fn run_import(cmd:&Command,session:&mut Session)->CommandResult {
    let path=cmd.params.get("path").and_then(Value::as_str).map(str::trim).filter(|path|!path.is_empty()).map(std::path::PathBuf::from).ok_or_else(CommandError::invalid_params)?;
    let imported=crate::text_import::import_file(&path)?;
    let text=imported.text.clone();
    let linked=cmd.params.get("linked").and_then(Value::as_bool).unwrap_or(session.document.text_import_linked);
    let target=filler_target(session,&cmd.params);
    if target.is_none() && cmd.params.get("id").is_some_and(|id|!id.is_null()) {return Err(CommandError::invalid_params());}
    let (id,story,create)=match target {
        Some(id)=>(id,frame_story(&session.document,id).ok_or_else(CommandError::not_found)?,Value::Null),
        None=>{
            let id=ObjectId(session.document.next_object_id);
            let [width,height]=session.document.pages.first().map(|page|page.size).ok_or_else(CommandError::invalid_params)?;
            let rect=cmd.params.get("rect").cloned().unwrap_or_else(||json!([width*0.1,height*0.1,width*0.8,height*0.8]));
            (id,StoryId(next_story_id(&session.document)),json!({"id":id.0,"rect":rect}))
        }
    };
    let undo=import_snapshot(&session.document,story,(!create.is_null()).then_some(id))?;
    let link=linked.then(||crate::text_link::link_for(story,&path,&text));
    let length=text.chars().count();
    let redo=Command::with_params(IMPORT,json!({"story":story.0,"text":text,"create":create,"link":json_of(&link)?,"runs":json_of(&imported.runs)?}));
    finish_import(session,IMPORT,redo,undo,story)?;
    Ok(json!({"doc_version":session.generation,"id":id.0,"story":story.0,"linked":linked,"length":length}))
}
fn run_import_update(cmd:&Command,session:&mut Session)->CommandResult {
    let story=StoryId(param_u64(&cmd.params,"story")?);
    let link=crate::text_link::text_link(&session.document,story).cloned().ok_or_else(CommandError::not_found)?;
    let imported=crate::text_import::import_file(&link.path)?;
    let text=imported.text.clone();
    let undo=import_snapshot(&session.document,story,None)?;
    let length=text.chars().count();
    let fresh=crate::text_link::link_for(story,&link.path,&text);
    let redo=Command::with_params(IMPORT,json!({"story":story.0,"text":text,"create":Value::Null,"link":json_of(&fresh)?,"runs":json_of(&imported.runs)?}));
    finish_import(session,IMPORT_UPDATE,redo,undo,story)?;
    Ok(json!({"doc_version":session.generation,"story":story.0,"length":length}))
}
fn apply_import(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let story=StoryId(param_u64(&cmd.params,"story")?);
    let text=cmd.params.get("text").and_then(Value::as_str).ok_or_else(CommandError::invalid_params)?.to_string();
    let link:Option<crate::text_link::TextLink>=serde_json::from_value(cmd.params.get("link").cloned().unwrap_or(Value::Null)).map_err(|_|CommandError::invalid_params())?;
    if let Some(create)=cmd.params.get("create").filter(|create|!create.is_null()) {
        let add=Command::with_params("text.frame.add",json!({"id":create.get("id").cloned().unwrap_or(Value::Null),"story":story.0,"rect":create.get("rect").cloned().unwrap_or(Value::Null)}));
        apply_frame_add(&add,doc)?;
    }
    let target=doc.stories.iter_mut().find(|s|s.id==story).ok_or_else(CommandError::not_found)?;
    let runs:Vec<crate::text_import::ImportedRun>=serde_json::from_value(cmd.params.get("runs").cloned().unwrap_or_else(||json!([]))).map_err(|_|CommandError::invalid_params())?;
    crate::text_cmd::fill_story(target,text.clone());
    if !runs.is_empty() {
        let base=target.ranges.first().cloned().unwrap_or_else(||json!({}));
        target.ranges=crate::text_import::styled_ranges(&base,&crate::text_import::ImportedText{text,runs});
    }
    doc.text_links.retain(|existing|existing.story!=story);
    doc.text_links.extend(link);
    Ok(())
}
fn apply_import_undo(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let story=StoryId(param_u64(&cmd.params,"story")?);
    let links:Vec<crate::text_link::TextLink>=serde_json::from_value(cmd.params.get("links").cloned().unwrap_or_else(||json!([]))).map_err(|_|CommandError::invalid_params())?;
    let before:Option<Story>=serde_json::from_value(cmd.params.get("before").cloned().unwrap_or(Value::Null)).map_err(|_|CommandError::invalid_params())?;
    match (cmd.params.get("drop").and_then(Value::as_u64),before) {
        (Some(id),_)=>apply_art_undo_add(&Command::with_params("text.frame.undo_add",json!({"id":id,"story":story.0})),doc)?,
        (None,Some(before))=>match doc.stories.iter_mut().find(|s|s.id==story) {Some(slot)=>*slot=before,None=>doc.stories.push(before)},
        (None,None)=>return Err(CommandError::invalid_params()),
    }
    doc.text_links=links;
    Ok(())
}
fn run_import_linked(cmd:&Command,session:&mut Session)->CommandResult {
    let value=cmd.params.get("value").and_then(Value::as_bool).unwrap_or(!session.document.text_import_linked);
    let undo=Command::with_params(IMPORT_LINKED,json!({"value":session.document.text_import_linked}));
    let redo=Command::with_params(IMPORT_LINKED,json!({"value":value}));
    apply_import_linked(&redo,&mut session.document)?;
    push_change(session,IMPORT_LINKED,redo,undo);
    Ok(json!({"doc_version":session.generation,"value":value}))
}
fn apply_import_linked(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    doc.text_import_linked=cmd.params.get("value").and_then(Value::as_bool).ok_or_else(CommandError::invalid_params)?;
    Ok(())
}
fn query_links(session:&Session)->CommandResult {
    Ok(json!({"links":json_of(&crate::text_link::text_link_rows(&session.document))?,"linked":session.document.text_import_linked}))
}
pub const CONVERT_TO_FRAME:&str="layer.convert.to_text_frame";
pub const CONVERT_TO_ART:&str="layer.convert.to_art_text";
pub const CONVERT_UNDO:&str="layer.convert.undo";
pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    if !handles(command) {return None;}
    Some(session.has_document && match command.name.as_str() {
        CONVERT_TO_FRAME=>!convert_ids(session,command,true).is_empty(),
        CONVERT_TO_ART=>!convert_ids(session,command,false).is_empty(),
        FILLER|FILLER_MENU=>filler_target(session,&command.params).is_some(),
        IMPORT_UPDATE=>import_update_ready(session,&command.params),
        _=>true,
    })
}
pub fn convertible(doc:&Document,id:ObjectId,to_frame:bool)->bool {
    doc.get_object(id).is_some_and(|object|match object.kind {
        ObjectKind::ArtText{..}=>to_frame,
        ObjectKind::FrameText{..}=>!to_frame && crate::frame::chain_of(doc,id).is_none(),
        _=>false,
    })
}
fn listed_ids(params:&Value)->Option<Vec<ObjectId>> {
    params.get("ids").and_then(Value::as_array).map(|list|list.iter().filter_map(Value::as_u64).map(ObjectId).collect())
}
pub fn convert_ids(session:&Session,command:&Command,to_frame:bool)->Vec<ObjectId> {
    let ids=listed_ids(&command.params).unwrap_or_else(||session.selection.ids.clone());
    ids.into_iter().filter(|id|convertible(&session.document,*id,to_frame)).collect()
}
fn run_convert(cmd:&Command,session:&mut Session,to_frame:bool)->CommandResult {
    let ids=convert_ids(session,cmd,to_frame);
    if ids.is_empty() {return Err(CommandError::invalid_params());}
    let before:Vec<Object>=ids.iter().filter_map(|id|session.document.get_object(*id).cloned()).collect();
    let redo=Command::with_params(cmd.name.as_str(),json!({"ids":ids.iter().map(|id|id.0).collect::<Vec<u64>>()}));
    let mut candidate=session.document.clone();apply_convert(&redo,&mut candidate,to_frame)?;session.document=candidate;
    let undo=Command::with_params(CONVERT_UNDO,json!({"objects":before}));
    push_change(session,cmd.name.as_str(),redo,undo);
    Ok(json!({"doc_version":session.generation,"ids":ids.iter().map(|id|id.0).collect::<Vec<u64>>()}))
}
fn apply_convert(cmd:&Command,doc:&mut Document,to_frame:bool)->Result<(),CommandError> {
    let ids=listed_ids(&cmd.params).ok_or_else(CommandError::invalid_params)?;
    if ids.is_empty() || !ids.iter().all(|id|convertible(doc,*id,to_frame)) {return Err(CommandError::invalid_params());}
    for id in ids {
        let story=doc.get_object(id).and_then(|object|object.story_id()).ok_or_else(CommandError::not_found)?;
        let size=if to_frame {None} else {
            let source=doc.stories.iter().find(|s|s.id==story).ok_or_else(CommandError::not_found)?;
            let size = crate::text_cmd::size_pt_of(source, DEFAULT_SIZE_PT) * crate::units::units_per_point(doc.units, doc.dpi);
            Some(crate::text_cmd::measure_art(&source.text, size))
        };
        let object=doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        match size {
            None=>{object.kind=ObjectKind::FrameText{story};object.frame=Some(crate::FrameProps::from_rect(object.size));}
            Some(size)=>{object.kind=ObjectKind::ArtText{story};object.frame=None;object.size=size;}
        }
        if !to_frame {
            crate::text_cmd::refresh_art_width(doc, id);
        }
    }
    Ok(())
}
fn apply_convert_undo(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let objects:Vec<Object>=serde_json::from_value(cmd.params.get("objects").cloned().unwrap_or(Value::Null)).map_err(|_|CommandError::invalid_params())?;
    if !objects.iter().all(|object|doc.get_object(object.id).is_some()) {return Err(CommandError::not_found());}
    for object in objects {
        if let Some(target)=doc.find_object_mut(object.id.0) {*target=object;}
    }
    Ok(())
}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    if !handles(command) {return None;}
    Some(if !session.has_document {Err(CommandError::no_document())} else {match command.name.as_str() {
        "text.frame.add"=>run_frame_add(command,session),"text.shape_text.add"=>run_shape_text_add(command,session),"text.frame.set"=>run_frame_set(command,session),
        "text.link"=>run_link(command,session),"text.unlink"=>run_unlink(command,session),
        CONVERT_TO_FRAME=>run_convert(command,session,true),CONVERT_TO_ART=>run_convert(command,session,false),
        FILLER|FILLER_MENU=>run_filler(command,session),
        IMPORT=>run_import(command,session),IMPORT_UPDATE=>run_import_update(command,session),IMPORT_LINKED=>run_import_linked(command,session),LINKS_QUERY=>query_links(session),
        _=>apply_doc(&mut session.document,command).unwrap_or_else(||Err(CommandError::invalid_params())).map(|_| {bump(session);json!({"doc_version":session.generation})}),
    }})
}
pub fn apply_doc(doc:&mut Document,command:&Command)->Option<Result<(),CommandError>> {
    Some(match command.name.as_str() {
        "text.frame.add"=>apply_frame_add(command,doc),"text.shape_text.add"=>apply_shape_text_add(command,doc),"text.frame.set"=>apply_frame_set(command,doc),
        "text.frame.undo_add"=>apply_frame_undo_add(command,doc),"text.shape_text.undo_add"=>apply_shape_text_undo_add(command,doc),
        "text.link"=>apply_link(command,doc),"text.unlink"=>apply_unlink(command,doc),"text.link.undo"=>apply_link_undo(command,doc),
        CONVERT_TO_FRAME=>apply_convert(command,doc,true),CONVERT_TO_ART=>apply_convert(command,doc,false),CONVERT_UNDO=>apply_convert_undo(command,doc),FILLER=>apply_filler(command,doc),
        IMPORT=>apply_import(command,doc),IMPORT_UNDO=>apply_import_undo(command,doc),IMPORT_LINKED=>apply_import_linked(command,doc),_=>return None,
    })
}
pub fn apply(session:&mut Session,command:&Command)->Option<Result<(),CommandError>> {
    let result=apply_doc(&mut session.document,command)?;
    if result.is_ok() && session.text_edit.as_ref().is_some_and(|edit|!session.document.stories.iter().any(|story|story.id==edit.story)) {session.text_edit=None;}
    Some(result)
}

fn param_rect(params: &Value) -> Result<([f32; 2], [f32; 2]), CommandError> {
    let arr = params
        .get("rect")
        .and_then(|v| v.as_array())
        .ok_or_else(CommandError::invalid_params)?;
    if arr.len() != 4 {
        return Err(CommandError::invalid_params());
    }
    let vals: Result<Vec<f32>, CommandError> = arr
        .iter()
        .map(|v| {
            let n = v.as_f64().ok_or_else(CommandError::invalid_params)? as f32;
            if !n.is_finite() {
                return Err(CommandError::invalid_params());
            }
            Ok(n)
        })
        .collect();
    let vals = vals?;
    let x0 = vals[0].min(vals[0] + vals[2]);
    let y0 = vals[1].min(vals[1] + vals[3]);
    let w = vals[2].abs();
    let h = vals[3].abs();
    if w <= 0.0 || h <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    Ok(([x0, y0], [w, h]))
}

fn shape_path_for(object:&Object)->Vec<[f32;2]> {
    let path=match &object.kind {
        ObjectKind::Curve(curve)=>curve.contour.to_kurbo(),
        _ if object.shape.as_ref().is_some_and(|recipe|recipe.kind==crate::shape::ShapeKind::Ellipse)=>crate::geometry::ellipse_to_bez_path(f64::from(object.size[0])*0.5,f64::from(object.size[1])*0.5,f64::from(object.size[0])*0.5,f64::from(object.size[1])*0.5),
        _=>return crate::frame::rect_path([0.0,0.0],object.size),
    };
    let mut points=Vec::new();
    kurbo::flatten(path.elements().iter().copied(),0.25,|element|match element {
        kurbo::PathEl::MoveTo(point)|kurbo::PathEl::LineTo(point)=>points.push([point.x as f32,point.y as f32]),_=>{},
    });points
}

fn is_closed_convertible(object: &Object) -> bool {
    if object.locked || !object.pickable {return false;}
    match &object.kind {
        ObjectKind::Shape => object.shape.as_ref().is_none_or(|recipe|matches!(recipe.kind,crate::shape::ShapeKind::Rectangle|crate::shape::ShapeKind::Ellipse)),
        ObjectKind::Curve(curve) => curve.contour.closed && curve.contour.points.len() >= 2 && curve.holes.is_empty(),
        _ => false,
    }
}

fn start_edit(session: &mut Session, story: StoryId, object: ObjectId, caret: usize) {
    session.text_edit = Some(crate::command::TextEditState {
        story,
        object: Some(object),
        caret,
        anchor: caret,
        before: None,
    });
}

fn run_frame_add(cmd: &Command, session: &mut Session) -> CommandResult {
    let mut redo = cmd.clone();
    if optional_u64(&redo.params, "id")?.is_none() {
        redo.params["id"] = session.document.next_object_id.into();
    }
    if optional_u64(&redo.params, "story")?.is_none() {
        redo.params["story"] = next_story_id(&session.document).into();
    }
    crate::text_cmd::remember_story_defaults(&mut redo, &session.document);
    apply_frame_add(&redo, &mut session.document)?;
    let id = param_u64(&redo.params, "id")?;
    let story = param_u64(&redo.params, "story")?;
    let mut undo = Command::new("text.frame.undo_add");
    undo.params["id"] = id.into();
    undo.params["story"] = story.into();
    push_change(session, "text.frame.add", redo, undo);
    start_edit(session, StoryId(story), ObjectId(id), 0);
    Ok(json!({
        "doc_version": session.generation,
        "id": id,
        "story": story
    }))
}

fn apply_frame_add(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let (pos, size) = param_rect(&cmd.params)?;
    let text = param_text(&cmd.params);
    let object_id = ObjectId(param_u64(&cmd.params, "id")?);
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    if doc.stories.iter().any(|s| s.id == story_id) || doc.get_object(object_id).is_some() {
        return Err(conflict_error());
    }
    let mut story = Story {
        id: story_id,
        text,
        ranges: Vec::new(),
        specials: Vec::new(),
        paras: Vec::new(),
    };
    crate::text_cmd::style_new_story(&mut story, &cmd.params, doc)?;
    let frame = crate::FrameProps::from_rect(size);
    let mut object = Object {
        id: object_id,
        pos,
        size,
        kind: ObjectKind::FrameText { story: story_id },
        frame: Some(frame),
        name: if story.text.is_empty() {
            dtool_strings::text("text.frame.default_name").to_owned()
        } else {
            story.text.clone()
        },
        ..Object::default()
    };
    let center = [pos[0] + size[0] * 0.5, pos[1] + size[1] * 0.5];
    object.effects = super::defaults::effects_for_creation(doc, size, &cmd.params)?;
    crate::artboard::edit::assign_new_object(doc, &mut object, center);
    doc.stories.push(story);
    let _ = doc.create_object(object);
    Ok(())
}

fn apply_frame_undo_add(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    apply_art_undo_add(cmd, doc)
}

fn run_shape_text_add(cmd: &Command, session: &mut Session) -> CommandResult {
    let shape = ObjectId(param_u64(&cmd.params, "shape")?);
    let object = session
        .document
        .get_object(shape)
        .cloned()
        .ok_or_else(CommandError::not_found)?;
    if !is_closed_convertible(&object) {
        return Err(CommandError::invalid_params());
    }
    let mut redo = cmd.clone();
    if optional_u64(&redo.params, "id")?.is_none() {
        redo.params["id"] = shape.0.into();
    }
    if optional_u64(&redo.params, "story")?.is_none() {
        redo.params["story"] = next_story_id(&session.document).into();
    }
    crate::text_cmd::remember_story_defaults(&mut redo, &session.document);
    apply_shape_text_add(&redo, &mut session.document)?;
    let id = param_u64(&redo.params, "id")?;
    let story = param_u64(&redo.params, "story")?;
    let mut undo = Command::new("text.shape_text.undo_add");
    undo.params["object"] = serde_json::to_value(&object).unwrap_or(Value::Null);
    undo.params["story"] = story.into();
    push_change(session, "text.shape_text.add", redo, undo);
    start_edit(session, StoryId(story), ObjectId(id), 0);
    Ok(json!({
        "doc_version": session.generation,
        "id": id,
        "story": story
    }))
}

fn apply_shape_text_add(cmd: &Command, doc: &mut Document) -> Result<(), CommandError> {
    let shape = ObjectId(param_u64(&cmd.params, "shape")?);
    let object_id = ObjectId(optional_u64(&cmd.params, "id")?.unwrap_or(shape.0));
    let story_id = StoryId(param_u64(&cmd.params, "story")?);
    if doc.stories.iter().any(|s| s.id == story_id) {
        return Err(conflict_error());
    }
    let text = param_text(&cmd.params);
    if object_id!=shape {return Err(CommandError::invalid_params());}
    let object = doc.get_object(shape).ok_or_else(CommandError::not_found)?;
    if !is_closed_convertible(object) {
        return Err(CommandError::invalid_params());
    }
    let pos = object.pos;
    let size = object.size;
    let path = shape_path_for(object);
    let frame = crate::FrameProps::following_shape(size, shape, Some(path));
    let name = object.name.clone();
    let mut story = Story {
        id: story_id,
        text,
        ranges: Vec::new(),
        specials: Vec::new(),
        paras: Vec::new(),
    };
    crate::text_cmd::style_new_story(&mut story, &cmd.params, doc)?;
    let object_mut = doc.find_object_mut(shape.0).ok_or_else(CommandError::not_found)?;
    object_mut.kind = ObjectKind::FrameText { story: story_id };
    object_mut.frame = Some(frame);
    object_mut.pos = pos;
    object_mut.size = size;
    if object_mut.name.is_empty() {
        object_mut.name = name;
    }
    doc.stories.push(story);
    Ok(())
}

fn apply_shape_text_undo_add(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let object:Object=serde_json::from_value(cmd.params.get("object").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
    let story=optional_u64(&cmd.params,"story")?;
    let existing=doc.find_object_mut(object.id.0).ok_or_else(CommandError::not_found)?;*existing=object;
    if let Some(story)=story {crate::frame::drop_story_if_unowned(doc,StoryId(story));}Ok(())
}

fn run_frame_set(cmd:&Command,session:&mut Session)->CommandResult {
    let id=ObjectId(param_u64(&cmd.params,"id")?);
    let object=session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if !matches!(object.kind, ObjectKind::FrameText { .. }) {return Err(CommandError::invalid_params());}
    let frame=object.frame_props().ok_or_else(CommandError::invalid_params)?;
    let mut undo_params=serde_json::to_value(&frame).map_err(|_|CommandError::invalid_params())?;undo_params["id"]=id.0.into();
    let undo=Command::with_params("text.frame.set",undo_params);
    let mut candidate=session.document.clone();apply_frame_set(cmd,&mut candidate)?;
    session.document=candidate;push_change(session,"text.frame.set",cmd.clone(),undo);Ok(json!({"id":id.0,"doc_version":session.generation}))
}
fn apply_frame_set(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let id=ObjectId(param_u64(&cmd.params,"id")?);
    let object=doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    if !matches!(object.kind, ObjectKind::FrameText { .. }) {return Err(CommandError::invalid_params());}
    let current = object.frame_props().ok_or_else(CommandError::invalid_params)?;
    let mut value=serde_json::to_value(&current).map_err(|_|CommandError::invalid_params())?;
    for (key,entry) in cmd.params.as_object().ok_or_else(CommandError::invalid_params)? {if key!="id" {value[key.as_str()]=entry.clone();}}
    let next:crate::FrameProps=serde_json::from_value(value).map_err(|_|CommandError::invalid_params())?;
    if next.size.iter().any(|v|!v.is_finite()||*v<=0.0) || next.columns==0 || !next.gutter.is_finite() || next.gutter<0.0 || next.insets.iter().any(|v|!v.is_finite()||*v<0.0) || !next.max_para_space.is_finite() || next.max_para_space<0.0 || next.column_widths.as_ref().is_some_and(|v|v.len()!=next.columns as usize || v.iter().any(|w|!w.is_finite()||*w<=0.0)) || next.shape_path.as_ref().is_some_and(|p|p.len()<3 || p.iter().flatten().any(|v|!v.is_finite())) {return Err(CommandError::invalid_params());}
    if next.size != current.size {
        let size = std::array::from_fn(|axis| {
            (next.size[axis] * object.text_scale[axis]).copysign(object.size[axis])
        });
        if size.iter().any(|value| !value.is_finite() || *value == 0.0) {
            return Err(CommandError::invalid_params());
        }
        object.size = size;
    }
    object.frame = Some(next);
    Ok(())
}

fn flow_snapshot(doc:&Document,stories:&[StoryId],objects:&[ObjectId],drop:&[StoryId])->Result<Command,CommandError> {
    let kept:Vec<&Story>=doc.stories.iter().filter(|story|stories.contains(&story.id)).collect();
    let kinds:Vec<Value>=objects.iter().filter_map(|id|frame_story(doc,*id).map(|story|json!([id.0,story.0]))).collect();
    let drop:Vec<u64>=drop.iter().map(|story|story.0).collect();
    let order: Vec<_> = doc.stories.iter().map(|story| story.id).collect();
    let marks = crate::longdoc::marks::StoryMarks::capture(doc);
    Ok(Command::with_params("text.link.undo",json!({"chains":serde_json::to_value(&doc.frame_chains).map_err(|_|CommandError::invalid_params())?,"stories":serde_json::to_value(kept).map_err(|_|CommandError::invalid_params())?,"kinds":kinds,"drop":drop,"order":order,"marks":marks})))
}

fn shift_range(value:&Value,offset:usize)->Value {
    let mut out=value.clone();
    for key in ["start","end"] {if let Some(n)=value.get(key).and_then(Value::as_u64) {out[key]=(n+offset as u64).into();}}
    out
}

fn run_link(cmd:&Command,session:&mut Session)->CommandResult {
    let from=ObjectId(param_u64(&cmd.params,"from")?);let to=ObjectId(param_u64(&cmd.params,"to")?);
    let doc=&session.document;
    let from_story=frame_story(doc,from).ok_or_else(CommandError::invalid_params)?;let to_story=frame_story(doc,to).ok_or_else(CommandError::invalid_params)?;
    let undo=flow_snapshot(doc,&[from_story,to_story],&[to],&[])?;
    let mut candidate=session.document.clone();apply_link(cmd,&mut candidate)?;session.document=candidate;
    if session.text_edit.as_ref().is_some_and(|edit|edit.object==Some(to)) {session.text_edit=None;}
    push_change(session,"text.link",cmd.clone(),undo);
    Ok(json!({"doc_version":session.generation,"story":from_story.0,"frames":crate::frame::flow_frames(&session.document,from_story).iter().map(|id|id.0).collect::<Vec<u64>>()}))
}

pub(super) fn apply_link(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let from=ObjectId(param_u64(&cmd.params,"from")?);let to=ObjectId(param_u64(&cmd.params,"to")?);
    let story=frame_story(doc,from).ok_or_else(CommandError::not_found)?;let old=frame_story(doc,to).ok_or_else(CommandError::not_found)?;
    if from==to || old==story || crate::frame::chain_of(doc,to).is_some() {return Err(CommandError::invalid_params());}
    if crate::frame::story_owners(doc,old).iter().any(|owner|*owner!=crate::frame::StoryOwner::Object(to)) {return Err(CommandError::invalid_params());}
    let positions = |id| doc.stories.iter().find(|item| item.id == id)
        .map(|item| (doc.story_pos(id, 0), item.text.chars().count()))
        .ok_or_else(CommandError::not_found);
    let source = positions(old)?;
    let target = positions(story)?;
    let index=match doc.frame_chains.iter().position(|chain|chain.story==story) {
        Some(index)=>index,
        None=>{doc.frame_chains.push(FrameChain{story,frames:vec![from]});doc.frame_chains.len()-1}
    };
    let at=doc.frame_chains[index].position(from).ok_or_else(CommandError::invalid_params)?;
    doc.frame_chains[index].frames.insert(at+1,to);
    if let Some(moved)=doc.stories.iter().position(|s|s.id==old).map(|i|doc.stories.remove(i)) {
        if !moved.text.is_empty() {
            let target=doc.stories.iter_mut().find(|s|s.id==story).ok_or_else(CommandError::not_found)?;
            let offset=target.text.chars().count();
            target.text.push_str(&moved.text);
            target.ranges.extend(moved.ranges.iter().map(|range|shift_range(range,offset)));
            target.paras.extend(moved.paras.iter().map(|range|shift_range(range,offset)));
            target.specials.extend(moved.specials.iter().map(|(pos,mark)|(pos+offset,mark.clone())));
        }
    }
    let object=doc.find_object_mut(to.0).ok_or_else(CommandError::not_found)?;object.kind=ObjectKind::FrameText{story};
    crate::longdoc::marks::merge_story_marks(doc, source, target);
    crate::longdoc::xref::refresh_xrefs(doc);
    Ok(())
}

fn run_unlink(cmd:&Command,session:&mut Session)->CommandResult {
    let id=ObjectId(param_u64(&cmd.params,"id")?);
    let chain=crate::frame::chain_of(&session.document,id).cloned().ok_or_else(CommandError::invalid_params)?;
    let detached=detached_frames(&chain,id);
    let mut redo=cmd.clone();
    if redo.params.get("stories").is_none() {
        let first=next_story_id(&session.document);
        redo.params["stories"]=json!((0..detached.len() as u64).map(|i|first+i).collect::<Vec<u64>>());
    }
    let fresh:Vec<StoryId>=story_list(&redo.params)?;
    if fresh.len()!=detached.len() {return Err(CommandError::invalid_params());}
    let undo=flow_snapshot(&session.document,&[chain.story],&detached,&fresh)?;
    let mut candidate=session.document.clone();apply_unlink(&redo,&mut candidate)?;session.document=candidate;
    if session.text_edit.as_ref().is_some_and(|edit|edit.object.is_some_and(|object|detached.contains(&object))) {session.text_edit=None;}
    push_change(session,"text.unlink",redo,undo);
    Ok(json!({"doc_version":session.generation,"story":chain.story.0,"detached":detached.iter().map(|id|id.0).collect::<Vec<u64>>()}))
}

fn detached_frames(chain:&FrameChain,id:ObjectId)->Vec<ObjectId> {
    let at=chain.position(id).unwrap_or(chain.frames.len()).max(1);
    chain.frames.iter().skip(at).copied().collect()
}

fn story_list(params:&Value)->Result<Vec<StoryId>,CommandError> {
    params.get("stories").and_then(Value::as_array).ok_or_else(CommandError::invalid_params)?.iter().map(|v|v.as_u64().map(StoryId).ok_or_else(CommandError::invalid_params)).collect()
}

fn apply_unlink(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let id=ObjectId(param_u64(&cmd.params,"id")?);
    let index=doc.frame_chains.iter().position(|chain|chain.frames.contains(&id)).ok_or_else(CommandError::invalid_params)?;
    let detached=detached_frames(&doc.frame_chains[index],id);
    let fresh=story_list(&cmd.params)?;
    if fresh.len()!=detached.len() || fresh.iter().any(|story|doc.stories.iter().any(|s|s.id==*story)) {return Err(conflict_error());}
    let size=doc.stories.iter().find(|s|s.id==doc.frame_chains[index].story).and_then(|s|s.ranges.first()).and_then(|r|r.get("size_pt")).and_then(Value::as_f64).map_or(DEFAULT_SIZE_PT,|v|v as f32);
    for (frame,story) in detached.iter().zip(fresh.iter()) {
        let mut empty=Story{id:*story,text:String::new(),ranges:Vec::new(),specials:Vec::new(),paras:Vec::new()};set_size_range(&mut empty,size);
        doc.stories.push(empty);
        let object=doc.find_object_mut(frame.0).ok_or_else(CommandError::not_found)?;object.kind=ObjectKind::FrameText{story:*story};
    }
    let keep=doc.frame_chains[index].frames.len()-detached.len();
    doc.frame_chains[index].frames.truncate(keep);
    if keep<2 {doc.frame_chains.remove(index);}
    crate::longdoc::xref::refresh_xrefs(doc);
    Ok(())
}

pub(super) fn unlink_inherited(doc: &mut Document, id: ObjectId) -> Result<(), CommandError> {
    let chain = crate::frame::chain_of(doc, id).ok_or_else(CommandError::invalid_params)?;
    let count = detached_frames(chain, id).len();
    let first = next_story_id(doc);
    let stories: Vec<_> = (first..first + count as u64).collect();
    apply_unlink(&Command::with_params("text.unlink", json!({ "id": id.0, "stories": stories })), doc)
}

fn apply_link_undo(cmd:&Command,doc:&mut Document)->Result<(),CommandError> {
    let chains:Vec<FrameChain>=serde_json::from_value(cmd.params.get("chains").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
    let stories:Vec<Story>=serde_json::from_value(cmd.params.get("stories").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
    let drop:Vec<u64>=serde_json::from_value(cmd.params.get("drop").cloned().unwrap_or_else(||json!([]))).map_err(|_|CommandError::invalid_params())?;
    let kinds:Vec<(u64,u64)>=serde_json::from_value(cmd.params.get("kinds").cloned().unwrap_or_else(||json!([]))).map_err(|_|CommandError::invalid_params())?;
    let order: Option<Vec<StoryId>> = cmd.params.get("order").cloned().map(serde_json::from_value).transpose().map_err(|_| CommandError::invalid_params())?;
    let marks: Option<crate::longdoc::marks::StoryMarks> = cmd.params.get("marks").filter(|value| !value.is_null())
        .cloned().map(serde_json::from_value).transpose().map_err(|_| CommandError::invalid_params())?;
    for (id,_) in &kinds {if doc.get_object(ObjectId(*id)).is_none() {return Err(CommandError::not_found());}}
    doc.frame_chains=chains;
    for story in stories {
        match doc.stories.iter_mut().find(|s|s.id==story.id) {Some(slot)=>*slot=story,None=>doc.stories.push(story)}
    }
    for (id,story) in kinds {if let Some(object)=doc.find_object_mut(id) {object.kind=ObjectKind::FrameText{story:StoryId(story)};}}
    for id in drop {crate::frame::drop_story_if_unowned(doc,StoryId(id));}
    if let Some(order) = order {
        let positions: std::collections::HashMap<_, _> = order.into_iter().enumerate().map(|(index, id)| (id, index)).collect();
        doc.stories.sort_by_key(|story| positions.get(&story.id).copied().unwrap_or(usize::MAX));
    }
    if let Some(marks) = marks {
        marks.restore(doc);
    }
    Ok(())
}

pub fn sync_frame_shape_path(doc: &mut Document, id: ObjectId) -> Result<(), CommandError> {
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let shape_id = match &object.kind {
        ObjectKind::FrameText { .. } => object.frame.as_ref().and_then(|frame| frame.shape),
        _ => return Ok(()),
    };
    let Some(shape_id) = shape_id else {
        return Ok(());
    };
    if shape_id == id {
        return Ok(());
    }
    let shape = doc.get_object(shape_id).ok_or_else(CommandError::not_found)?;
    let path = shape_path_for(shape);
    let size = shape.size;
    let pos = shape.pos;
    let object = doc.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
    if matches!(object.kind, ObjectKind::FrameText { .. }) {
        let mut frame = object.frame.clone().unwrap_or_else(|| crate::FrameProps::from_rect(object.size));
        frame.shape_path = Some(path);
        frame.size = size;
        object.size = size;
        object.pos = pos;
        object.frame = Some(frame);
    }
    Ok(())
}


#[cfg(test)]
mod frame_state_contracts {
    use super::*;
    #[test]
    fn frame_set_is_atomic_and_one_undo_restores_properties() {
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let reply=session.run(&Command::with_params("text.frame.add",json!({"rect":[0,0,100,50]}))).unwrap();let id=ObjectId(reply["id"].as_u64().unwrap());
        let before=serde_json::to_value(session.document.get_object(id).unwrap()).unwrap();
        session.run(&Command::with_params("text.frame.set",json!({"id":id.0,"columns":2,"gutter":5}))).unwrap();
        session.run(&Command::new("edit.undo")).unwrap();assert_eq!(serde_json::to_value(session.document.get_object(id).unwrap()).unwrap(),before);
        let generation=session.generation;
        assert!(session.run(&Command::with_params("text.frame.set",json!({"id":id.0,"columns":0}))).is_err());
        assert_eq!(session.generation,generation);assert_eq!(serde_json::to_value(session.document.get_object(id).unwrap()).unwrap(),before);
    }
    #[test]
    fn link_shares_story_and_unlink_keeps_text_in_earlier_frames() {
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let a=ObjectId(session.run(&Command::with_params("text.frame.add",json!({"rect":[0,0,100,20],"text":"one two"}))).unwrap()["id"].as_u64().unwrap());
        let b=ObjectId(session.run(&Command::with_params("text.frame.add",json!({"rect":[0,40,100,20],"text":" three"}))).unwrap()["id"].as_u64().unwrap());
        let c=ObjectId(session.run(&Command::with_params("text.frame.add",json!({"rect":[0,80,100,20]}))).unwrap()["id"].as_u64().unwrap());
        let before=serde_json::to_value(&session.document).unwrap();
        session.run(&Command::with_params("text.link",json!({"from":a.0,"to":b.0}))).unwrap();
        session.run(&Command::with_params("text.link",json!({"from":b.0,"to":c.0}))).unwrap();
        let story=frame_story(&session.document,a).unwrap();
        assert_eq!(session.document.frame_chains,vec![FrameChain{story,frames:vec![a,b,c]}]);
        assert_eq!(session.document.stories.iter().find(|s|s.id==story).unwrap().text,"one two three");
        assert_eq!(session.document.stories.len(),1);
        assert!(session.run(&Command::with_params("text.link",json!({"from":c.0,"to":a.0}))).is_err());
        session.run(&Command::with_params("text.unlink",json!({"id":b.0}))).unwrap();
        assert!(session.document.frame_chains.is_empty());
        assert_eq!(session.document.stories.iter().find(|s|s.id==story).unwrap().text,"one two three");
        assert_ne!(frame_story(&session.document,b),Some(story));assert_ne!(frame_story(&session.document,c),Some(story));
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(crate::frame::flow_frames(&session.document,story),vec![a,b,c]);
        session.run(&Command::new("edit.undo")).unwrap();session.run(&Command::new("edit.undo")).unwrap();
        let mut restored=session.document.stories.clone();restored.sort_by_key(|s|s.id.0);
        let mut original:Vec<Story>=serde_json::from_value(before["stories"].clone()).unwrap();original.sort_by_key(|s|s.id.0);
        assert_eq!(serde_json::to_value(restored).unwrap(),serde_json::to_value(original).unwrap());
        assert_eq!(serde_json::to_value(&session.document).unwrap()["objects"],before["objects"]);
        assert!(session.document.frame_chains.is_empty());
    }
    #[test]
    fn art_text_converts_to_frame_and_back_keeping_story_and_styles() {
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let reply=session.run(&Command::with_params("text.art.add",json!({"pos":[10,10],"size_pt":24,"text":"one\ntwo"}))).unwrap();
        let id=ObjectId(reply["id"].as_u64().unwrap());let story=StoryId(reply["story"].as_u64().unwrap());
        let original=serde_json::to_value(session.document.get_object(id).unwrap()).unwrap();
        let stories=serde_json::to_value(&session.document.stories).unwrap();
        assert!(!session.can_execute_named(CONVERT_TO_FRAME));
        session.selection.ids=vec![id];
        assert!(session.can_execute_named(CONVERT_TO_FRAME));assert!(!session.can_execute_named(CONVERT_TO_ART));
        session.run_named(CONVERT_TO_FRAME,json!({})).unwrap();
        let object=session.document.get_object(id).unwrap();
        assert!(matches!(object.kind,ObjectKind::FrameText{story:s} if s==story));assert!(object.frame.is_some());
        assert_eq!(serde_json::to_value(&session.document.stories).unwrap(),stories);
        session.run_named(CONVERT_TO_ART,json!({})).unwrap();
        let object=session.document.get_object(id).unwrap();
        assert!(matches!(object.kind,ObjectKind::ArtText{story:s} if s==story));assert!(object.frame.is_none());
        assert_eq!(serde_json::to_value(&session.document.stories).unwrap(),stories);
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(matches!(session.document.get_object(id).unwrap().kind,ObjectKind::FrameText{..}));
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(serde_json::to_value(session.document.get_object(id).unwrap()).unwrap(),original);
        session.run(&Command::new("edit.redo")).unwrap();
        assert!(matches!(session.document.get_object(id).unwrap().kind,ObjectKind::FrameText{..}));
    }
    #[test]
    fn filler_fills_selected_frame_in_one_undo_step() {
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let id=ObjectId(session.run(&Command::with_params("text.frame.add",json!({"rect":[0,0,200,100]}))).unwrap()["id"].as_u64().unwrap());
        let story=frame_story(&session.document,id).unwrap();
        session.text_edit=None;
        assert!(!session.can_execute_named(FILLER));
        session.selection.ids=vec![id];
        assert!(session.can_execute_named(FILLER));assert!(session.can_execute_named(FILLER_MENU));
        session.run(&Command::with_params(FILLER,json!({"text":"alpha beta gamma"}))).unwrap();
        let filled=session.document.stories.iter().find(|s|s.id==story).unwrap().clone();
        assert!(filled.text.starts_with("alpha beta gamma alpha"));
        assert_eq!(filled.ranges[0]["end"],json!(filled.text.chars().count()));
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.stories.iter().find(|s|s.id==story).unwrap().text.is_empty());
        session.run(&Command::new("edit.redo")).unwrap();
        assert_eq!(session.document.stories.iter().find(|s|s.id==story).unwrap().text,filled.text);
    }
    #[test]
    fn deleting_a_chained_frame_prunes_chain() {
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let ids:Vec<ObjectId>=(0..3).map(|i|ObjectId(session.run(&Command::with_params("text.frame.add",json!({"rect":[0,i*40,100,20]}))).unwrap()["id"].as_u64().unwrap())).collect();
        session.run(&Command::with_params("text.link",json!({"from":ids[0].0,"to":ids[1].0}))).unwrap();
        session.run(&Command::with_params("text.link",json!({"from":ids[1].0,"to":ids[2].0}))).unwrap();
        let story=frame_story(&session.document,ids[0]).unwrap();
        session.document.delete_object(ids[1]).unwrap();
        assert_eq!(session.document.frame_chains,vec![FrameChain{story,frames:vec![ids[0],ids[2]]}]);
        session.document.delete_object(ids[2]).unwrap();
        assert!(session.document.frame_chains.is_empty());
    }
    #[test]
    fn linked_text_import_updates_story_after_file_change() {
        let path=std::env::temp_dir().join(format!("dtool_b41_t019_{}.txt",std::process::id()));
        std::fs::write(&path,"first line\r\nsecond").unwrap();
        let mut session=Session::new(Document::new([400.0,200.0],crate::document::Units::Px,72.0,1));
        let result=session.run(&Command::with_params(IMPORT,json!({"path":path.to_string_lossy(),"linked":true}))).unwrap();
        let id=ObjectId(result["id"].as_u64().unwrap());
        let story=frame_story(&session.document,id).unwrap();
        let text=|session:&Session|session.document.stories.iter().find(|s|s.id==story).unwrap().text.clone();
        assert_eq!(text(&session),"first line\nsecond");
        assert_eq!(crate::text_link::text_link_rows(&session.document)[0].status,crate::resources::ResourceStatus::Linked);
        assert!(session.can_execute(&Command::with_params(IMPORT_UPDATE,json!({"story":story.0}))));
        std::fs::write(&path,"edited text").unwrap();
        assert_eq!(crate::text_link::modified_links(&session.document).len(),1);
        session.run(&Command::with_params(IMPORT_UPDATE,json!({"story":story.0}))).unwrap();
        assert_eq!(text(&session),"edited text");
        assert!(crate::text_link::modified_links(&session.document).is_empty());
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(text(&session),"first line\nsecond");
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(session.document.get_object(id).is_none());
        assert!(session.document.text_links.is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
