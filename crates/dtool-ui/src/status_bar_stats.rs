use dtool_core::longdoc::DocStats;
use dtool_core::Document;
use dtool_strings::text;

pub fn render_stats(stats: &DocStats) -> String {
    let summary = text("stats.summary")
        .replace("{words}", &stats.words.to_string())
        .replace("{chars}", &stats.characters.to_string())
        .replace("{chars_sp}", &stats.characters_with_spaces.to_string());
    let paragraphs = text("stats.paragraphs").replace("{count}", &stats.paragraphs.to_string());
    let lines = text("stats.lines").replace("{count}", &stats.lines.to_string());
    format!("{} | {} | {}", summary, paragraphs, lines)
}

pub fn document_stats_text(doc: &Document) -> Option<String> {
    if doc.stories.is_empty() {
        return None;
    }
    dtool_core::longdoc::stats::doc_stats(doc, None).ok().map(|stats| render_stats(&stats))
}
