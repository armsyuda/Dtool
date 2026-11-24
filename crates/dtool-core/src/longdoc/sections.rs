use crate::document::Document;
use crate::CommandError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumberStyle {
    Arabic,
    RomanLower,
    RomanUpper,
    AlphaLower,
    AlphaUpper,
    Hangul,
    HanjaNumber,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub start_page: usize,
    pub name: String,
    pub restart_at: Option<u32>,
    pub prefix: String,
    pub style: NumberStyle,
    pub include_on_export: bool,
}

impl Document {
    pub fn sections(&self)->&[Section]{&self.sections}
    pub fn section_page_range(&self, index: usize) -> Option<std::ops::Range<usize>> {
        let start = self.sections.get(index)?.start_page.checked_sub(1)?;
        let end = match self.sections.get(index + 1) {
            Some(next) => next.start_page.checked_sub(1)?,
            None => self.pages.len(),
        };
        (start < end && end <= self.pages.len()).then_some(start..end)
    }
    pub fn set_section(&mut self,index:usize,section:Section)->Result<(),CommandError>{
        if index>self.sections.len() || section.start_page==0 || section.start_page>self.pages.len() || section.restart_at==Some(0){return Err(CommandError::invalid_params());}
        if index==0 && section.start_page!=1{return Err(CommandError::invalid_params());}
        if index>0 && section.start_page<=self.sections[index-1].start_page{return Err(CommandError::invalid_params());}
        if self.sections.get(index+1).is_some_and(|next|section.start_page>=next.start_page){return Err(CommandError::invalid_params());}
        if index==self.sections.len(){self.sections.push(section);}else{self.sections[index]=section;}Ok(())
    }
    pub fn delete_section(&mut self,index:usize)->Result<(),CommandError>{
        if index==0{return Err(CommandError::invalid_params());}if index>=self.sections.len(){return Err(CommandError::not_found());}self.sections.remove(index);Ok(())
    }
    pub fn list_sections(&self)->Vec<(usize,&Section)>{self.sections.iter().enumerate().collect()}
}

pub fn page_number(doc:&Document,page_index:usize)->(u32,String){
    let physical=page_index.saturating_add(1);let mut start=1usize;let mut value=1u32;let mut active=None;
    for section in doc.sections().iter().filter(|section|section.start_page<=physical){
        if section.start_page<start || section.start_page==0{continue;}
        value=value.saturating_add(section.start_page.saturating_sub(start).min(u32::MAX as usize) as u32);
        value=section.restart_at.unwrap_or(value);start=section.start_page;active=Some(section);
    }
    let number=value.saturating_add(physical.saturating_sub(start).min(u32::MAX as usize) as u32);
    let Some(section)=active else{return (number,number.to_string());};
    let formatted=format_number(number,&section.style);
    (number,format!("{}{}",section.prefix,formatted))
}

pub fn section_page_label(doc:&Document,page_index:usize)->String{page_number(doc,page_index).1}
fn alphabetic(mut n:u32,alphabet:&[char])->String{
    if n==0{return "0".into();}let mut result=String::new();while n>0{n-=1;result.insert(0,alphabet[n as usize%alphabet.len()]);n/=alphabet.len() as u32;}result
}
fn roman(mut n:u32)->String{
    if n==0 || n>3999{return n.to_string();}let mut result=String::new();
    for (value,symbol) in [(1000,"M"),(900,"CM"),(500,"D"),(400,"CD"),(100,"C"),(90,"XC"),(50,"L"),(40,"XL"),(10,"X"),(9,"IX"),(5,"V"),(4,"IV"),(1,"I")]{while n>=value{result.push_str(symbol);n-=value;}}result
}
fn hanja(n:u32)->String{
    if n==0{return "\u{96f6}".into();}
    let digits=['\u{96f6}','\u{4e00}','\u{4e8c}','\u{4e09}','\u{56db}','\u{4e94}','\u{516d}','\u{4e03}','\u{516b}','\u{4e5d}'];
    let mut result=String::new();
    for (base,large) in [(100_000_000,"\u{5104}"),(10_000,"\u{842c}"),(1,"")] {
        let group=(n/base)%10_000;if group==0{continue;}
        for (divisor,unit) in [(1000,"\u{5343}"),(100,"\u{767e}"),(10,"\u{5341}"),(1,"")]{
            let digit=(group/divisor)%10;if digit>0{if digit!=1||divisor==1{result.push(digits[digit as usize]);}result.push_str(unit);}
        }
        result.push_str(large);
    }
    result
}

pub const SECTION_RESTORE: &str = "section.restore";

fn section_index(command: &dtool_protocol::Command) -> Result<usize, CommandError> {
    command.params.get("index").and_then(serde_json::Value::as_u64).map(|v| v as usize).ok_or_else(CommandError::invalid_params)
}

fn section_record(session: &mut crate::command::Session, command: &dtool_protocol::Command, before: Vec<Section>) {
    super::xref::refresh_xrefs(&mut session.document);
    let undo = dtool_protocol::Command::with_params(SECTION_RESTORE, serde_json::json!({ "sections": before }));
    crate::text_cmd::push_change(session, &command.name, command.clone(), undo);
}

fn section_json(doc: &Document) -> serde_json::Value {
    serde_json::json!({
        "sections": doc.list_sections().into_iter().map(|(i, s)| serde_json::json!({ "index": i, "section": s })).collect::<Vec<_>>()
    })
}

pub fn can_execute(session: &crate::command::Session, command: &dtool_protocol::Command) -> Option<bool> {
    match command.name.as_str() {
        "section.set" | "section.list" | SECTION_RESTORE => Some(session.has_document),
        "section.delete" => Some(session.has_document && session.document.sections.len() > 1),
        _ => None,
    }
}

pub fn run(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Option<dtool_protocol::CommandResult> {
    let result = match command.name.as_str() {
        "section.set" => (|| {
            let index = section_index(command)?;
            let section: Section = serde_json::from_value(command.params.get("section").cloned().ok_or_else(CommandError::invalid_params)?)
                .map_err(|_| CommandError::invalid_params())?;
            let before = session.document.sections.clone();
            session.document.set_section(index, section)?;
            section_record(session, command, before);
            Ok(section_json(&session.document))
        })(),
        "section.delete" => (|| {
            let index = section_index(command)?;
            let before = session.document.sections.clone();
            session.document.delete_section(index)?;
            section_record(session, command, before);
            Ok(section_json(&session.document))
        })(),
        "section.list" => Ok(section_json(&session.document)),
        SECTION_RESTORE => (|| {
            let sections: Vec<Section> = serde_json::from_value(command.params.get("sections").cloned().ok_or_else(CommandError::invalid_params)?)
                .map_err(|_| CommandError::invalid_params())?;
            session.document.sections = sections;
            super::xref::refresh_xrefs(&mut session.document);
            crate::text_cmd::bump(session);
            Ok(section_json(&session.document))
        })(),
        _ => return None,
    };
    Some(result)
}

pub fn format_number(number:u32,style:&NumberStyle)->String{
    match style {
        NumberStyle::Arabic=>number.to_string(),
        NumberStyle::RomanLower=>roman(number).to_lowercase(),NumberStyle::RomanUpper=>roman(number),
        NumberStyle::AlphaLower=>alphabetic(number,&('a'..='z').collect::<Vec<_>>()),
        NumberStyle::AlphaUpper=>alphabetic(number,&('A'..='Z').collect::<Vec<_>>()),
        NumberStyle::Hangul=>alphabetic(number,&['\u{ac00}','\u{b098}','\u{b2e4}','\u{b77c}','\u{b9c8}','\u{bc14}','\u{c0ac}','\u{c544}','\u{c790}','\u{cc28}','\u{ce74}','\u{d0c0}','\u{d30c}','\u{d558}']),
        NumberStyle::HanjaNumber=>hanja(number),
    }
}
