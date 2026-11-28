use crate::blend::BlendMode;
use crate::command::Session;
use crate::layer_style::{default_effect_for, BlendRanges, EffectKind, LayerEffect};
use crate::object::{Object, ObjectKind};
use crate::ObjectId;
use dtool_protocol::{Command, CommandError, CommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard};

pub const BLEND_SET: &str = "layer.blend.set";
pub const BLEND_RANGES_SET: &str = "layer.blend_ranges.set";
pub const EFFECT_ADD: &str = "effect.add";
pub const EFFECT_UPDATE: &str = "effect.update";
pub const EFFECT_REMOVE: &str = "effect.remove";
pub const EFFECT_LIST: &str = "effect.list";
pub const EFFECT_PREVIEW: &str = "effect.preview";
pub const EFFECT_CANCEL: &str = "effect.cancel";
const RESTORE_STYLE: &str = "layer.style.history.restore";

pub const NAMES: [&str; 8] = [BLEND_SET, BLEND_RANGES_SET, EFFECT_ADD, EFFECT_UPDATE, EFFECT_REMOVE, EFFECT_LIST, EFFECT_PREVIEW, EFFECT_CANCEL];

#[derive(Debug, Clone, PartialEq)]
pub struct StagedEffect {
    pub object: ObjectId,
    pub index: Option<usize>,
    pub effect: LayerEffect,
}

type PreviewOwner = std::sync::Weak<()>;
static STAGED: Mutex<Vec<(PreviewOwner, StagedEffect)>> = Mutex::new(Vec::new());

fn staged_store() -> MutexGuard<'static, Vec<(PreviewOwner, StagedEffect)>> {
    let mut guard = STAGED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.retain(|(owner, _)| owner.strong_count() > 0);
    guard
}

fn session_key(session: &Session) -> PreviewOwner {
    std::sync::Arc::downgrade(&session.effect_preview_owner)
}

pub fn staged_effects(session: &Session, id: ObjectId) -> Vec<StagedEffect> {
    let key = session_key(session);
    staged_store().iter().filter(|(owner, item)| owner.ptr_eq(&key) && item.object == id).map(|(_, item)| item.clone()).collect()
}

pub fn staged(session: &Session, id: ObjectId) -> Option<StagedEffect> {
    staged_effects(session, id).into_iter().last()
}

pub fn snapshot_staged(session: &Session) -> Vec<StagedEffect> {
    let key = session_key(session);
    staged_store().iter().filter(|(owner, _)| owner.ptr_eq(&key)).map(|(_, item)| item.clone()).collect()
}

pub fn restore_staged(session: &Session, items: Vec<StagedEffect>) {
    let key = session_key(session);
    let mut store = staged_store();
    store.retain(|(owner, _)| !owner.ptr_eq(&key));
    store.extend(items.into_iter().map(|item| (key.clone(), item)));
}

fn clear_staged(session: &Session, id: ObjectId) -> bool {
    let key = session_key(session);
    let mut store = staged_store();
    let before = store.len();
    store.retain(|(owner, item)| !(owner.ptr_eq(&key) && item.object == id));
    store.len() != before
}

fn preview_set(params: &Value) -> Result<Option<Vec<LayerEffect>>, CommandError> {
    params.get("effects").map(|value| {
        let effects: Vec<LayerEffect> = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        effects.into_iter().map(checked).collect()
    }).transpose()
}

fn ours(name: &str) -> bool {
    NAMES.contains(&name)
}

fn object_id(params: &Value) -> Result<ObjectId, CommandError> {
    params.get("id").and_then(Value::as_u64).map(ObjectId).ok_or_else(CommandError::invalid_params)
}

fn index(params: &Value) -> Result<usize, CommandError> {
    params.get("index").and_then(Value::as_u64).map(|value| value as usize).ok_or_else(CommandError::invalid_params)
}

fn target<'a>(session: &'a Session, command: &Command) -> Result<&'a Object, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let id = object_id(&command.params)?;
    session.document.get_object(id).ok_or_else(CommandError::not_found)
}

fn writable<'a>(session: &'a Session, command: &Command) -> Result<&'a Object, CommandError> {
    let object = target(session, command)?;
    if session.document.is_object_locked(object) {
        return Err(CommandError::new("LOCKED"));
    }
    Ok(object)
}

fn parse_mode(object: &Object, params: &Value) -> Result<BlendMode, CommandError> {
    let mode: BlendMode = serde_json::from_value(params.get("mode").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())?;
    if mode == BlendMode::PassThrough && !matches!(object.kind, ObjectKind::Group) {
        return Err(CommandError::invalid_params());
    }
    Ok(mode)
}

fn parse_ranges(params: &Value) -> Result<Option<BlendRanges>, CommandError> {
    let value = params.get("ranges").ok_or_else(CommandError::invalid_params)?;
    let ranges: Option<BlendRanges> = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
    if ranges.as_ref().is_some_and(|ranges| !ranges.is_valid()) {
        return Err(CommandError::invalid_params());
    }
    Ok(ranges)
}

fn parse_kind(params: &Value) -> Result<EffectKind, CommandError> {
    serde_json::from_value(params.get("kind").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_| CommandError::invalid_params())
}

fn checked(effect: LayerEffect) -> Result<LayerEffect, CommandError> {
    if effect.is_valid() { Ok(effect) } else { Err(CommandError::invalid_params()) }
}

fn updated(current: &LayerEffect, params: &Value) -> Result<LayerEffect, CommandError> {
    let mut value = serde_json::to_value(current).map_err(|_| CommandError::invalid_params())?;
    let fields = params.as_object().ok_or_else(CommandError::invalid_params)?;
    let target = value.as_object_mut().ok_or_else(CommandError::invalid_params)?;
    for (key, field) in fields {
        match key.as_str() {
            "id" | "index" => {}
            "kind" => {
                if parse_kind(params)? != current.kind {
                    return Err(CommandError::invalid_params());
                }
            }
            "enabled" | "opacity" | "blend" | "params" | "band_pass" => {
                target.insert(key.clone(), field.clone());
            }
            _ => return Err(CommandError::invalid_params()),
        }
    }
    checked(serde_json::from_value(value).map_err(|_| CommandError::invalid_params())?)
}

fn preview_effect(object: &Object, params: &Value) -> Result<(Option<usize>, LayerEffect), CommandError> {
    let slot = match params.get("index") {
        None | Some(Value::Null) => None,
        Some(_) => Some(index(params)?),
    };
    let effect = match (params.get("effect"), slot) {
        (Some(value), _) => checked(serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?)?,
        (None, Some(slot)) => updated(object.effects.get(slot).ok_or_else(CommandError::invalid_params)?, &json!({}))?,
        (None, None) => default_effect_for(parse_kind(params)?, object.size),
    };
    if slot.is_some_and(|slot| slot >= object.effects.len()) {
        return Err(CommandError::invalid_params());
    }
    Ok((slot, effect))
}

#[derive(PartialEq, Serialize, Deserialize)]
struct LayerStyle {
    blend_mode: BlendMode,
    blend_ranges: Option<BlendRanges>,
    effects: Vec<LayerEffect>,
}

impl LayerStyle {
    fn from_object(object: &Object) -> Self {
        Self {
            blend_mode: object.blend_mode,
            blend_ranges: object.blend_ranges.clone(),
            effects: object.effects.clone(),
        }
    }

    fn install(self, object: &mut Object) {
        object.blend_mode = self.blend_mode;
        object.blend_ranges = self.blend_ranges;
        object.effects = self.effects;
    }
}

fn change_style(style: &mut LayerStyle, object: &Object, command: &Command) -> Result<Value, CommandError> {
    let params = &command.params;
    let extra = match command.name.as_str() {
        BLEND_SET => {
            style.blend_mode = parse_mode(object, params)?;
            json!({"id": object.id.0})
        }
        BLEND_RANGES_SET => {
            style.blend_ranges = parse_ranges(params)?;
            json!({"id": object.id.0})
        }
        EFFECT_ADD => {
            let effect = match params.get("effect") {
                Some(value) => checked(serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?)?,
                None => default_effect_for(parse_kind(params)?, object.size),
            };
            if params.get("kind").is_some() && parse_kind(params)? != effect.kind {
                return Err(CommandError::invalid_params());
            }
            style.effects.push(effect);
            json!({"id": object.id.0, "index": style.effects.len() - 1})
        }
        EFFECT_UPDATE => {
            let slot = index(params)?;
            let current = style.effects.get_mut(slot).ok_or_else(CommandError::invalid_params)?;
            *current = updated(current, params)?;
            json!({"id": object.id.0, "index": slot})
        }
        EFFECT_REMOVE => {
            let slot = index(params)?;
            if slot >= style.effects.len() {
                return Err(CommandError::invalid_params());
            }
            style.effects.remove(slot);
            json!({"id": object.id.0, "index": slot})
        }
        _ => return Err(CommandError::invalid_params()),
    };
    Ok(extra)
}

#[cfg(test)]
fn same_style(a: &Object, b: &Object) -> bool {
    a.blend_mode == b.blend_mode && a.blend_ranges == b.blend_ranges && a.effects == b.effects
}

fn plan(session: &Session, command: &Command) -> Result<(), CommandError> {
    match command.name.as_str() {
        EFFECT_LIST => target(session, command).map(|_| ()),
        EFFECT_CANCEL => target(session, command).map(|_| ()),
        EFFECT_PREVIEW => {
            let object = writable(session, command)?;
            match preview_set(&command.params)? { Some(_) => Ok(()), None => preview_effect(object, &command.params).map(|_| ()) }
        },
        _ => {
            let object = writable(session, command)?;
            change_style(&mut LayerStyle::from_object(object), object, command).map(|_| ())
        }
    }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    ours(&command.name).then(|| plan(session, command).is_ok())
}

fn commit(session: &mut Session, command: &Command) -> CommandResult {
    let object = writable(session, command)?;
    let id = object.id;
    let before = LayerStyle::from_object(object);
    let mut after = LayerStyle::from_object(object);
    let mut result = change_style(&mut after, object, command)?;
    if before == after {
        return Ok(json!({"id": id.0, "doc_version": session.generation, "changes": []}));
    }
    commit_style(session, &command.name, id, before, after)?;
    result["doc_version"] = json!(session.generation);
    result["changes"] = json!([{"name": command.name}]);
    Ok(result)
}

fn is_style_edit(command: &Command) -> bool {
    matches!(command.name.as_str(), BLEND_SET | BLEND_RANGES_SET | EFFECT_ADD | EFFECT_UPDATE | EFFECT_REMOVE)
}

fn install_style(session: &mut Session, id: ObjectId, style: LayerStyle) -> Result<(), CommandError> {
    let object = session.document.objects.iter_mut().find(|object| object.id == id)
        .ok_or_else(CommandError::not_found)?;
    style.install(object);
    clear_staged(session, id);
    Ok(())
}

fn commit_style(session: &mut Session, name: &str, id: ObjectId, before: LayerStyle, after: LayerStyle) -> Result<(), CommandError> {
    let restore = |style: &LayerStyle| Command::with_params(RESTORE_STYLE, json!({
        "id": id, "style": style, "selection": session.selection.ids,
    }));
    let undo = restore(&before);
    let redo = restore(&after);
    install_style(session, id, after)?;
    session.undo.push(crate::undo::Change { name: name.to_owned(), redo, undo });
    session.generation += 1;
    session.document.version += 1;
    Ok(())
}

/// Stage a batch of edits to one layer without cloning the document for rollback.
pub(crate) fn commit_batch(session: &mut Session, commands: &[Command]) -> Option<Result<(), CommandError>> {
    let first = commands.first()?;
    let id = object_id(&first.params).ok()?;
    if !commands.iter().all(|cmd| is_style_edit(cmd) && object_id(&cmd.params).ok() == Some(id)) {
        return None;
    }
    Some((|| {
        let object = writable(session, first)?;
        let before = LayerStyle::from_object(object);
        let mut after = LayerStyle::from_object(object);
        for command in commands {
            change_style(&mut after, object, command)?;
        }
        commit_style(session, "edit.batch", id, before, after)
    })())
}

fn restore_style(session: &mut Session, command: &Command) -> Result<(), CommandError> {
    #[derive(Deserialize)]
    struct Restore {
        id: ObjectId,
        style: LayerStyle,
        selection: Vec<ObjectId>,
    }
    let restore: Restore = serde_json::from_value(command.params.clone())
        .map_err(|_| CommandError::invalid_params())?;
    install_style(session, restore.id, restore.style)?;
    session.selection.ids = restore.selection.into_iter()
        .filter(|id| session.document.get_object(*id).is_some()).collect();
    session.document.version += 1;
    Ok(())
}

fn run_inner(session: &mut Session, command: &Command) -> CommandResult {
    if command.name == EFFECT_PREVIEW {
        if let Some(effects) = preview_set(&command.params)? {
            let object = writable(session, command)?;
            let id = object.id;
            let original = object.effects.clone();
            let mut items: Vec<_> = effects.iter().enumerate().map(|(index, effect)| StagedEffect { object: id, index: (index < original.len()).then_some(index), effect: effect.clone() }).collect();
            for (index, effect) in original.iter().enumerate().skip(effects.len()) {
                let mut effect = effect.clone(); effect.enabled = false;
                items.push(StagedEffect { object: id, index: Some(index), effect });
            }
            clear_staged(session, id);
            let key = session_key(session);
            staged_store().extend(items.into_iter().map(|item| (key.clone(), item)));
            return Ok(json!({"id": id.0, "effects": effects, "doc_version": session.generation, "changes": []}));
        }
    }
    match command.name.as_str() {
        EFFECT_LIST => {
            let object = target(session, command)?;
            Ok(json!({"id": object.id.0, "blend_mode": object.blend_mode, "blend_ranges": object.blend_ranges, "effects": object.effects, "doc_version": session.generation}))
        }
        EFFECT_PREVIEW => {
            let object = writable(session, command)?;
            let (slot, effect) = preview_effect(object, &command.params)?;
            let id = object.id;
            clear_staged(session, id);
            let item = StagedEffect { object: id, index: slot, effect };
            let result = json!({"id": id.0, "index": item.index, "effect": item.effect, "doc_version": session.generation, "changes": []});
            staged_store().push((session_key(session), item));
            Ok(result)
        }
        EFFECT_CANCEL => {
            let id = target(session, command)?.id;
            let cleared = clear_staged(session, id);
            Ok(json!({"id": id.0, "cancelled": cleared, "doc_version": session.generation, "changes": []}))
        }
        _ => commit(session, command),
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    ours(&command.name).then(|| run_inner(session, command))
}

/// Blend/effect changes as edit.batch steps: mutate the document only; the batch owns the single undo entry
/// (effects dialog OK = add + updates = one undo group). List/preview/cancel are not batch steps.
pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name == RESTORE_STYLE {
        return Some(restore_style(session, command));
    }
    if !is_style_edit(command) {
        return None;
    }
    Some((|| {
        let object = writable(session, command)?;
        let mut style = LayerStyle::from_object(object);
        change_style(&mut style, object, command)?;
        install_style(session, object.id, style)
    })())
}

/// edit.batch preflight runs against the document before the batch, so an effect.update/remove on an
/// effect added earlier in the same batch cannot be planned yet: only the target is checked up front and
/// the index is checked when the step applies (a bad index rolls the whole batch back).
pub fn batch_preflight(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), EFFECT_UPDATE | EFFECT_REMOVE).then(|| writable(session, command).is_ok() && index(&command.params).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_style::default_effect;
    use crate::document::{Document, Units};

    fn session() -> (Session, ObjectId) {
        let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1));
        let id = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], ..Object::default() });
        (s, id)
    }

    fn call(s: &mut Session, name: &str, params: Value) -> CommandResult {
        run(s, &Command::with_params(name, params)).expect("handled")
    }

    #[test]
    fn blend_set_is_one_undo_and_noop_keeps_version() {
        let (mut s, id) = session();
        let (version, depth) = (s.document.version, s.undo.applied_steps());
        call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "Normal"})).unwrap();
        assert_eq!((s.document.version, s.undo.applied_steps()), (version, depth));
        call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "Multiply"})).unwrap();
        assert_eq!(s.document.get_object(id).unwrap().blend_mode, BlendMode::Multiply);
        assert_eq!((s.document.version, s.undo.applied_steps()), (version + 1, depth + 1));
        call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "Multiply"})).unwrap();
        assert_eq!(s.document.version, version + 1);
        s.run_named("edit.undo", json!({})).unwrap();
        assert_eq!(s.document.get_object(id).unwrap().blend_mode, BlendMode::Normal);
        s.run_named("edit.redo", json!({})).unwrap();
        assert_eq!(s.document.get_object(id).unwrap().blend_mode, BlendMode::Multiply);
    }

    #[test]
    fn blend_set_rejects_bad_mode_and_pass_through_on_shape() {
        let (mut s, id) = session();
        assert!(call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "Sparkle"})).is_err());
        assert!(call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "PassThrough"})).is_err());
        assert!(call(&mut s, BLEND_SET, json!({"id": 999, "mode": "Multiply"})).is_err());
        assert_eq!(can_execute(&s, &Command::with_params(BLEND_SET, json!({"id": id.0, "mode": "Screen"}))), Some(true));
        assert_eq!(can_execute(&s, &Command::with_params(BLEND_SET, json!({"id": id.0}))), Some(false));
        assert_eq!(can_execute(&s, &Command::with_params("layer.other", json!({}))), None);
    }

    #[test]
    fn blend_ranges_set_validates_and_clears() {
        let (mut s, id) = session();
        let ranges = json!({"source": [[0.0, 0.0], [1.0, 1.0]], "underlying": [[0.0, 1.0], [1.0, 1.0]], "gamma": 1.0, "antialias": true});
        call(&mut s, BLEND_RANGES_SET, json!({"id": id.0, "ranges": ranges})).unwrap();
        assert!(s.document.get_object(id).unwrap().blend_ranges.is_some());
        assert!(call(&mut s, BLEND_RANGES_SET, json!({"id": id.0, "ranges": {"source": [[0.0, 2.0], [1.0, 1.0]], "underlying": [[0.0, 0.0], [1.0, 1.0]]}})).is_err());
        call(&mut s, BLEND_RANGES_SET, json!({"id": id.0, "ranges": null})).unwrap();
        assert!(s.document.get_object(id).unwrap().blend_ranges.is_none());
    }

    #[test]
    fn effect_add_update_remove_list_are_one_undo_each() {
        let (mut s, id) = session();
        let depth = s.undo.applied_steps();
        let added = call(&mut s, EFFECT_ADD, json!({"id": id.0, "kind": "OuterShadow"})).unwrap();
        assert_eq!(added["index"], 0);
        assert_eq!(s.undo.applied_steps(), depth + 1);
        assert_eq!(s.document.get_object(id).unwrap().effects[0], default_effect(EffectKind::OuterShadow));
        call(&mut s, EFFECT_UPDATE, json!({"id": id.0, "index": 0, "opacity": 0.5, "enabled": false})).unwrap();
        assert_eq!(s.undo.applied_steps(), depth + 2);
        let effect = &s.document.get_object(id).unwrap().effects[0];
        assert_eq!((effect.opacity, effect.enabled), (0.5, false));
        assert!(call(&mut s, EFFECT_UPDATE, json!({"id": id.0, "index": 0, "opacity": 2.0})).is_err());
        assert!(call(&mut s, EFFECT_UPDATE, json!({"id": id.0, "index": 0, "kind": "OuterGlow"})).is_err());
        assert!(call(&mut s, EFFECT_UPDATE, json!({"id": id.0, "index": 3, "opacity": 0.2})).is_err());
        let version = s.document.version;
        let listed = call(&mut s, EFFECT_LIST, json!({"id": id.0})).unwrap();
        assert_eq!(listed["effects"].as_array().unwrap().len(), 1);
        assert_eq!(s.document.version, version);
        call(&mut s, EFFECT_REMOVE, json!({"id": id.0, "index": 0})).unwrap();
        assert_eq!(s.undo.applied_steps(), depth + 3);
        assert!(s.document.get_object(id).unwrap().effects.is_empty());
        s.run_named("edit.undo", json!({})).unwrap();
        assert_eq!(s.document.get_object(id).unwrap().effects.len(), 1);
        s.run_named("edit.undo", json!({})).unwrap();
        assert_eq!(s.document.get_object(id).unwrap().effects[0].opacity, default_effect(EffectKind::OuterShadow).opacity);
        s.run_named("edit.undo", json!({})).unwrap();
        assert!(s.document.get_object(id).unwrap().effects.is_empty());
    }

    #[test]
    fn every_name_reaches_the_handler_through_the_session() {
        let (mut s, id) = session();
        s.run_named(BLEND_SET, json!({"id": id.0, "mode": "Darken"})).unwrap();
        s.run_named(BLEND_RANGES_SET, json!({"id": id.0, "ranges": null})).unwrap();
        s.run_named(EFFECT_ADD, json!({"id": id.0, "kind": "Outline"})).unwrap();
        s.run_named(EFFECT_UPDATE, json!({"id": id.0, "index": 0, "opacity": 0.4})).unwrap();
        s.run_named(EFFECT_LIST, json!({"id": id.0})).unwrap();
        s.run_named(EFFECT_PREVIEW, json!({"id": id.0, "index": 0})).unwrap();
        s.run_named(EFFECT_CANCEL, json!({"id": id.0})).unwrap();
        s.run_named(EFFECT_REMOVE, json!({"id": id.0, "index": 0})).unwrap();
        assert_eq!(s.document.get_object(id).unwrap().blend_mode, BlendMode::Darken);
        assert!(s.document.get_object(id).unwrap().effects.is_empty());
    }

    #[test]
    fn every_default_effect_is_valid() {
        for kind in EffectKind::ALL {
            assert!(default_effect(kind).is_valid(), "{kind:?}");
        }
    }

    #[test]
    fn preview_then_cancel_leaves_document_and_undo_unchanged() {
        let (mut s, id) = session();
        let (version, generation, depth) = (s.document.version, s.generation, s.undo.applied_steps());
        call(&mut s, EFFECT_PREVIEW, json!({"id": id.0, "kind": "GaussianBlur"})).unwrap();
        assert_eq!(staged(&s, id).unwrap().effect.kind, EffectKind::GaussianBlur);
        assert!(s.document.get_object(id).unwrap().effects.is_empty());
        let cancelled = call(&mut s, EFFECT_CANCEL, json!({"id": id.0})).unwrap();
        assert_eq!(cancelled["cancelled"], true);
        assert!(staged(&s, id).is_none());
        assert_eq!((s.document.version, s.generation, s.undo.applied_steps()), (version, generation, depth));
        assert!(call(&mut s, EFFECT_PREVIEW, json!({"id": id.0, "index": 0})).is_err());
    }

    #[test]
    fn commit_after_preview_clears_the_staged_effect() {
        let (mut s, id) = session();
        call(&mut s, EFFECT_PREVIEW, json!({"id": id.0, "kind": "OuterGlow"})).unwrap();
        call(&mut s, EFFECT_ADD, json!({"id": id.0, "kind": "OuterGlow"})).unwrap();
        assert!(staged(&s, id).is_none());
        assert_eq!(s.document.get_object(id).unwrap().effects.len(), 1);
    }

    #[test]
    fn locked_object_is_refused() {
        let (mut s, id) = session();
        s.document.find_object_mut(id.0).unwrap().locked = true;
        assert!(call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "Screen"})).is_err());
        assert!(call(&mut s, EFFECT_ADD, json!({"id": id.0, "kind": "Outline"})).is_err());
        assert!(call(&mut s, EFFECT_LIST, json!({"id": id.0})).is_ok());
    }

    #[test]
    fn style_survives_save_round_trip_and_old_json_defaults() {
        let (mut s, id) = session();
        call(&mut s, BLEND_SET, json!({"id": id.0, "mode": "Screen"})).unwrap();
        call(&mut s, EFFECT_ADD, json!({"id": id.0, "kind": "InnerGlow"})).unwrap();
        let object = s.document.get_object(id).unwrap().clone();
        let back: Object = serde_json::from_value(serde_json::to_value(&object).unwrap()).unwrap();
        assert!(same_style(&object, &back));
        let mut plain = serde_json::to_value(Object::default()).unwrap();
        let map = plain.as_object_mut().unwrap();
        map.remove("blend_mode");
        map.remove("blend_ranges");
        map.remove("effects");
        let old: Object = serde_json::from_value(plain).unwrap();
        assert_eq!(old.blend_mode, BlendMode::Normal);
        assert!(old.blend_ranges.is_none() && old.effects.is_empty());
    }
}
