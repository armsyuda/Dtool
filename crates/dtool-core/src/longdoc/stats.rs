use crate::command::Session;
use crate::document::Document;
use dtool_protocol::{Command, CommandError, CommandResult};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocStats {
    pub words: u64,
    pub characters: u64,
    pub characters_with_spaces: u64,
    pub paragraphs: u64,
    pub lines: u64,
}

fn compute_stats(text: &str) -> DocStats {
    let mut stats = DocStats::default();
    if text.is_empty() {
        return stats;
    }
    let mut in_word = false;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            in_word = false;
            continue;
        }
        stats.characters_with_spaces += 1;
        if ch.is_whitespace() {
            in_word = false;
        } else {
            stats.characters += 1;
            if !in_word {
                stats.words += 1;
                in_word = true;
            }
        }
    }
    for line in text.lines() {
        stats.lines += 1;
        if !line.trim().is_empty() {
            stats.paragraphs += 1;
        }
    }
    stats
}

pub fn doc_stats(doc: &Document, selection: Option<&str>) -> Result<DocStats, CommandError> {
    let text = match selection {
        Some(sel) => sel.to_string(),
        None => doc.stories.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n"),
    };
    Ok(compute_stats(&text))
}

pub fn stats_json(stats: &DocStats) -> serde_json::Value {
    serde_json::json!({
        "words": stats.words,
        "characters": stats.characters,
        "characters_with_spaces": stats.characters_with_spaces,
        "paragraphs": stats.paragraphs,
        "lines": stats.lines
    })
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    (command.name == "doc.stats").then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if command.name != "doc.stats" {
        return None;
    }
    Some((|| {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        let selection = command.params.get("selection").and_then(|v| v.as_str());
        let stats = doc_stats(&session.document, selection)?;
        Ok(stats_json(&stats))
    })())
}
