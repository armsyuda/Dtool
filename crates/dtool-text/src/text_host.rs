use crate::font_preview::{font_files, run_font_command, FONT_FILES, FONT_LIST, FONT_PREVIEW, FONT_PREVIEW_CANCEL};
use crate::fonts::FontLibrary;
use crate::text_find::{find_next_fmt, replace_all_fmt, replace_one_fmt, selection_scope, FindFlags, FindMatch, FindQuery, FindScope, FormatQuery, ReplaceFormat, TEXT_FIND, TEXT_REPLACE, TEXT_REPLACE_ALL};
use dtool_core::{Command, CommandError, Session};
use dtool_core::serde_json::{self, json, Value};

pub fn install() -> bool {
    dtool_core::text_cmd::register_font_availability(|family, style| {
        family.eq_ignore_ascii_case("sans-serif") || FontLibrary::shared().resolve(family, style).is_some()
    });
    dtool_core::text_cmd::register_font_coverage(|family, style, characters| {
        let library = FontLibrary::shared();
        library.resolve(family, style).map_or_else(Vec::new, |id| library.unsupported_chars(id, characters))
    });
    dtool_core::commands::text_host::install(run, can_execute)
}

fn format(params: &Value) -> Result<FormatQuery, CommandError> {
    FormatQuery::from_value(params.get("format"))
}

fn query(params: &Value) -> Result<FindQuery, CommandError> {
    let by_format = !format(params)?.is_empty();
    let pattern = match params.get("pattern") {
        None | Some(Value::Null) if by_format => "",
        Some(Value::String(p)) if p.is_empty() && by_format => "",
        other => other.and_then(Value::as_str).filter(|p| !p.is_empty()).ok_or_else(CommandError::invalid_params)?,
    };
    let flag = |key: &str, default: bool| match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(v)) => Ok(*v),
        Some(_) => Err(CommandError::invalid_params()),
    };
    Ok(FindQuery { pattern: pattern.to_owned(), regex: flag("regex", false)?, case_sensitive: flag("case_sensitive", true)? })
}

fn flags(params: &Value) -> Result<FindFlags, CommandError> {
    match params.get("whole_word") {
        None | Some(Value::Null) => Ok(FindFlags { whole_word: false }),
        Some(Value::Bool(v)) => Ok(FindFlags { whole_word: *v }),
        Some(_) => Err(CommandError::invalid_params()),
    }
}

fn scope(session: &Session, params: &Value) -> Result<FindScope, CommandError> {
    match params.get("scope").and_then(Value::as_str).unwrap_or("document") {
        "document" => Ok(FindScope::Document),
        "story" => params.get("story").and_then(Value::as_u64).map(FindScope::Story).ok_or_else(CommandError::invalid_params),
        "selection" => selection_scope(session).ok_or_else(CommandError::invalid_params),
        _ => Err(CommandError::invalid_params()),
    }
}

fn span(value: Option<&Value>) -> Option<FindMatch> {
    let value = value?;
    let get = |key: &str| value.get(key).and_then(Value::as_u64);
    Some(FindMatch { story: get("story")?, start: get("start")? as usize, end: get("end")? as usize })
}

fn match_json(m: Option<FindMatch>) -> Value {
    m.map_or(Value::Null, |m| json!({ "story": m.story, "start": m.start, "end": m.end }))
}

/// `replacement` may be omitted when `replace_format` is given (format-only replace keeps the matched text).
fn replacement(params: &Value) -> Result<Option<&str>, CommandError> {
    match params.get("replacement") {
        None | Some(Value::Null) if replace_format_given(params) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.as_str())),
        _ => Err(CommandError::invalid_params()),
    }
}

fn replace_format_given(params: &Value) -> bool {
    params.get("replace_format").is_some_and(|v| !v.is_null())
}

fn find(session: &Session, params: &Value) -> Result<Value, CommandError> {
    let query = query(params)?;
    let scope = scope(session, params)?;
    let from = match params.get("from") {
        None | Some(Value::Null) => None,
        Some(from) => Some(from.get("story").and_then(Value::as_u64).zip(from.get("at").and_then(Value::as_u64)).map(|(s, a)| (s, a as usize)).ok_or_else(CommandError::invalid_params)?),
    };
    let backwards = params.get("backwards").and_then(Value::as_bool).unwrap_or(false);
    let found = find_next_fmt(&session.document, &query, &scope, from, backwards, flags(params)?, &format(params)?)?;
    Ok(json!({ "match": match_json(found) }))
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<Value, CommandError>> {
    if let Some(reply) = crate::spelling::run_spell_command(session, command) {
        return Some(reply);
    }
    let params = &command.params;
    let name = command.name.as_str();
    if name == FONT_FILES {
        return Some(Ok(font_files(&FontLibrary::shared(), params)));
    }
    if [FONT_LIST, FONT_PREVIEW, FONT_PREVIEW_CANCEL].contains(&name) {
        if name != FONT_LIST && !session.has_document { return Some(Err(CommandError::no_document())); }
        let lib = FontLibrary::shared();
        return run_font_command(&lib, &session.document, command).map(|reply| {
            let mut value = reply.and_then(|reply| serde_json::to_value(reply).map_err(|_| CommandError::new("INTERNAL_ERROR")))?;
            if name == FONT_LIST { value["recent"] = json!(dtool_core::commands::text_host::recent_fonts()); }
            Ok(value)
        });
    }
    if ![TEXT_FIND, TEXT_REPLACE, TEXT_REPLACE_ALL].contains(&name) { return None; }
    if !session.has_document { return Some(Err(CommandError::no_document())); }
    Some((|| match name {
        TEXT_FIND => find(session, params),
        TEXT_REPLACE => {
            let (query, scope, text, flags, find_format) = (query(params)?, scope(session, params)?, replacement(params)?.map(str::to_owned), flags(params)?, format(params)?);
            let apply = ReplaceFormat::from_value(&session.document, params.get("replace_format"))?;
            let target = span(params.get("target")).ok_or_else(CommandError::invalid_params)?;
            let replaced = replace_one_fmt(session, &query, &scope, target, text.as_deref(), flags, &find_format, apply.as_ref())?;
            Ok(json!({ "match": match_json(Some(replaced)), "doc_version": session.generation }))
        }
        _ => {
            let (query, scope, text, flags, find_format) = (query(params)?, scope(session, params)?, replacement(params)?.map(str::to_owned), flags(params)?, format(params)?);
            let apply = ReplaceFormat::from_value(&session.document, params.get("replace_format"))?;
            let count = replace_all_fmt(session, &query, &scope, text.as_deref(), flags, &find_format, apply.as_ref())?;
            Ok(json!({ "replaced": count, "doc_version": session.generation }))
        }
    })())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if let Some(value) = crate::spelling::can_spell(session, command) {
        return Some(value);
    }
    let params = &command.params;
    Some(match command.name.as_str() {
        FONT_LIST | FONT_FILES => true,
        FONT_PREVIEW => session.has_document && params.get("family").and_then(Value::as_str).is_some()
            && (params.get("story").and_then(Value::as_u64).is_some()
                || params.get("targets").and_then(Value::as_array).is_some_and(|targets| !targets.is_empty())),
        FONT_PREVIEW_CANCEL => true,
        TEXT_FIND => session.has_document && query(params).is_ok() && flags(params).is_ok(),
        TEXT_REPLACE => session.has_document && query(params).is_ok() && flags(params).is_ok() && replacement(params).is_ok() && span(params.get("target")).is_some(),
        TEXT_REPLACE_ALL => session.has_document && query(params).is_ok() && flags(params).is_ok() && replacement(params).is_ok(),
        _ => return None,
    })
}
