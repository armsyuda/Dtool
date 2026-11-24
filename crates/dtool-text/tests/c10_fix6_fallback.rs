use dtool_text::fonts::{fallback_chain, fallback_families, is_hangul, split_by_fallback, FallbackRun, FontLibrary};

#[test]
fn c10_fix6_hangul_ranges_cover_jamo_compat_and_extended() {
    for ch in ['\u{1100}', '\u{11ff}', '\u{3131}', '\u{318e}', '\u{a960}', '\u{a97c}', '\u{ac00}', '\u{d7a3}', '\u{d7b0}', '\u{d7fb}'] {
        assert!(is_hangul(ch), "{:04X}", ch as u32);
        assert_eq!(fallback_families(ch)[0], "Malgun Gothic");
    }
    for ch in ['A', '\u{3000}', '\u{e000}', '\u{1200}'] {
        assert!(!is_hangul(ch), "{:04X}", ch as u32);
        assert_eq!(fallback_families(ch)[0], "Segoe UI");
    }
}

#[test]
fn c10_fix6_a_han_in_arial_splits_into_arial_and_hangul_face() {
    let lib = FontLibrary::system();
    let (Some(arial), Some(_)) = (lib.resolve("Arial", "Regular"), lib.resolve("Malgun Gothic", "Regular")) else {
        eprintln!("skipped: Arial and Malgun Gothic must both be installed");
        return;
    };
    let hangul = fallback_chain(&lib, arial, '\u{d55c}').into_iter().skip(1).find(|id| lib.has_glyph(*id, '\u{d55c}')).expect("a hangul face");
    let split = split_by_fallback(&lib, "Arial", "Regular", "A\u{d55c}");
    assert_eq!(split.runs, vec![FallbackRun { start: 0, end: 1, font: Some(arial) }, FallbackRun { start: 1, end: 2, font: Some(hangul) }]);
    assert!(split.missing_chars.is_empty());
    assert!(!split.missing_font);
    let joined = split_by_fallback(&lib, "Arial", "Regular", "AB \u{d55c}\u{ae00} C");
    assert_eq!(joined.runs.iter().map(|run| (run.start, run.end, run.font)).collect::<Vec<_>>(), vec![(0, 3, Some(arial)), (3, 6, Some(hangul)), (6, 7, Some(arial))]);
}

#[test]
fn c10_fix6_uncovered_chars_are_reported_missing() {
    let lib = FontLibrary::system();
    let Some(arial) = lib.resolve("Arial", "Regular") else {
        eprintln!("skipped: Arial is not installed");
        return;
    };
    let split = split_by_fallback(&lib, "Arial", "Regular", "\u{e000}");
    match fallback_chain(&lib, arial, '\u{e000}').into_iter().find(|id| lib.has_glyph(*id, '\u{e000}')) {
        None => assert_eq!(split.missing_chars, vec![0], "{split:?}"),
        Some(id) => {
            assert!(split.missing_chars.is_empty());
            assert_eq!(split.runs[0].font, Some(id));
        }
    }
    let unassigned = split_by_fallback(&lib, "Arial", "Regular", "A\u{fdd0}B");
    assert_eq!(unassigned.missing_chars, vec![1], "{unassigned:?}");
    assert_eq!(unassigned.runs.len(), 1);
    assert_eq!(unassigned.runs[0].font, Some(arial));
}

#[test]
fn c10_fix6_unknown_family_reports_missing_font() {
    let lib = FontLibrary::system();
    let split = split_by_fallback(&lib, "No Such Family 7f3", "Regular", "A");
    assert!(split.missing_font);
    if let Some(segoe) = lib.resolve("Segoe UI", "Regular") {
        assert!(split.missing_chars.is_empty());
        assert_eq!(split.runs[0].font, Some(segoe));
    }
}

#[test]
fn c10_fix6_empty_text_has_no_runs() {
    let split = split_by_fallback(&FontLibrary::system(), "Arial", "Regular", "");
    assert!(split.runs.is_empty() && split.missing_chars.is_empty());
}