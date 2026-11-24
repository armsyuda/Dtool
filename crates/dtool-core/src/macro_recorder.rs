//! Per-session macro recorder tap (C18). Session::run feeds successful, recordable commands here.
use dtool_protocol::{Command, CommandCatalog, CommandError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedStep {
    pub command: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct MacroRecorder {
    recording: bool,
    steps: Vec<RecordedStep>,
    depth: u32,
    /// Object ids created by recorded steps, in creation order ("$new:N" = created[N]).
    created: Vec<u64>,
}

impl MacroRecorder {
    /// Begin a new recording; previous steps are cleared.
    pub fn start(&mut self) {
        self.recording = true;
        self.steps.clear();
        self.created.clear();
    }

    /// Stop recording; steps stay frozen until `take` or the next `start`.
    pub fn stop(&mut self) -> Vec<RecordedStep> {
        self.recording = false;
        self.steps.clone()
    }

    /// Stop and hand the recorded steps out, leaving the recorder empty.
    pub fn take(&mut self) -> Vec<RecordedStep> {
        self.recording = false;
        std::mem::take(&mut self.steps)
    }

    pub fn reset(&mut self) {
        self.recording = false;
        self.steps.clear();
        self.created.clear();
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn steps(&self) -> &[RecordedStep] {
        &self.steps
    }

    /// Nested Session::run calls made by a handler are not separate user steps.
    pub(crate) fn enter(&mut self) {
        self.depth += 1;
    }

    pub(crate) fn exit(&mut self) -> bool {
        self.depth = self.depth.saturating_sub(1);
        self.depth == 0
    }

    pub fn tap(&mut self, catalog: &CommandCatalog, command: &Command) {
        self.tap_step(catalog, command, None, None);
    }

    /// Capture the state a top-level step starts from. Call before `enter`; None when not recording
    /// or when this is a nested Session::run.
    pub fn step_context(&self, session: &crate::command::Session) -> Option<StepContext> {
        (self.recording && self.depth == 0).then(|| StepContext::capture(session))
    }

    /// Record a successful step. With a context, object ids that were selected when the step started
    /// become "$sel"/"$sel:N" and ids created by earlier recorded steps become "$new:N"; other ids stay absolute.
    pub fn tap_step(
        &mut self,
        catalog: &CommandCatalog,
        command: &Command,
        context: Option<StepContext>,
        document_after: Option<&crate::document::Document>,
    ) {
        if !(self.recording && self.depth == 0 && is_recordable(catalog, &command.name)) {
            return;
        }
        let mut params = match &context {
            Some(ctx) => relativize(&command.name, &command.params, &ctx.selection, &self.created),
            None => command.params.clone(),
        };
        if INCLUDED_PREFIXES.iter().any(|prefix| command.name.starts_with(prefix)) {
            // A doc.setup preview token belongs to this session; the params alone replay the change.
            if let Value::Object(map) = &mut params {
                map.remove("token");
            }
        }
        self.steps.push(RecordedStep { command: command.name.clone(), params });
        if let (Some(ctx), Some(doc)) = (context, document_after) {
            for id in created_since(doc, ctx.next_object_id) {
                if !self.created.contains(&id) {
                    self.created.push(id);
                }
            }
        }
    }
}

/// Selection and id high-water a step starts from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepContext {
    pub selection: Vec<u64>,
    pub next_object_id: u64,
}

impl StepContext {
    pub fn capture(session: &crate::command::Session) -> Self {
        Self {
            selection: session.selection.ids.iter().map(|id| id.0).collect(),
            next_object_id: session.document.next_object_id,
        }
    }
}

pub const REF_SELECTION: &str = "$sel";
pub const REF_SELECTION_PREFIX: &str = "$sel:";
pub const REF_NEW_PREFIX: &str = "$new:";
pub const MACRO_REF_UNRESOLVED: &str = "MACRO_REF_UNRESOLVED";

/// Param keys that hold object ids.
const OBJECT_ID_KEYS: &[&str] = &[
    "id", "ids", "object_id", "object_ids", "target_id", "target_ids", "parent_id", "frame_id",
    "path_id", "mask_id", "source_id", "anchor_id", "key_object_id",
];
/// Command families whose "id" is not an object id (guides, swatches, pages, styles...).
const NON_OBJECT_ID_PREFIXES: &[&str] = &[
    "guides.", "guide.", "swatch", "colour.swatch", "color.swatch", "page.", "pages.", "doc.", "style.", "styles.",
    "story.", "slice.", "slices.", "resource.", "asset.", "assets.", "brush.", "symbol.", "layer.comp",
];

fn object_id_key(command: &str, key: &str) -> bool {
    OBJECT_ID_KEYS.contains(&key) && !NON_OBJECT_ID_PREFIXES.iter().any(|prefix| command.starts_with(prefix))
}

/// Object ids created since `next_before` (ids at or above the previous high-water), ascending.
pub fn created_since(doc: &crate::document::Document, next_before: u64) -> Vec<u64> {
    let mut ids: Vec<u64> = doc.objects.iter().map(|o| o.id.0).filter(|id| *id >= next_before).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn relative_ref(id: u64, selection: &[u64], created: &[u64]) -> Option<Value> {
    if let Some(n) = created.iter().position(|c| *c == id) {
        return Some(Value::String(format!("{REF_NEW_PREFIX}{n}")));
    }
    selection.iter().position(|s| *s == id).map(|n| Value::String(format!("{REF_SELECTION_PREFIX}{n}")))
}

fn relativize_ids(value: &Value, selection: &[u64], created: &[u64]) -> Value {
    match value {
        Value::Number(n) => n.as_u64().and_then(|id| relative_ref(id, selection, created)).unwrap_or_else(|| value.clone()),
        Value::Array(items) => {
            let ids: Option<Vec<u64>> = items.iter().map(|v| v.as_u64()).collect();
            if let Some(ids) = ids {
                if !ids.is_empty() && !selection.is_empty() && ids == selection && !ids.iter().any(|id| created.contains(id)) {
                    return Value::String(REF_SELECTION.to_string());
                }
            }
            Value::Array(items.iter().map(|v| relativize_ids(v, selection, created)).collect())
        }
        other => other.clone(),
    }
}

/// Rewrite object-id params: ids created by earlier recorded steps -> "$new:N", ids in the step's starting
/// selection -> "$sel:N", an id list equal to the whole selection -> "$sel". Ids in neither set stay absolute.
pub fn relativize(command: &str, params: &Value, selection: &[u64], created: &[u64]) -> Value {
    match params {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if object_id_key(command, key) {
                        relativize_ids(value, selection, created)
                    } else {
                        relativize(command, value, selection, created)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| relativize(command, v, selection, created)).collect()),
        other => other.clone(),
    }
}

fn unresolved() -> CommandError {
    CommandError::new(MACRO_REF_UNRESOLVED)
}

fn resolve_ref(text: &str, selection: &[u64], created: &[u64]) -> Result<Option<Vec<u64>>, CommandError> {
    if text == REF_SELECTION {
        return if selection.is_empty() { Err(unresolved()) } else { Ok(Some(selection.to_vec())) };
    }
    let pick = |rest: &str, pool: &[u64]| rest.parse::<usize>().ok().and_then(|n| pool.get(n).copied()).map(|id| vec![id]).ok_or_else(unresolved);
    if let Some(rest) = text.strip_prefix(REF_SELECTION_PREFIX) {
        return pick(rest, selection).map(Some);
    }
    if let Some(rest) = text.strip_prefix(REF_NEW_PREFIX) {
        return pick(rest, created).map(Some);
    }
    Ok(None)
}

fn resolve_ids(value: &Value, plural: bool, selection: &[u64], created: &[u64]) -> Result<Value, CommandError> {
    match value {
        Value::String(text) => match resolve_ref(text, selection, created)? {
            Some(ids) if plural && text == REF_SELECTION => Ok(Value::from(ids)),
            Some(ids) if ids.len() == 1 => Ok(Value::from(ids[0])),
            Some(_) => Err(unresolved()),
            None => Ok(value.clone()),
        },
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(text) => match resolve_ref(text, selection, created)? {
                        Some(ids) => out.extend(ids.into_iter().map(Value::from)),
                        None => out.push(item.clone()),
                    },
                    other => out.push(resolve_ids(other, true, selection, created)?),
                }
            }
            Ok(Value::Array(out))
        }
        other => Ok(other.clone()),
    }
}

/// Resolve "$sel"/"$sel:N"/"$new:N" against the playing session's selection (at step start) and the ids
/// created so far in this play. A single-id key given "$sel" needs exactly one selected object.
/// Steps without refs (old absolute format) come back unchanged.
pub fn resolve(command: &str, params: &Value, selection: &[u64], created: &[u64]) -> Result<Value, CommandError> {
    match params {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, value) in map {
                let value = if object_id_key(command, key) {
                    resolve_ids(value, key.ends_with("ids"), selection, created)?
                } else {
                    resolve(command, value, selection, created)?
                };
                out.insert(key.clone(), value);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => items.iter().map(|v| resolve(command, v, selection, created)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        other => Ok(other.clone()),
    }
}

/// Per-play ref state: resolve a step against the current session, then note what it created.
#[derive(Debug, Clone, Default)]
pub struct ReplayRefs {
    created: Vec<u64>,
}

impl ReplayRefs {
    pub fn created(&self) -> &[u64] {
        &self.created
    }

    /// Resolved params plus the context to hand to `finish` after the step ran.
    pub fn prepare(&self, session: &crate::command::Session, command: &str, params: &Value) -> Result<(Value, StepContext), CommandError> {
        let ctx = StepContext::capture(session);
        let params = resolve(command, params, &ctx.selection, &self.created)?;
        Ok((params, ctx))
    }

    pub fn finish(&mut self, session: &crate::command::Session, context: &StepContext) {
        for id in created_since(&session.document, context.next_object_id) {
            if !self.created.contains(&id) {
                self.created.push(id);
            }
        }
    }
}

const EXCLUDED_PREFIXES: &[&str] = &[
    "macro.", "history.", "doc.", "file.", "app.", "ui.", "window.", "view.", "pref.", "prefs.", "autosave.",
    "confirm.", "export.", "import.", "print.", "clipboard.", "tool.", "workspace.", "mcp.", "script.", "plugin.",
];
/// Allowed despite an excluded family prefix: document setup changes (Photoshop "Canvas Size"/"Image Size" steps).
/// .preview/.cancel/.query/.restore stay excluded by suffix.
const INCLUDED_PREFIXES: &[&str] = &["doc.setup."];
const EXCLUDED_NAMES: &[&str] = &["edit.undo", "edit.redo", "edit.copy", "edit.cut", "edit.paste", "open", "save", "close", "new"];
const EXCLUDED_SUFFIXES: &[&str] = &[".preview", ".cancel", ".query", ".list", ".restore"];

/// Shared predicate: a command may be recorded into (and replayed from) a macro.
/// Must be a known, implemented, mutating catalog command that is not a query,
/// undo/redo, file/window/UI/confirmation, preview/cancel, or internal restore command.
pub fn is_recordable(catalog: &CommandCatalog, name: &str) -> bool {
    let Some(spec) = catalog.resolve(name) else { return false };
    spec.implemented
        && spec.mutating
        && !EXCLUDED_NAMES.contains(&name)
        && (INCLUDED_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
            || !EXCLUDED_PREFIXES.iter().any(|prefix| name.starts_with(prefix)))
        && !EXCLUDED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}