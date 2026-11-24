use dtool_text::char_style::Caps;
use dtool_text::shape::{shape_styled, ShapeOptions};
use dtool_text::{layout_story, CharPatch, CharStyle, FontLibrary, Run, Story, TextRange};

fn fingerprint(style: &CharStyle, text: &str, lib: &FontLibrary) -> (Vec<(u16, usize)>, usize, i32) {
    let run = shape_styled(lib, text, style);
    let glyphs: Vec<(u16, usize)> = run.glyphs.iter().map(|g| (g.gid, g.cluster)).collect();
    let width = (run.width * 100.0).round() as i32;
    (glyphs.clone(), glyphs.len(), width)
}

fn prefers_family(lib: &FontLibrary) -> Vec<String> {
    let preferred = [
        "Pretendard",
        "Inter",
        "Segoe UI",
        "Calibri",
        "Cambria",
        "Times New Roman",
        "Georgia",
        "Arial",
        "Noto Serif",
        "Noto Sans",
        "Liberation Serif",
        "Source Serif 4",
    ];
    let have: Vec<String> = lib.families().into_iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    for name in preferred {
        if have.iter().any(|h| h.eq_ignore_ascii_case(name)) {
            let canon = have.iter().find(|h| h.eq_ignore_ascii_case(name)).unwrap().clone();
            out.push(canon);
        }
    }
    for family in have {
        if !out.iter().any(|h| h.eq_ignore_ascii_case(&family)) {
            out.push(family);
        }
    }
    out
}

fn style_for(family: &str) -> CharStyle {
    CharStyle {
        family: family.to_string(),
        style: "Regular".to_string(),
        size_pt: 24.0,
        ligatures: Some(false),
        caps: Caps::None,
        ..CharStyle::default()
    }
}

#[test]
fn caps_ligatures_and_opentype_do_not_rewrite_story_text() {
    let text = "official";
    let base = CharStyle {
        family: "Pretendard".to_string(),
        style: "Regular".to_string(),
        ..CharStyle::default()
    };
    let mut story = Story {
        text: text.to_string(),
        chars: vec![Run { range: TextRange::new(0, text.len()), style: base }],
        ..Story::default()
    };
    let before = story.text.clone();
    story.patch_char(
        TextRange::new(0, text.len()),
        &CharPatch {
            ligatures: Some(Some(true)),
            caps: Some(Caps::AllCaps),
            opentype: Some(vec![("ss01".to_string(), 1), ("smcp".to_string(), 1)]),
            ..CharPatch::default()
        },
    );
    assert_eq!(story.text, before);
    assert_eq!(story.text.as_bytes(), before.as_bytes());
    let style = &story.chars[0].style;
    assert_eq!(style.ligatures, Some(true));
    assert_eq!(style.caps, Caps::AllCaps);
    assert!(style.opentype.iter().any(|(tag, value)| tag == "ss01" && *value == 1));
    let options = ShapeOptions::from_style(style);
    assert_eq!(options.feature("liga"), Some(1));
    assert_eq!(options.feature("clig"), Some(1));
    assert_eq!(options.feature("ss01"), Some(1));
    assert_eq!(options.feature("smcp"), Some(1));
}

#[test]
fn shaped_output_differs_for_liga_or_caps_when_font_supports_it() {
    let lib = FontLibrary::system();
    let text = "official";
    let families = prefers_family(&lib);
    if families.is_empty() {
        eprintln!("skip: no installed font families available for liga/caps shaping");
        return;
    }
    for family in &families {
        let base = style_for(family);
        let default_shape = fingerprint(&base, text, &lib);
        if default_shape.1 == 0 {
            continue;
        }
        let mut liga = base.clone();
        liga.ligatures = Some(true);
        let mut caps = base.clone();
        caps.caps = Caps::SmallCaps;
        let liga_shape = fingerprint(&liga, text, &lib);
        let caps_shape = fingerprint(&caps, text, &lib);
        if liga_shape != default_shape || caps_shape != default_shape {
            let options = ShapeOptions::from_style(&liga);
            assert_eq!(options.feature("liga"), Some(1));
            assert_eq!(ShapeOptions::from_style(&caps).feature("smcp"), Some(1));
            return;
        }
    }
    eprintln!("skip: no installed face produced a different shaped output for liga or caps (smcp) on {:?}", text);
}

#[test]
fn allcaps_shapes_like_uppercase_without_rewriting_story() {
    let lib = FontLibrary::system();
    let families = prefers_family(&lib);
    if families.is_empty() {
        eprintln!("skip: no installed font families available for AllCaps shaping");
        return;
    }
    let text = "official";
    let upper = "OFFICIAL";
    for family in &families {
        let mut none = style_for(family);
        none.caps = Caps::None;
        let mut allcaps = style_for(family);
        allcaps.caps = Caps::AllCaps;
        let mut small = style_for(family);
        small.caps = Caps::SmallCaps;
        let expected = shape_styled(&lib, upper, &none);
        if expected.glyphs.is_empty() {
            continue;
        }
        let shaped = shape_styled(&lib, text, &allcaps);
        let expected_ids: Vec<u16> = expected.glyphs.iter().map(|g| g.gid).collect();
        let shaped_ids: Vec<u16> = shaped.glyphs.iter().map(|g| g.gid).collect();
        assert_eq!(shaped_ids, expected_ids);
        let story = Story {
            text: text.to_string(),
            chars: vec![Run { range: TextRange::new(0, text.len()), style: allcaps.clone() }],
            ..Story::default()
        };
        assert_eq!(story.text, text);
        assert_eq!(story.text.as_bytes(), text.as_bytes());
        let lines = layout_story(&lib, &story, None);
        assert!(!lines.is_empty());
        let laid: Vec<u16> = lines.iter().flat_map(|line| line.glyphs.iter().map(|g| g.gid)).collect();
        assert_eq!(laid, expected_ids);
        assert_eq!(ShapeOptions::from_style(&allcaps).feature("smcp"), None);
        assert_eq!(ShapeOptions::from_style(&allcaps).feature("case"), None);
        assert_eq!(ShapeOptions::from_style(&small).feature("smcp"), Some(1));
        return;
    }
    eprintln!("skip: no installed face produced glyphs for AllCaps/OFFICIAL on {:?}", text);
}


#[test]
fn allcaps_clusters_point_into_the_original_story_bytes() {
    let lib = FontLibrary::system();
    let families = prefers_family(&lib);
    let text = "\u{17f}tra\u{df}e \u{131}i \u{d55c}a";
    let boundaries: Vec<usize> = text.char_indices().map(|(b, _)| b).chain(std::iter::once(text.len())).collect();
    for family in &families {
        let mut style = style_for(family);
        style.caps = Caps::AllCaps;
        let run = shape_styled(&lib, text, &style);
        if run.glyphs.is_empty() {
            continue;
        }
        let mut last = 0usize;
        for glyph in &run.glyphs {
            assert!(boundaries.contains(&glyph.cluster), "{} not a boundary in {:?}", glyph.cluster, text);
            assert!(glyph.cluster >= last);
            last = glyph.cluster;
        }
        let story = Story {
            text: text.to_string(),
            chars: vec![Run { range: TextRange::new(0, text.len()), style: style.clone() }],
            ..Story::default()
        };
        let lines = layout_story(&lib, &story, None);
        let count = text.chars().count();
        assert!(lines.iter().flat_map(|l| l.glyphs.iter()).all(|g| g.cluster <= count));
        return;
    }
    eprintln!("skip: no installed face shaped {:?}", text);
}
