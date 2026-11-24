use dtool_text::{caret_at_point, layout_story, CharStyle, FontLibrary, Run, Story, TextRange};

fn two_families(lib: &FontLibrary) -> Option<(String, String)> {
    let usable: Vec<String> = lib.families().into_iter().map(str::to_owned).filter(|family| lib.resolve(family, "Regular").is_some()).collect();
    let pick = |names: &[&str]| names.iter().find_map(|name| usable.iter().find(|have| have.eq_ignore_ascii_case(name)).cloned());
    let first = pick(&["Arial", "Segoe UI", "Noto Sans", "DejaVu Sans", "Liberation Sans"]).or_else(|| usable.first().cloned())?;
    let second = pick(&["Times New Roman", "Georgia", "Cambria", "Noto Serif", "DejaVu Serif", "Liberation Serif"]).filter(|name| *name != first).or_else(|| usable.iter().find(|name| **name != first).cloned())?;
    Some((first, second))
}

fn style(family: &str) -> CharStyle {
    CharStyle { family: family.to_string(), style: "Regular".into(), size_pt: 40.0, ..CharStyle::default() }
}

fn story(text: &str, runs: &[(usize, usize, &str)]) -> Story {
    Story { text: text.into(), chars: runs.iter().map(|(s, e, f)| Run { range: TextRange::new(*s, *e), style: style(f) }).collect(), ..Story::default() }
}

#[test]
fn c10_mixed_runs_in_one_line_are_shaped_with_their_own_face() {
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mixed = layout_story(&lib, &story("Hamburgs", &[(0, 4, &a), (4, 8, &b)]), None);
    let only_a = layout_story(&lib, &story("Hamburgs", &[(0, 8, &a)]), None);
    let only_b = layout_story(&lib, &story("Hamburgs", &[(0, 8, &b)]), None);
    assert_eq!(mixed.len(), 1);
    let glyphs = &mixed[0].glyphs;
    assert_eq!(glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
    let font_a = only_a[0].glyphs[0].font;
    let font_b = only_b[0].glyphs[0].font;
    assert_ne!(font_a, font_b);
    assert!(glyphs[..4].iter().all(|g| g.font == font_a));
    assert!(glyphs[4..].iter().all(|g| g.font == font_b));
    assert_eq!(glyphs[..4].iter().map(|g| (g.gid, g.advance)).collect::<Vec<_>>(), only_a[0].glyphs[..4].iter().map(|g| (g.gid, g.advance)).collect::<Vec<_>>());
    let width: f32 = glyphs.iter().map(|g| g.advance).sum();
    assert!((mixed[0].width - width).abs() < 0.01, "{} vs {}", mixed[0].width, width);
    for x in [0.5f32, 30.0, 60.0, 90.0, 400.0] {
        let at = [x, mixed[0].baseline];
        assert_eq!(caret_at_point(&mixed, at), caret_at_point(&only_a, at));
    }
}

#[test]
fn c10_uniform_story_layout_is_unchanged_and_mixed_story_wraps_by_run_widths() {
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let text = "alpha beta gamma delta epsilon zeta eta theta";
    let uniform = layout_story(&lib, &story(text, &[(0, text.chars().count(), &a)]), Some(200.0));
    assert!(uniform.len() > 1);
    let same = layout_story(&lib, &story(text, &[(0, 20, &a), (20, text.chars().count(), &a)]), Some(200.0));
    assert_eq!(uniform.iter().map(|l| (l.range, l.width.to_bits())).collect::<Vec<_>>(), same.iter().map(|l| (l.range, l.width.to_bits())).collect::<Vec<_>>());
    let mixed = layout_story(&lib, &story(text, &[(0, 20, &a), (20, text.chars().count(), &b)]), Some(200.0));
    assert!(mixed.iter().all(|line| line.width <= 200.0 + 0.01));
    assert_eq!(mixed.first().unwrap().range.start, 0);
    assert_eq!(mixed.last().unwrap().range.end, text.chars().count());
    for pair in mixed.windows(2) {
        assert_eq!(pair[0].range.end, pair[1].range.start);
    }
}