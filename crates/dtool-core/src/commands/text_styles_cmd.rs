use crate::command::Session;
use crate::document::{Story, StoryId};
use crate::text_cmd::{param_u64, push_change, push_linked_text_change, refresh_art_bounds, sync_linked_change};
use crate::text_styles::{AppliedStyle, NamedTextStyle, StyleKind, TextStyleStore};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Map, Value};

pub const DEFINE: &str = "text.style.define";
pub const APPLY_NAMED: &str = "text.style.apply_named";
pub const CLEAR_OVERRIDE: &str = "text.style.clear_override";
pub const RESTORE: &str = "text.story.restore";

pub const DEFINE_FROM_SELECTION: &str = "text.style.define_from_selection";
pub const REMOVE: &str = "text.style.remove";
pub const LIST: &str = "text.styles.list";
pub const STORE_RESTORE: &str = "text.styles.restore";
pub const IMPORT: &str = "text.style.import";
pub const DETACH: &str = "text.style.detach";
pub const SET_NO_STYLE: &str = "text.style.set_no_style";
pub const DELETE_UNUSED: &str = "text.style.delete_unused";

pub fn handles(cmd: &Command) -> bool {
    matches!(cmd.name.as_str(), DEFINE | APPLY_NAMED | CLEAR_OVERRIDE)
}

fn kind_of(params: &Value) -> Result<StyleKind, CommandError> {
    match params.get("kind") {
        None | Some(Value::Null) => Ok(StyleKind::Character),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params()),
    }
}

fn style_name(params: &Value) -> Result<String, CommandError> {
    params.get("id").or_else(|| params.get("name")).and_then(Value::as_str).map(str::to_string).ok_or_else(CommandError::invalid_params)
}

fn is_break(ch: char) -> bool {
    ch == '\n' || ch == '\u{2029}'
}

fn range_of(params: &Value, story: &Story, kind: StyleKind) -> Result<(usize, usize), CommandError> {
    let range = params.get("range").ok_or_else(CommandError::invalid_params)?;
    let start = range.get("start").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize;
    let end = range.get("end").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)? as usize;
    let chars: Vec<char> = story.text.chars().collect();
    if start > end || end > chars.len() {
        return Err(CommandError::invalid_params());
    }
    if kind == StyleKind::Character {
        return Ok((start, end));
    }
    let low = chars[..start].iter().rposition(|ch| is_break(*ch)).map(|i| i + 1).unwrap_or(0);
    let high = chars[end..].iter().position(|ch| is_break(*ch)).map(|i| end + i + 1).unwrap_or(chars.len());
    Ok((low, high.max(low)))
}

fn bound(run: &Value, key: &str) -> usize {
    run.get(key).and_then(Value::as_u64).unwrap_or(0) as usize
}

pub fn restyle(runs: &[Value], start: usize, end: usize, fill_gaps: bool, mut edit: impl FnMut(&mut Map<String, Value>) -> Result<(), CommandError>) -> Result<Vec<Value>, CommandError> {
    let mut out = Vec::new();
    let mut covered = Vec::new();
    for run in runs {
        let (r_start, r_end) = (bound(run, "start"), bound(run, "end"));
        if !run.is_object() || r_end <= start || r_start >= end || start >= end {
            out.push(run.clone());
            continue;
        }
        let original = run.as_object().cloned().unwrap_or_default();
        let mut mid = original.clone();
        edit(&mut mid)?;
        if mid == original {
            out.push(run.clone());
            covered.push((r_start, r_end));
            continue;
        }
        if r_start < start {
            let mut left = run.clone();
            left["end"] = json!(start);
            out.push(left);
        }
        if r_end > end {
            let mut right = run.clone();
            right["start"] = json!(end);
            out.push(right);
        }
        let (m_start, m_end) = (r_start.max(start), r_end.min(end));
        mid.insert("start".into(), json!(m_start));
        mid.insert("end".into(), json!(m_end));
        covered.push((m_start, m_end));
        out.push(Value::Object(mid));
    }
    if fill_gaps && start < end {
        covered.sort_unstable();
        let mut cursor = start;
        let mut gaps = Vec::new();
        for (c_start, c_end) in covered {
            if c_start > cursor {
                gaps.push((cursor, c_start));
            }
            cursor = cursor.max(c_end);
        }
        if cursor < end {
            gaps.push((cursor, end));
        }
        for (g_start, g_end) in gaps {
            let mut run = Map::new();
            edit(&mut run)?;
            run.insert("start".into(), json!(g_start));
            run.insert("end".into(), json!(g_end));
            out.push(Value::Object(run));
        }
    }
    out.sort_by_key(|run| bound(run, "start"));
    Ok(out)
}

fn runs_mut(story: &mut Story, kind: StyleKind) -> &mut Vec<Value> {
    match kind {
        StyleKind::Character => &mut story.ranges,
        StyleKind::Paragraph => &mut story.paras,
    }
}

fn resolved_or_empty(store: &TextStyleStore, kind: StyleKind, applied: &AppliedStyle) -> Map<String, Value> {
    applied.style.as_deref().and_then(|name| store.resolve(kind, name).ok()).unwrap_or_default()
}

pub fn apply_named_to(story: &mut Story, store: &TextStyleStore, kind: StyleKind, name: &str, start: usize, end: usize, keep_overrides: bool) -> Result<bool, CommandError> {
    let props = store.resolve(kind, name)?;
    let before = runs_mut(story, kind).clone();
    let next = restyle(&before, start, end, true, |run| {
        let wrapped = Value::Object(run.clone());
        let applied = AppliedStyle::read_range(kind, &wrapped);
        let old_props = resolved_or_empty(store, kind, &applied);
        let next = store.apply(kind, name, keep_overrides, &applied)?;
        for key in old_props.keys().chain(applied.overrides.keys()) {
            if !props.contains_key(key) && !next.overrides.contains_key(key) {
                run.remove(key);
            }
        }
        for (key, value) in props.iter().chain(next.overrides.iter()) {
            run.insert(key.clone(), value.clone());
        }
        let mut wrapped = Value::Object(std::mem::take(run));
        next.write_range(kind, &mut wrapped);
        *run = wrapped.as_object().cloned().unwrap_or_default();
        Ok(())
    })?;
    let changed = next != before;
    *runs_mut(story, kind) = next;
    Ok(changed)
}

pub fn clear_override_on(story: &mut Story, store: &TextStyleStore, kind: StyleKind, keys: Option<&[String]>, start: usize, end: usize) -> Result<bool, CommandError> {
    let before = runs_mut(story, kind).clone();
    let next = restyle(&before, start, end, false, |run| {
        let wrapped = Value::Object(run.clone());
        let mut applied = AppliedStyle::read_range(kind, &wrapped);
        if applied.style.is_none() {
            return Ok(());
        }
        let props = resolved_or_empty(store, kind, &applied);
        let targets: Vec<String> = match keys {
            Some(keys) => keys.to_vec(),
            None => applied.overrides.keys().cloned().collect(),
        };
        for key in &targets {
            applied.clear_override(Some(key));
            match props.get(key) {
                Some(value) => {
                    run.insert(key.clone(), value.clone());
                }
                None => {
                    run.remove(key);
                }
            }
        }
        let mut wrapped = Value::Object(std::mem::take(run));
        applied.write_range(kind, &mut wrapped);
        *run = wrapped.as_object().cloned().unwrap_or_default();
        Ok(())
    })?;
    let changed = next != before;
    *runs_mut(story, kind) = next;
    Ok(changed)
}

fn drop_style_link(kind: StyleKind, run: &mut Map<String, Value>) {
    let wrapped = Value::Object(run.clone());
    let mut applied = AppliedStyle::read_range(kind, &wrapped);
    if applied.style.is_none() {
        return;
    }
    applied.style = None;
    applied.overrides = Map::new();
    let mut wrapped = Value::Object(std::mem::take(run));
    applied.write_range(kind, &mut wrapped);
    *run = wrapped.as_object().cloned().unwrap_or_default();
}

pub fn set_no_style_on(story: &mut Story, kind: StyleKind, start: usize, end: usize) -> Result<bool, CommandError> {
    detach_on(story, kind, start, end, None)
}

pub fn detach_on(story: &mut Story, kind: StyleKind, start: usize, end: usize, only: Option<&str>) -> Result<bool, CommandError> {
    let before = runs_mut(story, kind).clone();
    let next = restyle(&before, start, end, false, |run| {
        let wrapped = Value::Object(run.clone());
        let applied = AppliedStyle::read_range(kind, &wrapped);
        if applied.style.is_none() {
            return Ok(());
        }
        if only.is_some_and(|name| applied.style.as_deref() != Some(name)) {
            return Ok(());
        }
        drop_style_link(kind, run);
        Ok(())
    })?;
    let changed = next != before;
    *runs_mut(story, kind) = next;
    Ok(changed)
}

fn run_key(kind: StyleKind, run: &Value) -> Option<String> {
    run.get(crate::text_styles::style_key(kind)).and_then(Value::as_str).map(str::to_string)
}

fn detach_named(session: &mut Session, store: &TextStyleStore, kind: StyleKind, name: &str) -> CommandResult {
    store.get(kind, name).ok_or_else(CommandError::not_found)?;
    let mut old_stories = Vec::new();
    let mut new_stories = Vec::new();
    for story in session.document.stories.iter_mut() {
        let mut next = story.clone();
        let len = next.text.chars().count();
        if detach_on(&mut next, kind, 0, len, Some(name))? {
            old_stories.push(story.clone());
            *story = next.clone();
            new_stories.push(next);
        }
    }
    let (old_symbols, new_symbols) = edit_symbol_stories(&mut session.document.symbols, |story| {
        let len = story.text.chars().count();
        detach_on(story, kind, 0, len, Some(name))
    })?;
    if old_stories.is_empty() && old_symbols.is_empty() {
        return Ok(json!({ "doc_version": session.generation, "changes": [], "detached": 0 }));
    }
    push_change(session, DETACH,
        store_restore_command(store, &new_stories, &new_symbols),
        store_restore_command(store, &old_stories, &old_symbols));
    Ok(json!({ "doc_version": session.generation, "changes": [{ "name": DETACH }], "detached": new_stories.len() }))
}

fn initial_word_style(value: &Value) -> Option<&str> {
    value.get("initial_words")?.get("style")?.as_str()
}

fn used_style_names(stories: &[Story], store: &TextStyleStore, kind: StyleKind) -> Vec<String> {
    let mut names = Vec::new();
    for story in stories {
        let runs = match kind {
            StyleKind::Character => &story.ranges,
            StyleKind::Paragraph => &story.paras,
        };
        for run in runs {
            if let Some(name) = run_key(kind, run) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    if kind == StyleKind::Character {
        for name in stories.iter().flat_map(|story| story.paras.iter()).filter_map(initial_word_style) {
            if !names.iter().any(|used| used == name) {
                names.push(name.to_owned());
            }
        }
        for style in store.styles.iter().filter(|style| style.kind == StyleKind::Paragraph) {
            let name = style.props.get("initial_words").and_then(|options| options.get("style")).and_then(Value::as_str);
            if let Some(name) = name && !names.iter().any(|used| used == name) {
                names.push(name.to_owned());
            }
        }
    }
    let mut index = 0;
    while index < names.len() {
        if let Some(parent) = store.get(kind, &names[index]).and_then(|style| style.based_on.clone()) {
            if !names.contains(&parent) {
                names.push(parent);
            }
        }
        index += 1;
    }
    names
}

fn delete_unused_from(store: &mut TextStyleStore, stories: &[Story], params: &Value) -> Result<Vec<Value>, CommandError> {
    let kinds: Vec<StyleKind> = match params.get("kind") {
        None | Some(Value::Null) => vec![StyleKind::Character, StyleKind::Paragraph],
        Some(_) => vec![kind_of(params)?],
    };
    let mut keep: Vec<String> = Vec::new();
    for kind in &kinds {
        for name in used_style_names(stories, store, *kind) {
            keep.push(format!("{:?}:{}", kind, name));
        }
    }
    let mut removed = Vec::new();
    store.styles.retain(|style| {
        if !kinds.contains(&style.kind) {
            return true;
        }
        let key = format!("{:?}:{}", style.kind, style.name);
        if keep.contains(&key) {
            true
        } else {
            removed.push(json!({ "name": style.name, "kind": style.kind }));
            false
        }
    });
    Ok(removed)
}

fn story_index(session: &Session, params: &Value) -> Result<usize, CommandError> {
    let id = StoryId(param_u64(params, "story")?);
    session.document.stories.iter().position(|story| story.id == id).ok_or_else(CommandError::not_found)
}

fn commit(session: &mut Session, name: &str, before: Story, after: Story) -> CommandResult {
    if before == after {
        return Ok(json!({ "doc_version": session.generation, "changes": [] }));
    }
    if !session.document.symbols.instances.is_empty() {
        refresh_art_bounds(&mut session.document, after.id)?;
    }
    let linked = sync_linked_change(session, &before);
    if !push_linked_text_change(session, name, &before, linked)? {
        let restore = |story: &Story| Command::with_params(RESTORE, json!({ "story": story }));
        push_change(session, name, restore(&after), restore(&before));
    }
    Ok(json!({ "doc_version": session.generation, "changes": [{ "name": name }] }))
}

pub fn run_with(session: &mut Session, store: &mut TextStyleStore, cmd: &Command) -> Option<CommandResult> {
    if !handles(cmd) {
        return None;
    }
    Some(run_inner(session, store, cmd))
}

fn run_inner(session: &mut Session, store: &mut TextStyleStore, cmd: &Command) -> CommandResult {
    let params = &cmd.params;
    let kind = kind_of(params)?;
    if cmd.name == DEFINE {
        let style = NamedTextStyle {
            name: style_name(params)?,
            kind,
            based_on: params.get("based_on").and_then(Value::as_str).map(str::to_string),
            props: params.get("props").and_then(Value::as_object).cloned().unwrap_or_default(),
            next_style: params.get("next_style").and_then(Value::as_str).filter(|name| !name.is_empty()).map(str::to_string),
        };
        let before = store.clone();
        store.define(style)?;
        let changed = *store != before;
        return Ok(json!({ "doc_version": session.generation, "changed": changed }));
    }
    if cmd.name == DETACH && params.get("story").is_none() {
        return detach_named(session, store, kind, &style_name(params)?);
    }
    let index = story_index(session, params)?;
    let before = session.document.stories[index].clone();
    let (start, end) = range_of(params, &before, kind)?;
    let mut after = before.clone();
    if cmd.name == APPLY_NAMED {
        let keep = params.get("keep_overrides").and_then(Value::as_bool).unwrap_or(false);
        apply_named_to(&mut after, store, kind, &style_name(params)?, start, end, keep)?;
        session.document.stories[index] = after.clone();
        return commit(session, &cmd.name, before, after);
    }
    if cmd.name == SET_NO_STYLE {
        set_no_style_on(&mut after, kind, start, end)?;
        session.document.stories[index] = after.clone();
        return commit(session, &cmd.name, before, after);
    }
    if cmd.name == DETACH {
        if params.get("story").is_some() {
            detach_on(&mut after, kind, start, end, params.get("id").or_else(|| params.get("name")).and_then(Value::as_str))?;
            session.document.stories[index] = after.clone();
            return commit(session, &cmd.name, before, after);
        }
        return detach_named(session, store, kind, &style_name(params)?);
    }
    let keys: Option<Vec<String>> = match params.get("keys") {
        None | Some(Value::Null) => None,
        Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?),
    };
    clear_override_on(&mut after, store, kind, keys.as_deref(), start, end)?;
    session.document.stories[index] = after.clone();
    commit(session, &cmd.name, before, after)
}

pub fn can_execute(session: &Session, cmd: &Command) -> Option<bool> {
    match cmd.name.as_str() {
        LIST | DEFINE | DEFINE_FROM_SELECTION | REMOVE | APPLY_NAMED | CLEAR_OVERRIDE | IMPORT | DETACH | SET_NO_STYLE | DELETE_UNUSED => Some(session.has_document),
        STORE_RESTORE => Some(false),
        _ => None,
    }
}

pub fn run(session: &mut Session, cmd: &Command) -> Option<CommandResult> {
    let name = cmd.name.as_str();
    if !matches!(name, LIST | DEFINE | DEFINE_FROM_SELECTION | REMOVE | APPLY_NAMED | CLEAR_OVERRIDE | STORE_RESTORE | IMPORT | DETACH | SET_NO_STYLE | DELETE_UNUSED) {
        return None;
    }
    if name == STORE_RESTORE {
        return Some(Err(CommandError::invalid_params()));
    }
    if !session.has_document {
        return Some(Err(CommandError::no_document()));
    }
    Some(match name {
        LIST => list(session, &cmd.params),
        APPLY_NAMED | CLEAR_OVERRIDE | DETACH | SET_NO_STYLE => {
            let mut store = session.document.text_styles.clone();
            run_inner(session, &mut store, cmd)
        }
        _ => store_edit(session, cmd),
    })
}

pub fn apply(session: &mut Session, cmd: &Command) -> Option<Result<(), CommandError>> {
    if cmd.name != STORE_RESTORE {
        return None;
    }
    Some(restore_store(session, &cmd.params))
}

fn restore_store(session: &mut Session, params: &Value) -> Result<(), CommandError> {
    let store: TextStyleStore = serde_json::from_value(params.get("store").cloned().unwrap_or(Value::Null)).map_err(|_| CommandError::invalid_params())?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let stories: Vec<Story> = match params.get("stories") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
    };
    if stories.iter().any(|story| !session.document.stories.iter().any(|s| s.id == story.id)) {
        return Err(CommandError::not_found());
    }
    let symbol_stories: Vec<(u64, Story)> = match params.get("symbol_stories") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
    };
    session.document.text_styles = store;
    for (definition, current) in session.document.symbols.stories_mut() {
        if let Some((_, prior)) = symbol_stories.iter().find(|(id, story)| *id == definition && story.id == current.id) {
            current.clone_from(prior);
        }
    }
    for story in stories {
        let id = story.id;
        if let Some(slot) = session.document.stories.iter_mut().find(|s| s.id == id) {
            *slot = story;
        }
        refresh_art_bounds(&mut session.document, id)?;
    }
    session.document.version = version;
    Ok(())
}

fn canon(key: &str, value: &Value) -> Value {
    let mut wrapped = json!({ key: value });
    crate::paint::normalize_range_fill(&mut wrapped);
    wrapped.get(key).cloned().unwrap_or(Value::Null)
}

pub fn relink_runs(runs: &mut [Value], kind: StyleKind, old: &TextStyleStore, new: &TextStyleStore) -> bool {
    let mut changed = false;
    for run in runs.iter_mut() {
        let applied = AppliedStyle::read_range(kind, run);
        let Some(name) = applied.style.clone() else { continue };
        let Some(fields) = run.as_object_mut() else { continue };
        let before = fields.clone();
        match new.resolve(kind, &name) {
            Ok(new_props) => {
                let old_props = old.resolve(kind, &name).unwrap_or_default();
                let mut overrides = applied.overrides.clone();
                let mut keys: Vec<String> = old_props.keys().chain(new_props.keys()).cloned().collect();
                keys.sort();
                keys.dedup();
                for key in keys {
                    if overrides.contains_key(&key) {
                        continue;
                    }
                    if let Some(was) = old_props.get(&key) {
                        match fields.get(&key) {
                            Some(current) if canon(&key, current) != canon(&key, was) => {
                                overrides.insert(key.clone(), current.clone());
                                continue;
                            }
                            None => continue,
                            _ => {}
                        }
                    }
                    match new_props.get(&key) {
                        Some(value) => {
                            fields.insert(key, value.clone());
                        }
                        None => {
                            fields.remove(&key);
                        }
                    }
                }
                let mut wrapped = Value::Object(std::mem::take(fields));
                AppliedStyle { style: Some(name), overrides }.write_range(kind, &mut wrapped);
                *run = wrapped;
            }
            Err(_) => {
                fields.remove(crate::text_styles::style_key(kind));
                fields.remove(crate::text_styles::STYLE_OVERRIDES_KEY);
            }
        }
        changed |= run.as_object() != Some(&before);
    }
    changed
}

pub fn relink_story(story: &mut Story, old: &TextStyleStore, new: &TextStyleStore) -> bool {
    let chars = relink_runs(&mut story.ranges, StyleKind::Character, old, new);
    let paras = relink_runs(&mut story.paras, StyleKind::Paragraph, old, new);
    chars || paras || story.paras.iter().filter_map(initial_word_style).any(|name| {
        old.resolve(StyleKind::Character, name).ok() != new.resolve(StyleKind::Character, name).ok()
    })
}

fn list(session: &Session, params: &Value) -> CommandResult {
    let store = &session.document.text_styles;
    let kinds: Vec<StyleKind> = match params.get("kind") {
        None | Some(Value::Null) => vec![StyleKind::Character, StyleKind::Paragraph],
        Some(_) => vec![kind_of(params)?],
    };
    let styles: Vec<Value> = store
        .styles
        .iter()
        .filter(|style| kinds.contains(&style.kind))
        .map(|style| json!({ "name": style.name, "kind": style.kind, "based_on": style.based_on, "next_style": style.next_style, "props": style.props }))
        .collect();
    Ok(json!({ "doc_version": session.generation, "styles": styles }))
}

fn selection_props(story: &Story, kind: StyleKind, start: usize) -> Map<String, Value> {
    let runs = match kind {
        StyleKind::Character => &story.ranges,
        StyleKind::Paragraph => &story.paras,
    };
    let hit = runs.iter().find(|run| bound(run, "start") <= start && bound(run, "end") > start);
    let mut props = hit.and_then(Value::as_object).cloned().unwrap_or_default();
    for key in ["start", "end", crate::text_styles::style_key(kind), crate::text_styles::STYLE_OVERRIDES_KEY] {
        props.remove(key);
    }
    props
}

type SymbolStories = Vec<(u64, Story)>;

fn edit_symbol_stories(
    symbols: &mut super::symbols::Symbols, mut edit: impl FnMut(&mut Story) -> Result<bool, CommandError>,
) -> Result<(SymbolStories, SymbolStories), CommandError> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (definition, story) in symbols.stories_mut() {
        let mut updated = story.clone();
        if edit(&mut updated)? {
            before.push((definition, story.clone()));
            story.clone_from(&updated);
            after.push((definition, updated));
        }
    }
    Ok((before, after))
}

fn store_restore_command(store: &TextStyleStore, stories: &[Story], symbols: &SymbolStories) -> Command {
    Command::with_params(STORE_RESTORE, json!({ "store": store, "stories": stories, "symbol_stories": symbols }))
}

fn store_edit(session: &mut Session, cmd: &Command) -> CommandResult {
    let params = &cmd.params;
    let before = session.document.text_styles.clone();
    let mut store = before.clone();
    let mut imported = Vec::new();
    let mut removed = Value::Null;
    match cmd.name.as_str() {
        IMPORT => {
            imported = import_styles(&mut store, params)?;
        }
        DEFINE => {
            run_inner(session, &mut store, cmd)?;
        }
        REMOVE => {
            store.remove(kind_of(params)?, &style_name(params)?)?;
        }
        DELETE_UNUSED => {
            let stories = session.document.stories.iter().chain(session.document.symbols.stories()).cloned().collect::<Vec<_>>();
            removed = Value::Array(delete_unused_from(&mut store, &stories, params)?);
            if store == before {
                return Ok(json!({ "doc_version": session.generation, "changes": [], "removed": removed }));
            }
        }
        _ => {
            let kind = kind_of(params)?;
            let index = story_index(session, params)?;
            let story = &session.document.stories[index];
            let (start, _) = range_of(params, story, kind)?;
            let current = selection_props(story, kind, start);
            let based_on = params.get("based_on").and_then(Value::as_str);
            store.define_from_selection(&style_name(params)?, kind, based_on, &current)?;
        }
    }
    if store == before {
        return Ok(json!({ "doc_version": session.generation, "changes": [], "imported": imported }));
    }
    let mut old_stories = Vec::new();
    let mut new_stories = Vec::new();
    for story in session.document.stories.iter_mut() {
        let mut next = story.clone();
        if relink_story(&mut next, &before, &store) {
            old_stories.push(story.clone());
            *story = next.clone();
            new_stories.push(next);
        }
    }
    let (old_symbols, new_symbols) = edit_symbol_stories(&mut session.document.symbols, |story| {
        Ok(relink_story(story, &before, &store))
    })?;
    session.document.text_styles = store.clone();
    for story in &new_stories {
        refresh_art_bounds(&mut session.document, story.id)?;
    }
    push_change(session, &cmd.name,
        store_restore_command(&store, &new_stories, &new_symbols),
        store_restore_command(&before, &old_stories, &old_symbols));
    Ok(json!({ "doc_version": session.generation, "changes": [{ "name": cmd.name }], "relinked_stories": new_stories.iter().map(|s| s.id.0).collect::<Vec<_>>(), "imported": imported, "removed": removed }))
}

fn import_target_name(store: &TextStyleStore, taken: &[(StyleKind, String)], kind: StyleKind, name: &str) -> String {
    (2..)
        .map(|n| format!("{name} {n}"))
        .find(|candidate| store.get(kind, candidate).is_none() && !taken.iter().any(|(k, n)| *k == kind && n == candidate))
        .unwrap_or_else(|| name.to_string())
}

/// `text.style.import {path, kind?, names?, conflict: "rename"|"replace"}`: copies named styles from another
/// native document. Requested names bring their based_on ancestors. "rename" (default) keeps the existing style and
/// imports the clash as "<name> 2"; "replace" redefines the existing style (stories using it relink). One undo step.
fn import_styles(store: &mut TextStyleStore, params: &Value) -> Result<Vec<Value>, CommandError> {
    let path = params.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()).ok_or_else(CommandError::invalid_params)?;
    let replace = match params.get("conflict").and_then(Value::as_str).unwrap_or("rename") {
        "rename" => false,
        "replace" => true,
        _ => return Err(CommandError::invalid_params()),
    };
    let kinds: Vec<StyleKind> = match params.get("kind") {
        None | Some(Value::Null) => vec![StyleKind::Character, StyleKind::Paragraph],
        Some(_) => vec![kind_of(params)?],
    };
    let names: Option<Vec<String>> = match params.get("names") {
        None | Some(Value::Null) => None,
        Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?),
    };
    let source = crate::document::read_text_styles(std::path::Path::new(path))?;
    let mut wanted: Vec<(StyleKind, String)> = source
        .styles
        .iter()
        .filter(|style| kinds.contains(&style.kind) && names.as_ref().map_or(true, |names| names.contains(&style.name)))
        .map(|style| (style.kind, style.name.clone()))
        .collect();
    if let Some(names) = &names {
        if names.iter().any(|name| !wanted.iter().any(|(_, n)| n == name)) {
            return Err(CommandError::not_found());
        }
    }
    let mut index = 0;
    while index < wanted.len() {
        let (kind, name) = wanted[index].clone();
        if let Some(parent) = source.get(kind, &name).and_then(|style| style.based_on.clone()) {
            if !wanted.iter().any(|(k, n)| *k == kind && *n == parent) && source.get(kind, &parent).is_some() {
                wanted.push((kind, parent));
            }
        }
        index += 1;
    }
    let mut targets: Vec<((StyleKind, String), String, &'static str)> = Vec::new();
    let mut taken: Vec<(StyleKind, String)> = Vec::new();
    for (kind, name) in &wanted {
        let (target, action) = match store.get(*kind, name) {
            None => (name.clone(), "added"),
            Some(_) if replace => (name.clone(), "replaced"),
            Some(_) => (import_target_name(store, &taken, *kind, name), "renamed"),
        };
        taken.push((*kind, target.clone()));
        targets.push(((*kind, name.clone()), target, action));
    }
    let target_of = |kind: StyleKind, name: &str| targets.iter().find(|((k, n), _, _)| *k == kind && n == name).map(|(_, target, _)| target.clone());
    let mut pending: Vec<NamedTextStyle> = wanted.iter().filter_map(|(kind, name)| source.get(*kind, name).cloned()).collect();
    let mut report = Vec::new();
    while !pending.is_empty() {
        let ready = pending
            .iter()
            .position(|style| style.based_on.as_ref().map_or(true, |parent| !pending.iter().any(|other| other.kind == style.kind && &other.name == parent)))
            .ok_or_else(|| CommandError::new("STYLE_CYCLE"))?;
        let mut style = pending.remove(ready);
        let original = style.name.clone();
        let action = targets.iter().find(|((k, n), _, _)| *k == style.kind && *n == original).map(|(_, _, action)| *action).unwrap_or("added");
        style.name = target_of(style.kind, &original).unwrap_or_else(|| original.clone());
        style.based_on = style.based_on.take().map(|parent| target_of(style.kind, &parent).unwrap_or(parent));
        if style.based_on.as_ref().is_some_and(|parent| store.get(style.kind, parent).is_none()) {
            style.props = source.resolve(style.kind, &original)?;
            style.based_on = None;
        }
        report.push(json!({ "name": original, "kind": style.kind, "as": style.name, "action": action }));
        store.define(style)?;
    }
    Ok(report)
}
