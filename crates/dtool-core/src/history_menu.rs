use crate::history_store::HistoryStore;
use dtool_protocol::{Command, CommandError, CommandResult};

pub fn can_execute(name: &str) -> bool {
    matches!(
        name,
        "history.list"
            | "history.jump"
            | "history.undo"
            | "history.redo"
            | "history.snapshot.create"
            | "history.snapshot.restore"
            | "history.snapshot.list"
            | "history.save_with_doc.set"
            | "history.save_with_doc.query"
            | "history.labels.query"
            | "history.budget.query"
            | "history.budget.set"
    )
}

pub fn run(
    name: &str,
    params: &serde_json::Value,
    store: &mut HistoryStore<'_>,
) -> Result<CommandResult, CommandError> {
    match name {
        "history.list" => {
            let entries = store.list();
            let json = serde_json::json!({
                "entries": entries.iter().map(|e| serde_json::json!({
                    "index": e.index,
                    "label_key": e.label_key,
                    "label": e.label
                })).collect::<Vec<_>>()
            });
            Ok(CommandResult::ok(json))
        }
        "history.jump" => {
            let index = params.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
            store.jump(index)?;
            Ok(CommandResult::ok(serde_json::json!({})))
        }
        "history.undo" => {
            if !store.can_undo() {
                return Err(CommandError::validation("error.history_no_undo"));
            }
            Ok(CommandResult::ok(serde_json::json!({})))
        }
        "history.redo" => {
            if !store.can_redo() {
                return Err(CommandError::validation("error.history_no_redo"));
            }
            Ok(CommandResult::ok(serde_json::json!({})))
        }
        "history.snapshot.create" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let snapshot = store.add_snapshot(name)?;
            Ok(CommandResult::ok(serde_json::json!({
                "id": snapshot.id,
                "name": snapshot.name
            })))
        }
        "history.snapshot.restore" => {
            let id = params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            store.restore_snapshot(id)?;
            Ok(CommandResult::ok(serde_json::json!({})))
        }
        "history.snapshot.list" => {
            Ok(CommandResult::ok(serde_json::json!({"snapshots": []})))
        }
        "history.save_with_doc.set" => {
            let enabled = params.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            store.set_save_history_with_document(enabled);
            Ok(CommandResult::ok(serde_json::json!({})))
        }
        "history.save_with_doc.query" => {
            Ok(CommandResult::ok(serde_json::json!({
                "enabled": store.save_history_with_document()
            })))
        }
        "history.labels.query" => {
            let entries = store.list();
            let labels: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
                "index": e.index,
                "label_key": e.label_key,
                "label": e.label
            })).collect();
            Ok(CommandResult::ok(serde_json::json!({"labels": labels})))
        }
        "history.budget.query" => {
            let budget = store.budget();
            Ok(CommandResult::ok(serde_json::json!({
                "max_steps": budget.max_steps,
                "memory_estimate_bytes": budget.memory_estimate_bytes,
                "reachable_depth": budget.reachable_depth
            })))
        }
        "history.budget.set" => {
            let max_steps = params.get("max_steps").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            store.set_max_steps(max_steps);
            Ok(CommandResult::ok(serde_json::json!({})))
        }
        _ => Err(CommandError::not_implemented("error.unknown_command")),
    }
}

pub fn apply(
    name: &str,
    params: &serde_json::Value,
    store: &mut HistoryStore<'_>,
) -> Result<(), CommandError> {
    run(name, params, store)?;
    Ok(())
}
