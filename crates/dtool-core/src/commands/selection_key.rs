use crate::command::Session;
use crate::object::ObjectId;
use dtool_protocol::{Command, CommandError};

pub const KEY_SET: &str = "selection.key.set";
pub const KEY_QUERY: &str = "selection.key.query";

fn key_arg(params: &serde_json::Value) -> Result<ObjectId, CommandError> {
    let obj = params.as_object().ok_or_else(CommandError::invalid_params)?;
    if obj.keys().any(|k| k != "id") {
        return Err(CommandError::invalid_params());
    }
    obj.get("id").and_then(|v| v.as_u64()).map(ObjectId).ok_or_else(CommandError::invalid_params)
}

fn reply(session: &Session) -> serde_json::Value {
    serde_json::json!({
        "key": session.selection.key().map(|id| id.0),
        "explicit_key": session.selection.explicit_key().map(|id| id.0),
        "ids": session.selection.ids.iter().map(|id| id.0).collect::<Vec<u64>>(),
    })
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    (command.name == KEY_SET).then(|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        session.selection.set_key(key_arg(&command.params)?).then_some(()).ok_or_else(CommandError::invalid_params)
    })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        KEY_QUERY => Some(true),
        KEY_SET => Some(session.has_document && key_arg(&command.params).is_ok_and(|id| session.selection.ids.contains(&id))),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<serde_json::Value, CommandError>> {
    match command.name.as_str() {
        KEY_QUERY => Some(Ok(reply(session))),
        KEY_SET => Some((|| {
            let old = session.selection.clone();
            apply(session, command).expect("key selection command")?;
            if session.selection != old {
                session.undo.push(crate::undo::Change {
                    name: KEY_SET.to_string(),
                    redo: command.clone(),
                    undo: Command {
                        name: "selection.set".to_string(),
                        params: serde_json::json!({
                            "ids": old.ids.iter().map(|id| id.0).collect::<Vec<_>>(),
                            "key_id": old.explicit_key().map(|id| id.0),
                            "op": crate::selection::SelectOp::Replace,
                        }),
                    },
                });
                session.generation += 1;
            }
            let mut out = reply(session);
            out["doc_version"] = serde_json::json!(session.generation);
            out["changes"] = serde_json::json!([{ "name": KEY_SET }]);
            Ok(out)
        })()),
        _ => None,
    }
}
