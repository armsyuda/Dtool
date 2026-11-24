use dtool_core::serde_json::json;
use dtool_core::{Command, CommandError, CommandResult, Session};
use dtool_tools::dtool_text::special::{is_break_special, resolve_special_char};

pub const MENU_PREFIX: &str = "text.insert.";

const MENU_SPECIALS: &[(&str, &str)] = &[
    ("dashes_and_hyphens.em_dash", "em_dash"),
    ("dashes_and_hyphens.en_dash", "en_dash"),
    ("dashes_and_hyphens.figure_dash", "figure_dash"),
    ("dashes_and_hyphens.soft_hyphen", "soft_hyphen"),
    ("dashes_and_hyphens.non_breaking_hyphen", "nb_hyphen"),
    ("spaces_and_tabs.em_space", "em_space"),
    ("spaces_and_tabs.en_space", "en_space"),
    ("spaces_and_tabs.non_breaking_space", "nbsp"),
    ("spaces_and_tabs.zero_width_space", "zw_space"),
    ("spaces_and_tabs.hair_space", "hair_space"),
    ("spaces_and_tabs.thin_space", "thin_space"),
    ("spaces_and_tabs.punctuation_space", "punct_space"),
    ("spaces_and_tabs.figure_space", "figure_space"),
    ("spaces_and_tabs.zero_width_non_joiner", "zwnj"),
    ("spaces_and_tabs.right_indent_tab", "right_indent_tab"),
    ("spaces_and_tabs.indent_to_here", "indent_to_here"),
    ("breaks.line_break", "line_break"),
    ("breaks.column_break", "column_break"),
    ("breaks.frame_break", "frame_break"),
    ("breaks.page_break", "page_break"),
    ("breaks.odd_page_break", "odd_page_break"),
    ("breaks.even_page_break", "even_page_break"),
];

pub fn special_for(menu_id: &str) -> Option<&'static str> {
    let key = menu_id.strip_prefix(MENU_PREFIX)?;
    MENU_SPECIALS.iter().find(|(id, _)| *id == key).map(|(_, what)| *what)
}

fn literal_for(menu_id: &str) -> Option<char> {
    Some(match menu_id.strip_prefix(MENU_PREFIX)? {
        "symbols.bullet" => '•',
        "symbols.dagger" => '†',
        "symbols.double_dagger" => '‡',
        "symbols.ellipsis" => '…',
        "symbols.inverted_question_mark" => '¿',
        "symbols.inverted_exclamation_mark" => '¡',
        "symbols.copyright" => '©',
        "symbols.trademark" => '™',
        "symbols.registered_trademark" => '®',
        "symbols.paragraph_marker" => '¶',
        "symbols.section_marker" => '§',
        "maths.plus_minus" => '±',
        "maths.multiply" => '×',
        "maths.divide" => '÷',
        "maths.approximately_equal" => '≈',
        "maths.not_equal" => '≠',
        "maths.less_than_or_equal" => '≤',
        "maths.greater_than_or_equal" => '≥',
        "maths.degree" => '°',
        "maths.per_mille" => '‰',
        "maths.pi" => 'π',
        "maths.infinity" => '∞',
        "quotation_marks.double_left_quote" => '“',
        "quotation_marks.double_right_quote" => '”',
        "quotation_marks.single_left_quote" => '‘',
        "quotation_marks.single_right_quote" => '’',
        "quotation_marks.double_base_quote" => '„',
        "quotation_marks.single_base_quote" => '‚',
        "quotation_marks.left_guillemet" => '«',
        "quotation_marks.right_guillemet" => '»',
        "quotation_marks.single_left_guillemet" => '‹',
        "quotation_marks.single_right_guillemet" => '›',
        "spaces_and_tabs.narrow_non_breaking_space" => '\u{202F}',
        "spaces_and_tabs.sixth_space" => '\u{2006}',
        "spaces_and_tabs.quarter_space" => '\u{2005}',
        "spaces_and_tabs.third_space" => '\u{2004}',
        "spaces_and_tabs.tab" => '\t',
        _ => return None,
    })
}

pub(crate) fn menu_command(menu_id: &str) -> Option<&'static str> {
    if literal_for(menu_id).is_some() {
        return Some("text.edit.type");
    }
    special_for(menu_id).map(|what| {
        if resolve_special_char(what).is_some() { "text.edit.type" } else { "text.special.insert" }
    })
}

pub fn handles(menu_id: &str) -> bool {
    menu_command(menu_id).is_some()
}

pub fn build_command(story: u64, at: usize, what: &str) -> Option<Command> {
    let mut params = json!({ "story": story, "at": at, "what": what });
    if !is_break_special(what) && !matches!(what, "indent_to_here" | "right_indent_tab") {
        params["char"] = resolve_special_char(what)?.to_string().into();
    }
    Some(Command::with_params("text.special.insert", params))
}

pub fn insert_at_caret(session: &mut Session, menu_id: &str) -> CommandResult {
    if let Some(ch) = literal_for(menu_id).or_else(|| special_for(menu_id).and_then(resolve_special_char)) {
        return session.run(&Command::with_params("text.edit.type", json!({ "text": ch.to_string() })));
    }
    let what = special_for(menu_id).ok_or_else(CommandError::invalid_params)?;
    let edit = session.text_edit.as_ref().ok_or_else(CommandError::invalid_params)?;
    let (story, at) = (edit.story.0, edit.caret);
    dtool_core::text_cmd::commit_text_edit(session);
    session.run(&build_command(story, at, what).ok_or_else(CommandError::invalid_params)?)
}