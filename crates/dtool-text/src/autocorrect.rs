use dtool_core::prefs::Prefs;
use dtool_core::{Command, CommandError, Session, StoryId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoCorrect {
    pub pairs: Vec<(String, String)>,
    pub smart_quotes: bool,
    pub cap_first: bool,
}

impl Default for AutoCorrect {
    fn default() -> Self {
        Self {
            pairs: vec![
                ("(c)".to_string(), "©".to_string()),
                ("(r)".to_string(), "®".to_string()),
                ("(tm)".to_string(), "™".to_string()),
                ("...".to_string(), "…".to_string()),
            ],
            smart_quotes: true,
            cap_first: true,
        }
    }
}

pub fn apply_autocorrect(text: &str, config: &AutoCorrect) -> String {
    let mut result = text.to_string();

    for (from, to) in &config.pairs {
        if !from.is_empty() && result.ends_with(from.as_str()) {
            let len = from.len();
            result.truncate(result.len() - len);
            result.push_str(to);
            break;
        }
    }

    if config.smart_quotes {
        result = apply_smart_quotes(&result);
    }

    if config.cap_first {
        result = apply_cap_first(&result);
    }

    result
}

fn apply_smart_quotes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    for i in 0..len {
        let c = chars[i];
        if c == '"' {
            let prev_is_opening = i > 0 && (chars[i - 1] == ' ' || chars[i - 1] == '\n' || chars[i - 1] == '(');
            if prev_is_opening || i == 0 {
                result.push('“');
            } else {
                result.push('”');
            }
        } else if c == '\'' {
            let prev_is_word = i > 0 && chars[i - 1].is_alphanumeric();
            if prev_is_word {
                result.push('’');
            } else {
                result.push('‘');
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn apply_cap_first(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut start_of_sentence = true;

    for i in 0..len {
        let c = chars[i];
        if start_of_sentence && c.is_alphabetic() {
            result.push(c.to_uppercase().next().unwrap_or(c));
            start_of_sentence = false;
        } else {
            result.push(c);
            if c == '.' || c == '!' || c == '?' {
                start_of_sentence = true;
            } else if !c.is_whitespace() && !c.is_alphabetic() {
                start_of_sentence = false;
            }
        }
    }

    result
}

pub fn is_word_end(text: &str) -> bool {
    text.ends_with(' ') || text.ends_with('\n') || text.ends_with('.') || text.ends_with(',')
}

pub const WORD_END: &[char] = &[' ', '\n', '\t', '\u{2029}', '.', ',', ';', ':', '!', '?'];

impl AutoCorrect {
    pub fn from_prefs(prefs: &Prefs) -> Option<Self> {
        if !prefs.autocorrect_enabled || !prefs.ac_replace_while_typing {
            return None;
        }
        Some(Self {
            pairs: prefs.ac_replacements.iter().filter(|rule| !rule.replace.is_empty()).map(|rule| (rule.replace.clone(), rule.with.clone())).collect(),
            smart_quotes: prefs.ac_typographic_quotes,
            cap_first: prefs.ac_capitalise_sentences,
        })
    }
}

pub fn is_trigger(inserted: &str) -> bool {
    let mut chars = inserted.chars();
    matches!((chars.next(), chars.next()), (Some(ch), None) if WORD_END.contains(&ch))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    pub start: usize,
    pub end: usize,
    pub with: String,
}

pub fn pending_expansion(text: &str, caret: usize, config: &AutoCorrect) -> Option<Expansion> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    if caret == 0 || !WORD_END.contains(&chars[caret - 1]) {
        return None;
    }
    let end = caret - 1;
    let start = chars[..end].iter().rposition(|ch| ch.is_whitespace()).map(|i| i + 1).unwrap_or(0);
    let word: String = chars[start..end].iter().collect();
    if word.is_empty() {
        return None;
    }
    config.pairs.iter().find_map(|(from, to)| {
        let exact = word == *from;
        let symbol_suffix = !exact && from.chars().next().is_some_and(|ch| !ch.is_alphanumeric()) && word.ends_with(from.as_str());
        (exact || symbol_suffix).then(|| Expansion { start: end - from.chars().count(), end, with: to.clone() })
    })
}

pub fn expand_after_insert(session: &mut Session, story: StoryId, caret: usize, inserted: &str, prefs: &Prefs) -> Result<bool, CommandError> {
    if !is_trigger(inserted) {
        return Ok(false);
    }
    let Some(config) = AutoCorrect::from_prefs(prefs) else { return Ok(false) };
    let text = session.document.stories.iter().find(|s| s.id == story).ok_or_else(CommandError::not_found)?.text.clone();
    let Some(expansion) = pending_expansion(&text, caret, &config) else { return Ok(false) };
    session.execute_batch(expansion_commands(story, &expansion))?;
    let removed = expansion.end - expansion.start;
    let added = expansion.with.chars().count();
    if let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story) {
        for at in [&mut edit.caret, &mut edit.anchor] {
            if *at >= expansion.end {
                *at = *at + added - removed;
            }
        }
    }
    Ok(true)
}

fn expansion_commands(story: StoryId, expansion: &Expansion) -> Vec<Command> {
    let mut delete = Command::new("text.delete");
    delete.params["story"] = story.0.into();
    delete.params["range"]["start"] = expansion.start.into();
    delete.params["range"]["end"] = expansion.end.into();
    let mut insert = Command::new("text.insert");
    insert.params["story"] = story.0.into();
    insert.params["at"] = expansion.start.into();
    insert.params["text"] = expansion.with.clone().into();
    if expansion.with.is_empty() { vec![delete] } else { vec![delete, insert] }
}

pub fn type_with_autocorrect(session: &mut Session, story: StoryId, at: usize, inserted: &str, prefs: &Prefs) -> Result<bool, CommandError> {
    if inserted.is_empty() || inserted.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\t') {
        return Ok(false);
    }
    let text = session.document.stories.iter().find(|s| s.id == story).ok_or_else(CommandError::not_found)?.text.clone();
    let mut chars: Vec<char> = text.chars().collect();
    if at > chars.len() {
        return Err(CommandError::invalid_params());
    }
    let mut typed = Command::new("text.insert");
    typed.params["story"] = story.0.into();
    typed.params["at"] = at.into();
    typed.params["text"] = inserted.into();
    let expansion = AutoCorrect::from_prefs(prefs).filter(|_| is_trigger(inserted)).and_then(|config| {
        chars.splice(at..at, inserted.chars());
        let after: String = chars.iter().collect();
        pending_expansion(&after, at + inserted.chars().count(), &config)
    });
    match expansion {
        Some(expansion) => {
            let mut commands = vec![typed];
            commands.extend(expansion_commands(story, &expansion));
            session.execute_batch(commands)?;
            let typed_len = inserted.chars().count();
            let removed = expansion.end - expansion.start;
            let added = expansion.with.chars().count();
            if let Some(edit) = session.text_edit.as_mut().filter(|edit| edit.story == story) {
                for pos in [&mut edit.caret, &mut edit.anchor] {
                    if *pos >= at {
                        *pos += typed_len;
                    }
                    if *pos >= expansion.end {
                        *pos = *pos + added - removed;
                    }
                }
            }
            Ok(true)
        }
        None => {
            session.run(&typed)?;
            Ok(false)
        }
    }
}

pub fn expand_in_edit(session: &mut Session, inserted: &str, prefs: &Prefs) -> Result<bool, CommandError> {
    if !is_trigger(inserted) {
        return Ok(false);
    }
    let Some(config) = AutoCorrect::from_prefs(prefs) else { return Ok(false) };
    let Some((story, caret)) = session.text_edit.as_ref().filter(|edit| edit.caret == edit.anchor).map(|edit| (edit.story, edit.caret)) else {
        return Ok(false);
    };
    let text = session.document.stories.iter().find(|s| s.id == story).ok_or_else(CommandError::not_found)?.text.clone();
    let Some(expansion) = pending_expansion(&text, caret, &config) else { return Ok(false) };
    if let Some(edit) = session.text_edit.as_mut() {
        edit.anchor = expansion.start;
        edit.caret = expansion.end;
    }
    let mut typed = Command::new("text.edit.type");
    typed.params["text"] = expansion.with.clone().into();
    let result = session.run(&typed);
    let after = if result.is_ok() { caret + expansion.with.chars().count() - (expansion.end - expansion.start) } else { caret };
    if let Some(edit) = session.text_edit.as_mut() {
        edit.anchor = after;
        edit.caret = after;
    }
    result.map(|_| true)
}