pub use crate::para_style::{BreakKind, Special};

pub fn resolve_special_char(what: &str) -> Option<char> {
    match what {
        "em_dash" => Some('\u{2014}'),
        "en_dash" => Some('\u{2013}'),
        "figure_dash" => Some('\u{2012}'),
        "soft_hyphen" => Some('\u{00AD}'),
        "nb_hyphen" => Some('\u{2011}'),
        "zw_space" => Some('\u{200B}'),
        "zwnj" => Some('\u{200C}'),
        "em_space" => Some('\u{2003}'),
        "en_space" => Some('\u{2002}'),
        "thin_space" => Some('\u{2009}'),
        "hair_space" => Some('\u{200A}'),
        "figure_space" => Some('\u{2007}'),
        "punct_space" => Some('\u{2008}'),
        "line_break" => Some('\u{2028}'),
        "para_break" => Some('\u{2029}'),
        "nbsp" => Some('\u{00A0}'),
        _ => None,
    }
}

pub fn is_break_special(what: &str) -> bool {
    matches!(what, "column_break" | "frame_break" | "page_break" | "odd_page_break" | "even_page_break")
}

pub fn special_from_name(what: &str) -> Option<Special> {
    match what {
        "column_break" => Some(Special::Break(BreakKind::Column)),
        "frame_break" => Some(Special::Break(BreakKind::Frame)),
        "page_break" => Some(Special::Break(BreakKind::Page)),
        "odd_page_break" => Some(Special::Break(BreakKind::OddPage)),
        "even_page_break" => Some(Special::Break(BreakKind::EvenPage)),
        "line_break" => Some(Special::Break(BreakKind::Line)),
        "indent_to_here" | "indent_here" => Some(Special::IndentToHere),
        "right_indent_tab" => Some(Special::RightIndentTab),
        _ => None,
    }
}

pub fn special_menu_items() -> Vec<(&'static str, &'static str)> {
    vec![
        ("insert.dashes", "dashes"),
        ("insert.em_dash", "em_dash"),
        ("insert.en_dash", "en_dash"),
        ("insert.figure_dash", "figure_dash"),
        ("insert.soft_hyphen", "soft_hyphen"),
        ("insert.nb_hyphen", "nb_hyphen"),
        ("insert.zw_space", "zw_space"),
        ("insert.zwnj", "zwnj"),
        ("insert.em_space", "em_space"),
        ("insert.en_space", "en_space"),
        ("insert.thin_space", "thin_space"),
        ("insert.hair_space", "hair_space"),
        ("insert.figure_space", "figure_space"),
        ("insert.punct_space", "punct_space"),
        ("insert.nbsp", "nbsp"),
        ("insert.line_break", "line_break"),
        ("insert.para_break", "para_break"),
    ]
}

pub fn break_menu_items() -> Vec<(&'static str, &'static str)> {
    vec![
        ("insert.column_break", "column_break"),
        ("insert.frame_break", "frame_break"),
        ("insert.page_break", "page_break"),
        ("insert.odd_page_break", "odd_page_break"),
        ("insert.even_page_break", "even_page_break"),
    ]
}

pub fn space_tab_menu_items() -> Vec<(&'static str, &'static str)> {
    vec![
        ("insert.indent_here", "indent_to_here"),
        ("insert.right_indent_tab", "right_indent_tab"),
    ]
}

pub fn special_json(what: &str) -> Option<String> {
    resolve_special_char(what).map(|c| c.to_string())
}
