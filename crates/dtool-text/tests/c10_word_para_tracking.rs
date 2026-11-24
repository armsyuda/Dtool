use dtool_text::char_style::Kerning;
use dtool_text::shape::{apply_letter_spacing, letter_spacing_pt};
use dtool_text::story::{paragraph_range, word_range};
use dtool_text::{layout_story, CharStyle, FontLibrary, Run, Story, TextRange};

fn empty_library() -> FontLibrary {
    FontLibrary::from_fixtures::<&str>(&[]).expect("empty library")
}

fn story(text: &str, style: CharStyle) -> Story {
    let count = text.chars().count();
    Story { text: text.into(), chars: vec![Run { range: TextRange::new(0, count), style }], ..Story::default() }
}

fn style(tracking: f32, kerning: Kerning) -> CharStyle {
    CharStyle { family: String::new(), size_pt: 10.0, tracking, kerning, ..CharStyle::default() }
}

#[test]
fn c10_word_range_uses_scalar_indices() {
    assert_eq!(word_range("one two", 5), TextRange::new(4, 7));
    assert_eq!(word_range("one two", 4), TextRange::new(4, 7));
    assert_eq!(word_range("one two", 7), TextRange::new(4, 7));
    assert_eq!(word_range("one two", 3), TextRange::new(0, 3));
    assert_eq!(word_range("가나 다라", 3), TextRange::new(3, 5));
    assert_eq!(word_range("a  b", 2), TextRange::new(2, 3));
    assert_eq!(word_range("", 4), TextRange::new(0, 0));
    let s = Story { text: "one two".into(), ..Story::default() };
    assert_eq!(s.select_word(5), TextRange::new(4, 7));
}

#[test]
fn c10_paragraph_range_includes_its_separator() {
    let text = "ab\ncd\r\nef";
    assert_eq!(paragraph_range(text, 1), TextRange::new(0, 3));
    assert_eq!(paragraph_range(text, 2), TextRange::new(0, 3));
    assert_eq!(paragraph_range(text, 4), TextRange::new(3, 7));
    assert_eq!(paragraph_range(text, 5), TextRange::new(3, 7));
    assert_eq!(paragraph_range(text, 6), TextRange::new(3, 7));
    assert_eq!(paragraph_range(text, 8), TextRange::new(7, 9));
    assert_eq!(paragraph_range(text, 9), TextRange::new(7, 9));
    assert_eq!(paragraph_range("가\u{2029}나", 2), TextRange::new(2, 3));
    assert_eq!(paragraph_range("ab\n", 3), TextRange::new(3, 3));
    let s = Story { text: text.into(), ..Story::default() };
    assert_eq!(s.select_paragraph(4), TextRange::new(3, 7));
}

#[test]
fn c10_letter_spacing_amounts() {
    assert_eq!(letter_spacing_pt(&style(100.0, Kerning::Auto)), 1.0);
    assert_eq!(letter_spacing_pt(&style(0.0, Kerning::Manual(50.0))), 0.5);
    assert_eq!(letter_spacing_pt(&style(100.0, Kerning::Optical)), 1.0);
    assert_eq!(letter_spacing_pt(&style(f32::NAN, Kerning::Auto)), 0.0);
    let mut glyphs = vec![
        dtool_text::ShapedGlyph { font: dtool_text::FontId(1), gid: 1, cluster: 0, advance: 5.0, offset: [0.0, 0.0] },
        dtool_text::ShapedGlyph { font: dtool_text::FontId(1), gid: 2, cluster: 0, advance: 0.0, offset: [0.0, 0.0] },
        dtool_text::ShapedGlyph { font: dtool_text::FontId(1), gid: 3, cluster: 1, advance: 5.0, offset: [0.0, 0.0] },
    ];
    assert_eq!(apply_letter_spacing(&mut glyphs, 2.0), 2.0);
    assert_eq!(glyphs.iter().map(|g| g.advance).collect::<Vec<_>>(), vec![5.0, 2.0, 5.0]);
}

#[test]
fn c10_tracking_100_widens_ab_by_a_tenth_em() {
    let lib = empty_library();
    let plain = layout_story(&lib, &story("AB", style(0.0, Kerning::Auto)), None);
    let tracked = layout_story(&lib, &story("AB", style(100.0, Kerning::Auto)), None);
    assert_eq!(plain.len(), 1);
    assert_eq!(tracked.len(), 1);
    assert!((tracked[0].width - plain[0].width - 1.0).abs() < 1e-4, "{} vs {}", tracked[0].width, plain[0].width);
    assert!((tracked[0].glyphs[1].advance - plain[0].glyphs[1].advance).abs() < 1e-6);
    let manual = layout_story(&lib, &story("AB", style(0.0, Kerning::Manual(100.0))), None);
    assert!((manual[0].width - plain[0].width - 1.0).abs() < 1e-4);
}

#[test]
fn c10_tracking_is_not_added_after_the_last_glyph_of_a_wrapped_line() {
    let lib = empty_library();
    let text = "aaaa bbbb cccc";
    let plain = layout_story(&lib, &story(text, style(0.0, Kerning::Auto)), Some(40.0));
    let tracked = layout_story(&lib, &story(text, style(200.0, Kerning::Auto)), Some(400.0));
    assert_eq!(tracked.len(), 1);
    let narrow = layout_story(&lib, &story(text, style(200.0, Kerning::Auto)), Some(40.0));
    assert!(narrow.len() >= 2, "{}", narrow.len());
    assert!(plain.len() >= 2);
    let base = plain[0].glyphs[0].advance;
    for line in &narrow {
        let last = line.glyphs.last().unwrap();
        assert!((last.advance - base).abs() < 1e-4, "line end advance {} base {}", last.advance, base);
        let sum: f32 = line.glyphs.iter().map(|g| g.advance).sum();
        assert!((sum - line.width).abs() < 1e-3);
    }
    let count = narrow[0].glyphs.len() as f32;
    assert!((narrow[0].width - (count * base + (count - 1.0) * 2.0)).abs() < 1e-3, "{}", narrow[0].width);
}

#[test]
fn c10_tracking_with_a_real_system_font_widens_by_a_tenth_em() {
    let lib = FontLibrary::system();
    let Some(family) = ["Arial", "Segoe UI", "Malgun Gothic"].into_iter().find(|name| lib.families().contains(name)) else { return };
    let face = lib.faces(family).into_iter().next().map(|face| face.style.clone()).unwrap_or_default();
    let make = |tracking: f32| CharStyle { family: family.to_string(), style: face.clone(), size_pt: 10.0, tracking, ..CharStyle::default() };
    let plain = layout_story(&lib, &story("AB", make(0.0)), None);
    let tracked = layout_story(&lib, &story("AB", make(100.0)), None);
    assert!((tracked[0].width - plain[0].width - 1.0).abs() < 1e-3, "{} vs {}", tracked[0].width, plain[0].width);
}
