use crate::command::Session;
use crate::history_store::{DocumentSnapshot, HistoryStore};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::object::{Object, ObjectId, ObjectKind};
use std::collections::HashMap;

pub const HISTORY_LIST: &str = "history.list";
pub const HISTORY_LABELS_QUERY: &str = "history.labels.query";
pub const HISTORY_JUMP: &str = "history.jump";
pub const HISTORY_RECOVER: &str = "history.recover";
pub const HISTORY_UNDO: &str = "history.undo";
pub const HISTORY_REDO: &str = "history.redo";
pub const HISTORY_SNAPSHOT_CREATE: &str = "history.snapshot.create";
pub const HISTORY_SNAPSHOT_RESTORE: &str = "history.snapshot.restore";
pub const HISTORY_SNAPSHOT_LIST: &str = "history.snapshot.list";
pub const HISTORY_SNAPSHOT_LAYER_FROM: &str = "history.snapshot.layer_from";
pub const HISTORY_BUDGET_QUERY: &str = "history.budget.query";
pub const HISTORY_BUDGET_SET: &str = "history.budget.set";
pub const HISTORY_SAVE_WITH_DOC_SET: &str = "history.save_with_doc.set";
pub const HISTORY_SAVE_WITH_DOC_QUERY: &str = "history.save_with_doc.query";

pub const MAX_HISTORY_STEPS: u32 = 100_000;
const MAX_SNAPSHOTS: usize = 256;
const MAX_SNAPSHOT_NAME: usize = 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JumpArgs { index: u64 }

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SnapshotCreateArgs { #[serde(default)] name: Option<String> }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotIdArgs { id: u64 }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetArgs { max_steps: u32 }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnabledArgs { enabled: bool }

fn is_history(name: &str) -> bool {
    matches!(name, HISTORY_LIST | HISTORY_LABELS_QUERY | HISTORY_JUMP | HISTORY_RECOVER | HISTORY_UNDO | HISTORY_REDO | HISTORY_SNAPSHOT_CREATE | HISTORY_SNAPSHOT_RESTORE | HISTORY_SNAPSHOT_LAYER_FROM | HISTORY_SNAPSHOT_LIST | HISTORY_BUDGET_QUERY | HISTORY_BUDGET_SET | HISTORY_SAVE_WITH_DOC_SET | HISTORY_SAVE_WITH_DOC_QUERY)
}

fn params(command: &Command) -> Value {
    if command.params.is_null() { json!({}) } else { command.params.clone() }
}

fn parse<T: for<'de> Deserialize<'de>>(command: &Command) -> Result<T, CommandError> {
    serde_json::from_value(params(command)).map_err(|_| CommandError::invalid_params())
}

fn no_params(command: &Command) -> Result<(), CommandError> {
    crate::command::check_no_params(&params(command))
}

fn entries(session: &Session) -> Value {
    let store = HistoryStore::from_undo(&session.undo);
    Value::Array(store.list().iter().map(|e| json!({"index": e.index, "label_key": e.label_key, "label": e.label})).collect())
}

fn budget(session: &Session) -> Value {
    let budget = HistoryStore::from_undo(&session.undo).budget();
    json!({"max_steps": budget.max_steps, "memory_estimate_bytes": budget.memory_estimate_bytes, "reachable_depth": budget.reachable_depth})
}

fn cursor(session: &Session) -> Value {
    match session.undo.applied_steps() {
        0 => Value::Null,
        n => json!(n as u64 - 1),
    }
}

fn snapshot_list(session: &Session) -> Value {
    Value::Array(session.history.snapshots.iter().map(|s| json!({"id": s.id, "name": s.name, "history_index": s.history_index})).collect())
}

fn layer_from_snapshot(session: &mut Session, id: u64) -> Result<Value, CommandError> {
    let snapshot = session.history.snapshots.iter().find(|s| s.id == id).cloned().ok_or_else(CommandError::not_found)?;
    if snapshot.document.objects.is_empty() { return Err(CommandError::validation("HISTORY.SNAPSHOT_EMPTY")); }
    let max_id = session.document.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
    let first = session.document.next_object_id.max(max_id.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    let count = u64::try_from(snapshot.document.objects.len()).map_err(|_| CommandError::invalid_params())?;
    let next = first.checked_add(count).and_then(|n| n.checked_add(1)).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let group = ObjectId(first);
    let remap: HashMap<ObjectId, ObjectId> = snapshot.document.objects.iter().enumerate().map(|(i, o)| (o.id, ObjectId(first + 1 + i as u64))).collect();
    let before = serde_json::to_value(&session.document).map_err(|_| CommandError::invalid_params())?;
    let mut document = session.document.clone();
    document.objects.push(Object { id: group, name: snapshot.name.clone(), kind: ObjectKind::Group, parent_id: None, ..Object::default() });
    for object in snapshot.document.objects {
        let mut copy = object;
        copy.id = remap[&copy.id];
        copy.parent_id = Some(copy.parent_id.and_then(|p| remap.get(&p).copied()).unwrap_or(group));
        document.objects.push(copy);
    }
    document.next_object_id = next;
    document.version = version;
    document.keep_high_water_from(&session.document);
    let after = serde_json::to_value(&document).map_err(|_| CommandError::invalid_params())?;
    session.document = document;
    session.undo.push(crate::undo::Change {
        name: HISTORY_SNAPSHOT_LAYER_FROM.to_string(),
        undo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({"document": before})),
        redo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({"document": after})),
    });
    session.generation += 1;
    Ok(json!({"id": id, "group_id": group.0, "object_count": count, "doc_version": session.generation, "changes": [{"name": HISTORY_SNAPSHOT_LAYER_FROM}]}))
}

fn preflight(session: &Session, command: &Command) -> Result<(), CommandError> {
    match command.name.as_str() {
        HISTORY_JUMP => {
            let args: JumpArgs = parse(command)?;
            if args.index >= session.undo.total_steps() as u64 { return Err(CommandError::invalid_params()); }
        }
        HISTORY_SNAPSHOT_CREATE => {
            let args: SnapshotCreateArgs = parse(command)?;
            if args.name.as_ref().is_some_and(|n| n.chars().count() > MAX_SNAPSHOT_NAME) { return Err(CommandError::invalid_params()); }
            if session.history.snapshots.len() >= MAX_SNAPSHOTS { return Err(CommandError::validation("HISTORY.SNAPSHOT_LIMIT")); }
        }
        HISTORY_SNAPSHOT_RESTORE | HISTORY_SNAPSHOT_LAYER_FROM => {
            let args: SnapshotIdArgs = parse(command)?;
            if !session.history.snapshots.iter().any(|s| s.id == args.id) { return Err(CommandError::not_found()); }
        }
        HISTORY_BUDGET_SET => {
            let args: BudgetArgs = parse(command)?;
            if args.max_steps == 0 || args.max_steps > MAX_HISTORY_STEPS { return Err(CommandError::invalid_params()); }
        }
        HISTORY_SAVE_WITH_DOC_SET => { parse::<EnabledArgs>(command)?; }
        _ => no_params(command)?,
    }
    Ok(())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !is_history(&command.name) { return None; }
    if !session.has_document || preflight(session, command).is_err() { return Some(false); }
    Some(match command.name.as_str() {
        HISTORY_UNDO => session.undo.can_undo(),
        HISTORY_REDO => session.undo.can_redo(),
        HISTORY_RECOVER => recover_available(session),
        _ => true,
    })
}

fn step(session: &mut Session, name: &str) -> Result<(), CommandError> {
    session.run(&Command::new(name)).map(|_| ())
}

fn jump(session: &mut Session, index: u64) -> Result<(), CommandError> {
    let target = index as usize + 1;
    let origin = session.undo.applied_steps();
    if origin != target {
        session.history.recover = Some(crate::history_store::RecoverPoint {
            applied_steps: origin,
            after_jump_steps: target,
            total_steps: session.undo.total_steps(),
            document: session.document.clone(),
        });
    }
    while session.undo.applied_steps() > target {
        step(session, "edit.undo")?;
    }
    while session.undo.applied_steps() < target && session.undo.can_redo() {
        step(session, "edit.redo")?;
    }
    if session.undo.applied_steps() != target {
        return Err(CommandError::invalid_params());
    }
    if let Some(snapshot) = session.history.snapshots.iter().rev().find(|s| s.history_index == index).cloned() {
        let mut document = snapshot.document;
        document.keep_high_water_from(&session.document);
        document.version = session.document.version.max(document.version);
        session.document = document;
    }
    Ok(())
}

fn recover_available(session: &Session) -> bool {
    match &session.history.recover {
        Some(point) => session.undo.applied_steps() == point.after_jump_steps && session.undo.total_steps() == point.total_steps,
        None => false,
    }
}

pub fn recover(session: &mut Session) -> Result<(), CommandError> {
    if !recover_available(session) {
        return Err(CommandError::not_found());
    }
    let point = session.history.recover.take().ok_or_else(CommandError::not_found)?;
    let target = point.applied_steps;
    while session.undo.applied_steps() > target {
        step(session, "edit.undo")?;
    }
    while session.undo.applied_steps() < target && session.undo.can_redo() {
        step(session, "edit.redo")?;
    }
    let mut expected = point.document;
    expected.keep_high_water_from(&session.document);
    let objects_match = serde_json::to_value(&session.document.objects).ok() == serde_json::to_value(&expected.objects).ok();
    let stories_match = session.document.stories == expected.stories;
    if session.undo.applied_steps() != target || !objects_match || !stories_match {
        expected.version = session.document.version.max(expected.version);
        session.document = expected;
    } else if session.document.next_object_id < expected.next_object_id {
        session.document.keep_high_water_from(&expected);
    }
    Ok(())
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !is_history(&command.name) { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        preflight(session, command)?;
        match command.name.as_str() {
            HISTORY_LIST => Ok(json!({"entries": entries(session), "cursor": cursor(session), "can_undo": session.undo.can_undo(), "can_redo": session.undo.can_redo()})),
            HISTORY_LABELS_QUERY => Ok(json!({"labels": entries(session)})),
            HISTORY_JUMP => {
                let args: JumpArgs = parse(command)?;
                jump(session, args.index)?;
                Ok(json!({"cursor": cursor(session), "doc_version": session.generation, "changes": [{"name": HISTORY_JUMP}]}))
            }
            HISTORY_RECOVER => {
                recover(session)?;
                Ok(json!({"cursor": cursor(session), "doc_version": session.generation, "changes": [{"name": HISTORY_RECOVER}]}))
            }
            HISTORY_UNDO | HISTORY_REDO => {
                let (name, available) = if command.name == HISTORY_UNDO { ("edit.undo", session.undo.can_undo()) } else { ("edit.redo", session.undo.can_redo()) };
                if !available { return Ok(json!({"doc_version": session.generation, "changes": []})); }
                session.run(&Command::new(name))
            }
            HISTORY_SNAPSHOT_CREATE => {
                let args: SnapshotCreateArgs = parse(command)?;
                session.history.next_snapshot_id += 1;
                let id = session.history.next_snapshot_id;
                let name = args.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Snapshot {id}"));
                let history_index = session.undo.applied_steps().saturating_sub(1) as u64;
                session.history.snapshots.push(DocumentSnapshot { id, name: name.clone(), history_index, document: session.document.clone() });
                Ok(json!({"id": id, "name": name, "history_index": history_index}))
            }
            HISTORY_SNAPSHOT_RESTORE => {
                let args: SnapshotIdArgs = parse(command)?;
                let snapshot = session.history.snapshots.iter().find(|s| s.id == args.id).cloned().ok_or_else(CommandError::not_found)?;
                let before = serde_json::to_value(&session.document).map_err(|_| CommandError::invalid_params())?;
                let after = serde_json::to_value(&snapshot.document).map_err(|_| CommandError::invalid_params())?;
                let mut document = snapshot.document;
                document.version = session.document.version + 1;
                document.keep_high_water_from(&session.document);
                session.document = document;
                session.undo.push(crate::undo::Change {
                    name: HISTORY_SNAPSHOT_RESTORE.to_string(),
                    undo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({"document": before})),
                    redo: Command::with_params(crate::commands::feature_restore::FEATURE_RESTORE, json!({"document": after})),
                });
                session.generation += 1;
                Ok(json!({"id": args.id, "doc_version": session.generation, "changes": [{"name": HISTORY_SNAPSHOT_RESTORE}]}))
            }
            HISTORY_SNAPSHOT_LAYER_FROM => {
                let args: SnapshotIdArgs = parse(command)?;
                layer_from_snapshot(session, args.id)
            }
            HISTORY_SNAPSHOT_LIST => Ok(json!({"snapshots": snapshot_list(session)})),
            HISTORY_BUDGET_QUERY => Ok(budget(session)),
            HISTORY_BUDGET_SET => {
                let args: BudgetArgs = parse(command)?;
                session.undo.set_limit(args.max_steps as usize);
                Ok(budget(session))
            }
            HISTORY_SAVE_WITH_DOC_SET => {
                let args: EnabledArgs = parse(command)?;
                session.history.save_with_document = args.enabled;
                Ok(json!({"enabled": args.enabled}))
            }
            HISTORY_SAVE_WITH_DOC_QUERY => Ok(json!({"enabled": session.history.save_with_document})),
            _ => Err(CommandError::invalid_params()),
        }
    })())
}

pub fn apply(_session: &mut Session, _command: &Command) -> Option<Result<(), CommandError>> {
    None
}
