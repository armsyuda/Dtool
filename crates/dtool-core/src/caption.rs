use serde::{Deserialize, Serialize};

use crate::{Document, Object, ObjectId};
use dtool_protocol::{CommandError, CommandResult};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptionSide {
    Below,
    Above,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Caption {
    pub owner: ObjectId,
    pub text_node: ObjectId,
    pub side: CaptionSide,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CounterType {
    Figure,
    Table,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionSettings {
    pub counter_type: CounterType,
    pub restart_numbering: bool,
    pub format: String,
    pub alignment: CaptionSide,
    pub h_offset: f64,
    pub v_offset: f64,
    pub paragraph_style: Option<String>,
}

impl Default for CaptionSettings {
    fn default() -> Self {
        Self {
            counter_type: CounterType::Figure,
            restart_numbering: false,
            format: "Figure {counter}".to_string(),
            alignment: CaptionSide::Below,
            h_offset: 0.0,
            v_offset: 0.0,
            paragraph_style: None,
        }
    }
}

fn links(doc: &Document) -> Result<Vec<(Caption, CaptionSettings)>, CommandError> {
    doc.feature_data.get("captions").cloned().map(serde_json::from_value).transpose()
        .map(|v| v.unwrap_or_default()).map_err(|_| CommandError::invalid_params())
}

fn save_links(doc: &mut Document, value: Vec<(Caption, CaptionSettings)>) -> Result<(), CommandError> {
    doc.feature_data.insert("captions".into(), serde_json::to_value(value).map_err(|_| CommandError::invalid_params())?);
    Ok(())
}

pub fn add_caption(doc: &mut Document, owner: ObjectId, settings: CaptionSettings) -> Result<Caption, CommandError> {
    let source = doc.get_object(owner).cloned().ok_or_else(CommandError::not_found)?;
    if !settings.h_offset.is_finite() || !settings.v_offset.is_finite() { return Err(CommandError::invalid_params()); }
    let mut captions = links(doc)?;
    if captions.iter().any(|(c, _)| c.owner == owner) { return Err(CommandError::new("CONFLICT")); }
    let count = if settings.restart_numbering { 1 } else { 1 + captions.iter().filter(|(_, s)| s.counter_type == settings.counter_type).count() };
    let text = settings.format.replace("{counter}", &count.to_string());
    let mut object = Object::default();
    object.name = source.name.clone();
    object.kind = crate::ObjectKind::Text(text);
    object.size = source.size;
    let text_node = doc.create_object(object);
    let caption = Caption { owner, text_node, side: settings.alignment.clone() };
    captions.push((caption.clone(), settings));
    save_links(doc, captions)?;
    update_caption_position(doc, owner)?;
    Ok(caption)
}

pub fn remove_caption(doc: &mut Document, owner: ObjectId) -> Result<(), CommandError> {
    let mut captions = links(doc)?;
    let index = captions.iter().position(|(c, _)| c.owner == owner).ok_or_else(CommandError::not_found)?;
    let (caption, _) = captions.remove(index);
    doc.delete_object(caption.text_node)?;
    save_links(doc, captions)
}

pub fn update_caption_position(doc: &mut Document, owner: ObjectId) -> Result<(), CommandError> {
    let (caption, settings) = links(doc)?.into_iter().find(|(c, _)| c.owner == owner).ok_or_else(CommandError::not_found)?;
    let source = doc.get_object(owner).cloned().ok_or_else(CommandError::not_found)?;
    let target = doc.find_object_mut(caption.text_node.0).ok_or_else(CommandError::not_found)?;
    let dx = settings.h_offset as f32;
    let dy = settings.v_offset as f32;
    target.pos = match caption.side {
        CaptionSide::Below => [source.pos[0] + dx, source.pos[1] + source.size[1] + dy],
        CaptionSide::Above => [source.pos[0] + dx, source.pos[1] - target.size[1] - dy],
        CaptionSide::Left => [source.pos[0] - target.size[0] - dx, source.pos[1] + dy],
        CaptionSide::Right => [source.pos[0] + source.size[0] + dx, source.pos[1] + dy],
    };
    Ok(())
}

pub fn run_caption_command(doc: &mut Document, command_name: &str, args: &serde_json::Value) -> Result<CommandResult, CommandError> {
    let owner = ObjectId(args.get("owner").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    match command_name {
        "caption.add" => {
            let mut settings: CaptionSettings = serde_json::from_value(args.get("settings").cloned().unwrap_or_else(|| serde_json::json!({}))).map_err(|_| CommandError::invalid_params())?;
            let object = doc.get_object(owner).ok_or_else(CommandError::not_found)?;
            if matches!(object.kind, crate::ObjectKind::Table(_)) && args.get("settings").is_none() {
                settings.alignment = CaptionSide::Above;
                settings.counter_type = CounterType::Table;
                settings.format = "Table {counter}".into();
            }
            if args.get("from_name").and_then(|v|v.as_bool()).unwrap_or(false) {
                if !matches!(object.fill, crate::paint::Paint::Bitmap(_)) { return Err(CommandError::invalid_params()); }
                let name = if !object.name.trim().is_empty() {object.name.as_str()} else {
                    match &object.fill {crate::paint::Paint::Bitmap(bitmap)=>bitmap.resource_id.as_str(),_=>return Err(CommandError::invalid_params())}
                };
                let file_name = std::path::Path::new(name).file_name().and_then(|s|s.to_str()).filter(|s|!s.is_empty()).ok_or_else(CommandError::invalid_params)?;
                settings.format = file_name.to_string();
            }
            let caption = add_caption(doc, owner, settings)?;
            Ok(Ok(serde_json::json!({"text_node":caption.text_node.0,"side":caption.side})))
        }
        "caption.remove" => { remove_caption(doc, owner)?; Ok(Ok(serde_json::json!({}))) }
        _ => Err(CommandError::not_implemented()),
    }
}
