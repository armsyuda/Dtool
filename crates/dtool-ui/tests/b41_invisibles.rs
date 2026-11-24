use std::sync::Arc;
use dtool_core::serde_json::json;
use dtool_core::{Command, Scope, SharedServices, Workspace};
use dtool_ui::app_canvas::{invisible_glyph, invisible_marks};

#[test]
fn b41_t020_marks_cover_spaces_tabs_breaks_and_para_end() {
    assert_eq!(invisible_marks("a b\tc\u{00A0}d\u{2028}", true), vec![(1, '\u{00B7}'), (3, '\u{2192}'), (5, '\u{00B0}'), (7, '\u{00AC}'), (8, '\u{00B6}')]);
    assert_eq!(invisible_marks("ab", false), Vec::new());
    assert_eq!(invisible_marks("", true), vec![(0, '\u{00B6}')]);
    assert_eq!(invisible_glyph('\u{2003}'), Some('\u{00B7}'));
    assert_eq!(invisible_glyph('\u{3000}'), Some('\u{00B7}'));
    assert_eq!(invisible_glyph('\u{00AD}'), Some('-'));
    assert_eq!(invisible_glyph('\u{200B}'), Some('|'));
    assert_eq!(invisible_glyph('\u{2029}'), Some('\u{00B6}'));
    assert_eq!(invisible_glyph('x'), None);
    assert_eq!(invisible_marks("\u{AC00} \u{AC01}", false), vec![(1, '\u{00B7}')]);
}

#[test]
fn b41_t020_toggle_is_a_live_view_flag() {
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let id = ws.new_document("inv");
    assert!(!ws.get(id).unwrap().session.view.canvas_flags.show_invisibles);
    let on = ws.run(Scope::of(id), Command::with_params("view.flags.set", json!({"show_invisibles": true}))).1.unwrap();
    assert_eq!(on["flags"]["show_invisibles"], true);
    assert!(ws.get(id).unwrap().session.view.canvas_flags.show_invisibles);
    let other = ws.new_document("other");
    assert!(!ws.get(other).unwrap().session.view.canvas_flags.show_invisibles);
    let q = ws.run(Scope::of(id), Command::with_params("view.flags.query", json!({}))).1.unwrap();
    assert_eq!(q["flags"]["show_invisibles"], true);
    ws.run(Scope::of(id), Command::with_params("view.flags.set", json!({"show_invisibles": false}))).1.unwrap();
    assert!(!ws.get(id).unwrap().session.view.canvas_flags.show_invisibles);
    assert!(ws.run(Scope::of(id), Command::with_params("view.flags.set", json!({"show_invisibles": "yes"}))).1.is_err());
}
