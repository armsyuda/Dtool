use crate::command::Session;
use dtool_protocol::{Command, CommandError};

pub const RULER_TOGGLE: &str = "ruler.toggle";
pub const GUIDES_TOGGLE: &str = "guides.toggle";
pub const SNAP_TOGGLE: &str = "snap.toggle";
pub const TEXT_ALIGN: &str = "text.align";
pub const TEXT_ALIGN_VALUES: [(&str, &str); 7] = [
    ("left", "Left"),
    ("centre", "Center"),
    ("right", "Right"),
    ("justify_left", "JustifyLeft"),
    ("justify_centre", "JustifyCenter"),
    ("justify_right", "JustifyRight"),
    ("justify_all", "JustifyAll"),
];

fn empty(params: &serde_json::Value) -> bool {
    params.is_null() || params.as_object().is_some_and(|o| o.is_empty())
}

fn text_align_plan(session: &Session, params: &serde_json::Value) -> Result<Vec<Command>, CommandError> {
    let obj = params.as_object().ok_or_else(CommandError::invalid_params)?;
    if obj.keys().any(|k| k != "align" && k != "ids") {
        return Err(CommandError::invalid_params());
    }
    let align = obj.get("align").and_then(|v| v.as_str()).ok_or_else(CommandError::invalid_params)?;
    let value = TEXT_ALIGN_VALUES.iter().find(|(k, _)| *k == align).map(|(_, v)| *v).ok_or_else(CommandError::invalid_params)?;
    if !obj.contains_key("ids") && session.text_edit.is_some() {
        let (story, range) = session.active_para_range().ok_or_else(CommandError::invalid_params)?;
        return Ok(vec![Command::with_params("text.para.set",
            serde_json::json!({ "story": story.0, "range": range, "patch": { "align": value } }))]);
    }
    let ids: Vec<crate::object::ObjectId> = match obj.get("ids") {
        Some(list) => serde_json::from_value::<Vec<u64>>(list.clone()).map_err(|_| CommandError::invalid_params())?.into_iter().map(crate::object::ObjectId).collect(),
        None => session.selection.ids.clone(),
    };
    let mut stories = Vec::new();
    for id in ids {
        let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
        if let Some(story) = object.story_id() {
            if !stories.contains(&story) {
                stories.push(story);
            }
        }
    }
    if stories.is_empty() {
        return Err(CommandError::invalid_params());
    }
    Ok(stories
        .into_iter()
        .map(|story| Command {
            name: "text.para.set".to_string(),
            params: serde_json::json!({ "story": story.0, "range": "all", "patch": { "align": value } }),
        })
        .collect())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        RULER_TOGGLE | GUIDES_TOGGLE | SNAP_TOGGLE => Some(session.has_document && empty(&command.params)),
        TEXT_ALIGN => Some(session.has_document && text_align_plan(session, &command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    let name = command.name.as_str();
    if ![RULER_TOGGLE, GUIDES_TOGGLE, SNAP_TOGGLE, TEXT_ALIGN].contains(&name) {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        if name != TEXT_ALIGN && !empty(&command.params) {
            return Err(CommandError::invalid_params());
        }
        let mut result = match name {
            RULER_TOGGLE => {
                let show = !session.document.rulers.show;
                let mut r = session.run_named(crate::rulers::RULER_SHOW, serde_json::json!({ "show": show }))?;
                r["show"] = serde_json::json!(show);
                r
            }
            GUIDES_TOGGLE => {
                let show = !session.guides.show;
                session.run_named("guides.show", serde_json::json!({ "show": show }))?
            }
            SNAP_TOGGLE => {
                let enabled = !session.snap_prefs.query().enabled;
                let mut r = session.run_named("snap.prefs.set", serde_json::json!({ "enabled": enabled }))?;
                r["enabled"] = serde_json::json!(enabled);
                r
            }
            _ => {
                let commands = text_align_plan(session, &command.params)?;
                let mut r = session.run_named("edit.batch", serde_json::json!({ "commands": commands }))?;
                if !r.is_object() {
                    r = serde_json::json!({});
                }
                r
            }
        };
        if let Some(obj) = result.as_object_mut() {
            obj.insert("changes".to_string(), serde_json::json!([{ "name": name }]));
        }
        Ok(result)
    })())
}
