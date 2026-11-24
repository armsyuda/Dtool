use crate::command::Session;
use crate::document::{Story,StoryId};
use dtool_protocol::{Command,CommandError,CommandResult};
use serde_json::{json,Value};

pub const CASE:&str="text.case";
pub const RESTORE:&str="text.case.restore";
pub const CHANGE_OPS:[&str;5]=["upper","lower","title","sentence","toggle"];
pub const STYLE_OPS:[&str;5]=["none","all_caps","small_caps","all_small_caps","petite_caps"];

pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    match command.name.as_str() {
        CASE=>Some(session.has_document && parse(session,&command.params).is_ok()),
        RESTORE=>Some(false),
        _=>None,
    }
}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    if command.name==RESTORE {return Some(Err(CommandError::invalid_params()));}
    if command.name!=CASE {return None;}
    Some(execute(session,command))
}
pub fn apply(session:&mut Session,command:&Command)->Option<Result<(),CommandError>> {
    if command.name!=RESTORE {return None;}
    Some(restore(session,&command.params))
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum CaseMode {Change,Style}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct CaseRequest {pub story:StoryId,pub start:usize,pub end:usize,pub mode:CaseMode,pub op:String}

pub fn caps_value(op:&str)->Option<&'static str> {
    match op {
        "none"=>Some("None"),
        "all_caps"|"upper"=>Some("AllCaps"),
        "small_caps"=>Some("SmallCaps"),
        "all_small_caps"=>Some("AllSmallCaps"),
        "petite_caps"=>Some("PetiteCaps"),
        _=>None,
    }
}

fn parse(session:&Session,params:&Value)->Result<CaseRequest,CommandError> {
    let story=StoryId(params.get("story").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    let found=session.document.stories.iter().find(|s|s.id==story).ok_or_else(||CommandError::new("STORY_NOT_FOUND"))?;
    let len=found.text.chars().count();
    let (start,end)=match params.get("range") {
        Some(range)=>(
            range.get("start").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)? as usize,
            range.get("end").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)? as usize,
        ),
        None=>(0,len),
    };
    if start>end || end>len {return Err(CommandError::new("RANGE_OUT_OF_BOUNDS"));}
    let mode=match params.get("mode").and_then(|v|v.as_str()).unwrap_or("change") {
        "change"=>CaseMode::Change,
        "style"=>CaseMode::Style,
        _=>return Err(CommandError::invalid_params()),
    };
    let op=params.get("op").and_then(|v|v.as_str()).ok_or_else(CommandError::invalid_params)?.to_string();
    let valid=match mode {CaseMode::Change=>CHANGE_OPS.contains(&op.as_str()),CaseMode::Style=>caps_value(&op).is_some()};
    if !valid {return Err(CommandError::invalid_params());}
    Ok(CaseRequest{story,start,end,mode,op})
}

fn single_upper(c:char)->char {
    let mut it=c.to_uppercase();
    match (it.next(),it.next()) {(Some(u),None)=>u,_=>c}
}
fn single_lower(c:char)->char {
    let mut it=c.to_lowercase();
    match (it.next(),it.next()) {(Some(l),None)=>l,_=>c}
}

pub fn convert_case(text:&str,start:usize,end:usize,op:&str)->String {
    let chars:Vec<char>=text.chars().collect();
    let mut out=String::with_capacity(text.len());
    let mut word_start=true;
    let mut sentence_start=true;
    let mut after_stop=false;
    for (i,c) in chars.iter().copied().enumerate() {
        let inside=i>=start && i<end;
        let mapped=if !inside {c} else {
            match op {
                "upper"=>single_upper(c),
                "lower"=>single_lower(c),
                "title"=>if c.is_alphabetic() && word_start {single_upper(c)} else {single_lower(c)},
                "sentence"=>if c.is_alphabetic() && sentence_start {single_upper(c)} else {single_lower(c)},
                "toggle"=>if c.is_uppercase() {single_lower(c)} else if c.is_lowercase() {single_upper(c)} else {c},
                _=>c,
            }
        };
        out.push(mapped);
        if c.is_alphanumeric() {
            word_start=false;
            sentence_start=false;
            after_stop=false;
        } else if c.is_whitespace() {
            word_start=true;
            if after_stop || c=='\n' || c=='\u{2029}' {sentence_start=true;}
        } else if matches!(c,'.'|'!'|'?'|'\u{3002}') {
            after_stop=true;
        } else if matches!(c,'-'|'/'|'('|'['|'{'|'"'|'\u{201C}') {
            word_start=true;
        }
    }
    out
}

fn snapshot(session:&Session,id:StoryId)->Result<Value,CommandError> {
    let story=session.document.stories.iter().find(|s|s.id==id).ok_or_else(||CommandError::new("STORY_NOT_FOUND"))?;
    Ok(json!({"story":id.0,"state":story}))
}

fn refresh(session:&mut Session,id:StoryId)->Result<(),CommandError> {
    let probe=Command::with_params("text.style",json!({"story":id.0,"range":{"start":0,"end":0},"patch":Value::Null}));
    crate::text_cmd::apply(&probe,&mut session.document)
}

fn restore(session:&mut Session,params:&Value)->Result<(),CommandError> {
    let id=StoryId(params.get("story").and_then(|v|v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    let state:Story=serde_json::from_value(params.get("state").cloned().unwrap_or(Value::Null)).map_err(|_|CommandError::invalid_params())?;
    if state.id!=id {return Err(CommandError::invalid_params());}
    let version=session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let slot=session.document.stories.iter_mut().find(|s|s.id==id).ok_or_else(||CommandError::new("STORY_NOT_FOUND"))?;
    *slot=state;
    session.document.version=version;
    refresh(session,id)
}

fn execute(session:&mut Session,command:&Command)->CommandResult {
    if !session.has_document {return Err(CommandError::no_document());}
    let request=parse(session,&command.params)?;
    if request.mode==CaseMode::Style {
        let caps=caps_value(&request.op).ok_or_else(CommandError::invalid_params)?;
        let style=Command::with_params("text.style",json!({"story":request.story.0,"range":{"start":request.start,"end":request.end},"patch":{"caps":caps}}));
        return crate::text_cmd::run(&style,session);
    }
    let undo=Command::with_params(RESTORE,snapshot(session,request.story)?);
    let generation=session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version=session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let slot=session.document.stories.iter_mut().find(|s|s.id==request.story).ok_or_else(||CommandError::new("STORY_NOT_FOUND"))?;
    let changed=convert_case(&slot.text,request.start,request.end,&request.op);
    if changed==slot.text {return Ok(json!({"doc_version":session.generation,"changed":false}));}
    slot.text=changed;
    let redo=Command::with_params(RESTORE,snapshot(session,request.story)?);
    session.generation=generation;
    session.document.version=version;
    refresh(session,request.story)?;
    session.undo.push(crate::undo::Change{name:CASE.to_string(),undo,redo});
    Ok(json!({"doc_version":session.generation,"changed":true}))
}
