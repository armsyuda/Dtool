use crate::document::Document;
use crate::object::{ObjectId,ObjectKind};
use crate::resources::ResourceStatus;
use crate::table::model::StoryId;
use dtool_protocol::CommandError;
use serde::{Deserialize,Serialize};
use std::path::{Path,PathBuf};

#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
pub struct TextLink {
    pub story:StoryId,
    pub path:PathBuf,
    #[serde(default)]
    pub mtime:Option<u64>,
    #[serde(default)]
    pub hash:String,
}

#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
pub struct TextLinkRow {
    pub story:StoryId,
    pub name:String,
    pub path:PathBuf,
    pub status:ResourceStatus,
    pub frames:Vec<ObjectId>,
}

pub fn decode_text(bytes:&[u8])->String {
    let text=if let Some(rest)=bytes.strip_prefix(&[0xEF,0xBB,0xBF]) {String::from_utf8_lossy(rest).into_owned()}
    else if let Some(rest)=bytes.strip_prefix(&[0xFF,0xFE]) {utf16(rest,u16::from_le_bytes)}
    else if let Some(rest)=bytes.strip_prefix(&[0xFE,0xFF]) {utf16(rest,u16::from_be_bytes)}
    else {String::from_utf8_lossy(bytes).into_owned()};
    text.replace("\r\n","\n").replace('\r',"\n")
}

fn utf16(bytes:&[u8],word:fn([u8;2])->u16)->String {
    let units:Vec<u16>=bytes.chunks_exact(2).map(|pair|word([pair[0],pair[1]])).collect();
    String::from_utf16_lossy(&units)
}

pub fn text_hash(text:&str)->String {
    use std::hash::{Hash,Hasher};
    let mut hasher=std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}",hasher.finish())
}

pub fn read_text_file(path:&Path)->Result<String,CommandError> {
    crate::text_import::import_file(path).map(|imported|imported.text)
}

pub fn link_for(story:StoryId,path:&Path,text:&str)->TextLink {
    TextLink{story,path:path.to_path_buf(),mtime:crate::resources::file_mtime(path),hash:text_hash(text)}
}

pub fn link_status(link:&TextLink)->ResourceStatus {
    match read_text_file(&link.path) {
        Err(_)=>ResourceStatus::Missing,
        Ok(text) if text_hash(&text)!=link.hash=>ResourceStatus::Modified,
        Ok(_)=>ResourceStatus::Linked,
    }
}

pub fn text_link(doc:&Document,story:StoryId)->Option<&TextLink> {
    doc.text_links.iter().find(|link|link.story==story)
}

pub fn text_link_rows(doc:&Document)->Vec<TextLinkRow> {
    doc.text_links.iter().filter(|link|doc.stories.iter().any(|story|story.id==link.story)).map(|link|TextLinkRow{
        story:link.story,
        name:link.path.file_name().map(|name|name.to_string_lossy().into_owned()).unwrap_or_else(||link.path.display().to_string()),
        path:link.path.clone(),
        status:link_status(link),
        frames:doc.objects.iter().filter(|object|matches!(object.kind,ObjectKind::FrameText{story} if story==link.story)).map(|object|object.id).collect(),
    }).collect()
}

pub fn modified_links(doc:&Document)->Vec<TextLinkRow> {
    text_link_rows(doc).into_iter().filter(|row|row.status==ResourceStatus::Modified).collect()
}
