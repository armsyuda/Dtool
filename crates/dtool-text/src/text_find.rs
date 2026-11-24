use dtool_core::document::Document;
use dtool_core::{Command, CommandError, Session, StoryId};
use dtool_core::serde_json::{json, Map, Value};
use dtool_core::text_styles::{StyleKind, CHAR_STYLE_KEY, PARA_STYLE_KEY};
use regex::{Captures, Regex, RegexBuilder};

pub const TEXT_FIND: &str = "text.find";
pub const TEXT_REPLACE: &str = "text.replace";
pub const TEXT_REPLACE_ALL: &str = "text.replace_all";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindScope {
    Document,
    Story(u64),
    Selection { story: u64, start: usize, end: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindQuery {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindFlags {
    pub whole_word: bool,
}

impl FindQuery {
    pub fn literal(pattern: &str) -> Self {
        Self { pattern: pattern.to_owned(), regex: false, case_sensitive: true }
    }

    pub fn regex(pattern: &str) -> Self {
        Self { pattern: pattern.to_owned(), regex: true, case_sensitive: true }
    }

    pub fn compile(&self) -> Result<Regex, CommandError> {
        if self.pattern.is_empty() {
            return Err(CommandError::invalid_params());
        }
        let source = if self.regex { self.pattern.clone() } else { regex::escape(&self.pattern) };
        RegexBuilder::new(&source)
            .case_insensitive(!self.case_sensitive)
            .build()
            .map_err(|_| CommandError::new("INVALID_PATTERN"))
    }
}

/// Format filter for find/replace (`format` param). Char keys compare against the character run at every matched
/// scalar (family, size_pt, weight, bold, italic, fill/color, char_style, ...); `para_style` and `para.<key>` compare
/// against the paragraph run at the match start. An empty pattern with a format finds each maximal formatted span.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatQuery {
    pub char: Map<String, Value>,
    pub para: Map<String, Value>,
}

impl FormatQuery {
    pub fn from_value(value: Option<&Value>) -> Result<Self, CommandError> {
        let mut out = Self::default();
        let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(out) };
        let map = value.as_object().ok_or_else(CommandError::invalid_params)?;
        for (key, value) in map {
            if key == PARA_STYLE_KEY {
                out.para.insert(key.clone(), value.clone());
            } else if let Some(rest) = key.strip_prefix("para.") {
                out.para.insert(rest.to_owned(), value.clone());
            } else if key == "color" || key == "colour" {
                out.char.insert("fill".to_owned(), value.clone());
            } else {
                out.char.insert(key.clone(), value.clone());
            }
        }
        Ok(out)
    }

    pub fn is_empty(&self) -> bool {
        self.char.is_empty() && self.para.is_empty()
    }
}

fn fill_canon(value: &Value) -> Value {
    let mut wrapped = json!({ "fill": value });
    dtool_core::paint::normalize_range_fill(&mut wrapped);
    wrapped.get("fill").cloned().unwrap_or(Value::Null)
}

fn format_value_eq(key: &str, want: &Value, have: Option<&Value>) -> bool {
    let have = have.unwrap_or(&Value::Null);
    match (want, have) {
        (Value::Bool(false), Value::Null) => true,
        (Value::Number(a), Value::Number(b)) => (a.as_f64().unwrap_or(f64::NAN) - b.as_f64().unwrap_or(f64::NAN)).abs() < 1e-3,
        (Value::String(a), Value::String(b)) if key == "family" => a.eq_ignore_ascii_case(b),
        _ if key == "fill" => fill_canon(want) == fill_canon(have),
        _ => want == have,
    }
}

fn run_at(runs: &[Value], at: usize) -> Option<&Value> {
    let bound = |run: &Value, key: &str| run.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
    runs.iter().find(|run| bound(run, "start") <= at && at < bound(run, "end"))
}

fn props_match(want: &Map<String, Value>, run: Option<&Value>) -> bool {
    want.iter().all(|(key, value)| format_value_eq(key, value, run.and_then(|r| r.get(key))))
}

fn char_ok(story: &dtool_core::Story, format: &FormatQuery, at: usize) -> bool {
    format.char.is_empty() || props_match(&format.char, run_at(&story.ranges, at))
}

fn para_ok(story: &dtool_core::Story, format: &FormatQuery, at: usize) -> bool {
    format.para.is_empty() || props_match(&format.para, run_at(&story.paras, at))
}

fn format_ok(story: &dtool_core::Story, format: &FormatQuery, start: usize, end: usize) -> bool {
    para_ok(story, format, start) && (start..end).all(|at| char_ok(story, format, at))
}

/// Replace-side format (`replace_format` param): char props patched onto the replaced range; `char_style` applies the
/// named character style (resolved props + style tag).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplaceFormat {
    pub patch: Map<String, Value>,
}

impl ReplaceFormat {
    pub fn from_value(doc: &Document, value: Option<&Value>) -> Result<Option<Self>, CommandError> {
        let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(None) };
        let map = value.as_object().ok_or_else(CommandError::invalid_params)?;
        if map.is_empty() || map.keys().any(|k| k == PARA_STYLE_KEY || k.starts_with("para.") || k == "start" || k == "end") {
            return Err(CommandError::invalid_params());
        }
        let mut patch = Map::new();
        if let Some(name) = map.get(CHAR_STYLE_KEY) {
            let name = name.as_str().ok_or_else(CommandError::invalid_params)?;
            patch = doc.text_styles.resolve(StyleKind::Character, name)?;
            patch.insert(CHAR_STYLE_KEY.to_owned(), json!(name));
        }
        for (key, value) in map {
            if key == CHAR_STYLE_KEY {
                continue;
            }
            let key = if key == "color" || key == "colour" { "fill".to_owned() } else { key.clone() };
            patch.insert(key, value.clone());
        }
        let mut wrapped = Value::Object(patch);
        dtool_core::paint::normalize_range_fill(&mut wrapped);
        Ok(Some(Self { patch: wrapped.as_object().cloned().unwrap_or_default() }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindMatch {
    pub story: u64,
    pub start: usize,
    pub end: usize,
}

fn scalar_at(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

fn byte_at(text: &str, scalar: usize) -> Option<usize> {
    if scalar == text.chars().count() {
        return Some(text.len());
    }
    text.char_indices().nth(scalar).map(|(b, _)| b)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let before_ok = start == 0 || text[..start].chars().next_back().is_none_or(|c| !is_word_char(c));
    let after_ok = end == text.len() || text[end..].chars().next().is_none_or(|c| !is_word_char(c));
    before_ok && after_ok
}

pub fn selection_scope(session: &Session) -> Option<FindScope> {
    let edit = session.text_edit.as_ref()?;
    let (start, end) = (edit.caret.min(edit.anchor), edit.caret.max(edit.anchor));
    (start < end).then_some(FindScope::Selection { story: edit.story.0, start, end })
}

fn scoped_stories<'a>(doc: &'a Document, scope: &FindScope) -> Result<Vec<(&'a dtool_core::Story, usize, usize)>, CommandError> {
    let pick = |id: u64| doc.stories.iter().find(|s| s.id.0 == id).ok_or_else(CommandError::not_found);
    Ok(match scope {
        FindScope::Document => doc.stories.iter().map(|s| (s, 0, s.text.chars().count())).collect(),
        FindScope::Story(id) => {
            let story = pick(*id)?;
            vec![(story, 0, story.text.chars().count())]
        }
        FindScope::Selection { story, start, end } => {
            let story = pick(*story)?;
            if start > end || *end > story.text.chars().count() {
                return Err(CommandError::invalid_params());
            }
            vec![(story, *start, *end)]
        }
    })
}

fn each_match(doc: &Document, query: &FindQuery, scope: &FindScope, flags: FindFlags, format: &FormatQuery, mut f: impl FnMut(&dtool_core::Story, FindMatch, Option<&Captures>)) -> Result<(), CommandError> {
    if query.pattern.is_empty() && !format.is_empty() {
        for (story, lo, hi) in scoped_stories(doc, scope)? {
            let mut at = lo;
            while at < hi {
                if !(char_ok(story, format, at) && para_ok(story, format, at)) {
                    at += 1;
                    continue;
                }
                let start = at;
                while at < hi && char_ok(story, format, at) && (at == start || para_ok(story, format, at)) {
                    at += 1;
                }
                f(story, FindMatch { story: story.id.0, start, end: at }, None);
            }
        }
        return Ok(());
    }
    let re = query.compile()?;
    for (story, lo, hi) in scoped_stories(doc, scope)? {
        let (Some(lo_b), Some(hi_b)) = (byte_at(&story.text, lo), byte_at(&story.text, hi)) else {
            return Err(CommandError::invalid_params());
        };
        let window = &story.text[lo_b..hi_b];
        for caps in re.captures_iter(window) {
            let whole = caps.get(0).expect("group 0");
            if whole.start() == whole.end() {
                continue;
            }
            if flags.whole_word && !is_whole_word(&story.text, lo_b + whole.start(), lo_b + whole.end()) {
                continue;
            }
            let start = scalar_at(&story.text, lo_b + whole.start());
            let end = scalar_at(&story.text, lo_b + whole.end());
            if !format.is_empty() && !format_ok(story, format, start, end) {
                continue;
            }
            f(story, FindMatch { story: story.id.0, start, end }, Some(&caps));
        }
    }
    Ok(())
}

pub fn find_all(doc: &Document, query: &FindQuery, scope: &FindScope) -> Result<Vec<FindMatch>, CommandError> {
    find_all_with(doc, query, scope, FindFlags::default())
}

pub fn find_all_with(doc: &Document, query: &FindQuery, scope: &FindScope, flags: FindFlags) -> Result<Vec<FindMatch>, CommandError> {
    find_all_fmt(doc, query, scope, flags, &FormatQuery::default())
}

pub fn find_all_fmt(doc: &Document, query: &FindQuery, scope: &FindScope, flags: FindFlags, format: &FormatQuery) -> Result<Vec<FindMatch>, CommandError> {
    let mut out = Vec::new();
    each_match(doc, query, scope, flags, format, |_, m, _| out.push(m))?;
    Ok(out)
}

fn story_order(doc: &Document, story: u64) -> usize {
    doc.stories.iter().position(|s| s.id.0 == story).unwrap_or(usize::MAX)
}

pub fn find_next(doc: &Document, query: &FindQuery, scope: &FindScope, from: Option<(u64, usize)>, backwards: bool) -> Result<Option<FindMatch>, CommandError> {
    find_next_with(doc, query, scope, from, backwards, FindFlags::default())
}

pub fn find_next_with(doc: &Document, query: &FindQuery, scope: &FindScope, from: Option<(u64, usize)>, backwards: bool, flags: FindFlags) -> Result<Option<FindMatch>, CommandError> {
    find_next_fmt(doc, query, scope, from, backwards, flags, &FormatQuery::default())
}

pub fn find_next_fmt(doc: &Document, query: &FindQuery, scope: &FindScope, from: Option<(u64, usize)>, backwards: bool, flags: FindFlags, format: &FormatQuery) -> Result<Option<FindMatch>, CommandError> {
    let all = find_all_fmt(doc, query, scope, flags, format)?;
    let key = |m: &FindMatch| (story_order(doc, m.story), m.start);
    let Some((story, at)) = from else {
        return Ok(if backwards { all.last().copied() } else { all.first().copied() });
    };
    let here = (story_order(doc, story), at);
    let found = if backwards {
        all.iter().rev().find(|m| key(m) < here).or(all.last())
    } else {
        all.iter().find(|m| key(m) >= here).or(all.first())
    };
    Ok(found.copied())
}

fn edit_commands(doc: &Document, m: &FindMatch, replacement: Option<&str>, format: Option<&ReplaceFormat>) -> Vec<Command> {
    let mut out = Vec::new();
    let end = match replacement {
        Some(text) => {
            out.push(dtool_core::text_cmd::range_replace_command(StoryId(m.story), m.start, m.end, text));
            let end = m.start + text.chars().count();
            if end > m.start {
                if let Some(story) = doc.stories.iter().find(|story| story.id.0 == m.story) {
                    if let Some(source) = run_at(&story.ranges, m.start).and_then(Value::as_object) {
                        // Deleting a complete run otherwise makes insertion inherit its neighbour.
                        // Clear properties absent from the matched run before restoring its style.
                        let mut patch = Map::new();
                        for run in &story.ranges {
                            if let Some(run) = run.as_object() {
                                for key in run.keys().filter(|key| !matches!(key.as_str(), "start" | "end" | "frame")) { patch.insert(key.clone(), Value::Null); }
                            }
                        }
                        for (key, value) in source.iter().filter(|(key, _)| !matches!(key.as_str(), "start" | "end" | "frame")) { patch.insert(key.clone(), value.clone()); }
                        out.push(Command::with_params("text.style", json!({"story": m.story, "range": {"start": m.start, "end": end}, "patch": patch})));
                    }
                }
            }
            end
        }
        None => m.end,
    };
    if let Some(format) = format.filter(|_| end > m.start) {
        out.push(Command::with_params("text.style", json!({ "story": m.story, "range": { "start": m.start, "end": end }, "patch": format.patch })));
    }
    out
}

fn planned(doc: &Document, query: &FindQuery, scope: &FindScope, replacement: Option<&str>, only: Option<FindMatch>, flags: FindFlags, format: &FormatQuery) -> Result<Vec<(FindMatch, Option<String>)>, CommandError> {
    let mut plan = Vec::new();
    each_match(doc, query, scope, flags, format, |_, m, caps| {
        if only.is_none_or(|target| target == m) {
            let text = replacement.map(|replacement| {
                let mut text = String::new();
                match caps.filter(|_| query.regex) {
                    Some(caps) => caps.expand(replacement, &mut text),
                    None => text.push_str(replacement),
                }
                text
            });
            plan.push((m, text));
        }
    })?;
    Ok(plan)
}

fn apply_plan(session: &mut Session, mut plan: Vec<(FindMatch, Option<String>)>, format: Option<&ReplaceFormat>) -> Result<usize, CommandError> {
    if plan.is_empty() {
        return Ok(0);
    }
    plan.sort_by(|a, b| (b.0.story, b.0.start).cmp(&(a.0.story, a.0.start)));
    let count = plan.len();
    let commands: Vec<Command> = plan.iter().flat_map(|(m, text)| edit_commands(&session.document, m, text.as_deref(), format)).collect();
    session.execute_batch(commands)?;
    Ok(count)
}

pub fn replace_one(session: &mut Session, query: &FindQuery, scope: &FindScope, target: FindMatch, replacement: &str) -> Result<FindMatch, CommandError> {
    replace_one_with(session, query, scope, target, replacement, FindFlags::default())
}

pub fn replace_one_with(session: &mut Session, query: &FindQuery, scope: &FindScope, target: FindMatch, replacement: &str, flags: FindFlags) -> Result<FindMatch, CommandError> {
    replace_one_fmt(session, query, scope, target, Some(replacement), flags, &FormatQuery::default(), None)
}

#[allow(clippy::too_many_arguments)]
pub fn replace_one_fmt(session: &mut Session, query: &FindQuery, scope: &FindScope, target: FindMatch, replacement: Option<&str>, flags: FindFlags, format: &FormatQuery, apply: Option<&ReplaceFormat>) -> Result<FindMatch, CommandError> {
    if replacement.is_none() && apply.is_none() {
        return Err(CommandError::invalid_params());
    }
    let plan = planned(&session.document, query, scope, replacement, Some(target), flags, format)?;
    let Some((m, text)) = plan.first().cloned() else {
        return Err(CommandError::new("STALE"));
    };
    apply_plan(session, plan, apply)?;
    let end = text.map_or(m.end, |text| m.start + text.chars().count());
    Ok(FindMatch { story: m.story, start: m.start, end })
}

pub fn replace_all(session: &mut Session, query: &FindQuery, scope: &FindScope, replacement: &str) -> Result<usize, CommandError> {
    replace_all_with(session, query, scope, replacement, FindFlags::default())
}

pub fn replace_all_with(session: &mut Session, query: &FindQuery, scope: &FindScope, replacement: &str, flags: FindFlags) -> Result<usize, CommandError> {
    replace_all_fmt(session, query, scope, Some(replacement), flags, &FormatQuery::default(), None)
}

pub fn replace_all_fmt(session: &mut Session, query: &FindQuery, scope: &FindScope, replacement: Option<&str>, flags: FindFlags, format: &FormatQuery, apply: Option<&ReplaceFormat>) -> Result<usize, CommandError> {
    if replacement.is_none() && apply.is_none() {
        return Err(CommandError::invalid_params());
    }
    let plan = planned(&session.document, query, scope, replacement, None, flags, format)?;
    apply_plan(session, plan, apply)
}
