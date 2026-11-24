use dtool_text::char_style::{apply_case, Caps, CaseOp, Position};
use dtool_text::shape::{shape_styled, GlyphTransform, ShapeOptions};
use dtool_text::{layout_story, CharPatch, CharPatchField, CharStyle, FontLibrary, Run, Story, TextRange};

fn base_style(family: &str) -> CharStyle {
    CharStyle { family: family.to_owned(), style: "Regular".to_owned(), size_pt: 24.0, ..CharStyle::default() }
}

fn story(text: &str, style: CharStyle) -> Story {
    Story { text: text.to_owned(), chars: vec![Run { range: TextRange::new(0, text.chars().count()), style }], ..Story::default() }
}

fn style_at(story: &Story, index: usize) -> CharStyle {
    story.chars.iter().find(|run| run.range.start <= index && index < run.range.end).expect("index not covered").style.clone()
}

fn usable_family(lib: &FontLibrary) -> Option<String> {
    let have: Vec<String> = lib.families().into_iter().map(str::to_owned).collect();
    ["Arial", "Segoe UI", "Pretendard", "Noto Sans", "Liberation Sans", "DejaVu Sans"]
        .iter()
        .find_map(|name| have.iter().find(|h| h.eq_ignore_ascii_case(name)).cloned())
        .or_else(|| have.into_iter().find(|family| lib.resolve(family, "Regular").is_some()))
}

fn line_width(lib: &FontLibrary, text: &str, style: CharStyle) -> f32 {
    layout_story(lib, &story(text, style), None).iter().map(|line| line.width).fold(0.0, f32::max)
}

#[test]
fn font_family_and_size_patch_keep_other_runs() {
    let original = base_style("Pretendard");
    let mut story = story("ABCDEF", original.clone());
    story.patch_char(TextRange::new(2, 4), &CharPatch { family: Some("Arial".to_owned()), size_pt: Some(36.0), ..CharPatch::default() });
    assert_eq!(story.text, "ABCDEF");
    for index in [0, 1, 4, 5] {
        assert_eq!(style_at(&story, index), original);
    }
    let mut expected = original.clone();
    expected.family = "Arial".to_owned();
    expected.size_pt = 36.0;
    assert_eq!(style_at(&story, 2), expected);
    assert_eq!(style_at(&story, 3), expected);
    assert!(matches!(story.query_char(TextRange::new(2, 4)).family, CharPatchField::Same(ref value) if value == "Arial"));
    assert!(matches!(story.query_char(TextRange::new(2, 4)).size_pt, CharPatchField::Same(value) if value == 36.0));
    assert!(matches!(story.query_char(TextRange::new(0, 6)).family, CharPatchField::Mixed));
    assert!(matches!(story.query_char(TextRange::new(0, 6)).size_pt, CharPatchField::Mixed));
    for index in 0..6 {
        assert_eq!(story.chars.iter().filter(|run| run.range.start <= index && index < run.range.end).count(), 1);
    }
}

#[test]
fn scale_skew_position_values_reach_layout() {
    let mut style = base_style("Pretendard");
    style.h_scale = 2.0;
    style.v_scale = 0.5;
    style.shear_deg = 15.0;
    style.baseline_pt = 3.0;
    let transform = GlyphTransform::from_style(&style);
    assert_eq!(transform.h_scale, 2.0);
    assert_eq!(transform.v_scale, 0.5);
    assert_eq!(transform.shear_deg, 15.0);
    assert_eq!(transform.baseline_shift, 3.0);
    let mut superscript = base_style("Pretendard");
    superscript.position = Position::Superscript;
    let sup = GlyphTransform::from_style(&superscript);
    assert!(sup.v_scale < 1.0 && sup.h_scale < 1.0, "superscript must shrink glyphs: {sup:?}");
    assert!(sup.baseline_shift > 0.0, "superscript must raise the baseline: {sup:?}");
    let mut subscript = base_style("Pretendard");
    subscript.position = Position::Subscript;
    assert!(GlyphTransform::from_style(&subscript).baseline_shift < 0.0);
    let lib = FontLibrary::system();
    let Some(family) = usable_family(&lib) else { return };
    let mut plain = base_style(&family);
    plain.ligatures = Some(false);
    let mut wide = plain.clone();
    wide.h_scale = 2.0;
    let run = shape_styled(&lib, "HHHH", &wide);
    assert_eq!(run.transform.h_scale, 2.0);
    let narrow_width = line_width(&lib, "HHHH", plain);
    let wide_width = line_width(&lib, "HHHH", wide);
    assert!(narrow_width > 0.0);
    assert!((wide_width - 2.0 * narrow_width).abs() < narrow_width * 0.05, "h_scale 2 must double line width: {narrow_width} -> {wide_width}");
    let mut raised = base_style(&family);
    raised.ligatures = Some(false);
    raised.baseline_pt = 4.0;
    let lifted = first_offset_y(&lib, raised);
    assert!((lifted + 4.0).abs() < 0.01, "baseline shift 4pt must raise glyphs by 4pt: {lifted}");
    let mut sup = base_style(&family);
    sup.ligatures = Some(false);
    sup.position = Position::Superscript;
    if !dtool_text::char_style::has_opentype_sups(&sup) {
        let expected = GlyphTransform::from_style(&sup);
        assert!(first_offset_y(&lib, sup.clone()) < 0.0, "superscript must sit above the baseline");
        let sup_width = line_width(&lib, "HHHH", sup);
        let mut normal = base_style(&family);
        normal.ligatures = Some(false);
        let normal_width = line_width(&lib, "HHHH", normal);
        assert!((sup_width - normal_width * expected.h_scale).abs() < normal_width * 0.05, "superscript scales once: {normal_width} -> {sup_width}");
    }
    let mut skew = base_style(&family);
    skew.ligatures = Some(false);
    skew.shear_deg = 20.0;
    let mut upright = skew.clone();
    upright.shear_deg = 0.0;
    assert!((line_width(&lib, "HHHH", skew) - line_width(&lib, "HHHH", upright)).abs() < 0.01, "skew must not change advances");
}

fn first_offset_y(lib: &FontLibrary, style: CharStyle) -> f32 {
    layout_story(lib, &story("HHHH", style), None).first().and_then(|line| line.glyphs.first()).map(|glyph| glyph.offset[1]).expect("glyph laid out")
}

#[test]
fn capitalisation_style_vs_case_change() {
    let mut story = story("hello world", base_style("Pretendard"));
    story.patch_char(TextRange::new(0, 5), &CharPatch { caps: Some(Caps::AllCaps), ..CharPatch::default() });
    assert_eq!(story.text, "hello world");
    assert_eq!(style_at(&story, 0).caps, Caps::AllCaps);
    assert_eq!(style_at(&story, 6).caps, Caps::None);
    assert_eq!(apply_case("hello world", CaseOp::Upper, "en"), "HELLO WORLD");
    assert_eq!(apply_case("Hello World", CaseOp::Lower, "en"), "hello world");
    assert_eq!(apply_case("hello WORLD", CaseOp::Toggle, "en"), "HELLO world");
    assert_eq!(apply_case("hello world", CaseOp::Title, "en"), "Hello World");
    assert_eq!(apply_case("hello. world", CaseOp::Sentence, "en"), "Hello. World");
    let mut small = base_style("Pretendard");
    small.caps = Caps::SmallCaps;
    assert_eq!(ShapeOptions::from_style(&small).feature("smcp"), Some(1));
    let lib = FontLibrary::system();
    let Some(family) = usable_family(&lib) else { return };
    let mut caps = base_style(&family);
    caps.caps = Caps::AllCaps;
    let shown: Vec<u16> = shape_styled(&lib, "hello", &caps).glyphs.iter().map(|g| g.gid).collect();
    let upper: Vec<u16> = shape_styled(&lib, "HELLO", &base_style(&family)).glyphs.iter().map(|g| g.gid).collect();
    let lower: Vec<u16> = shape_styled(&lib, "hello", &base_style(&family)).glyphs.iter().map(|g| g.gid).collect();
    assert_eq!(shown, upper, "all caps must draw capital glyphs");
    assert_ne!(shown, lower);
    let clusters: Vec<usize> = shape_styled(&lib, "hello", &caps).glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 2, 3, 4], "all caps keeps source clusters");
}

#[test]
fn ligatures_and_language() {
    let mut story = story("office", base_style("Pretendard"));
    story.patch_char(TextRange::new(0, 6), &CharPatch { ligatures: Some(Some(false)), language: Some("tr".to_owned()), ..CharPatch::default() });
    let style = style_at(&story, 0);
    assert_eq!(style.ligatures, Some(false));
    assert_eq!(style.language, "tr");
    assert_eq!(story.text, "office");
    let options = ShapeOptions::from_style(&style);
    assert_eq!(options.feature("liga"), Some(0));
    assert_eq!(options.feature("clig"), Some(0));
    assert_eq!(options.language.as_deref(), Some("tr"));
    let mut on = style.clone();
    on.ligatures = Some(true);
    assert_eq!(ShapeOptions::from_style(&on).feature("liga"), Some(1));
    on.opentype = vec![("ss01".to_owned(), 1)];
    assert_eq!(ShapeOptions::from_style(&on).feature("ss01"), Some(1));
    let lib = FontLibrary::system();
    let Some(family) = usable_family(&lib) else { return };
    let mut shaped = style.clone();
    shaped.family = family;
    let run = shape_styled(&lib, "office", &shaped);
    assert_eq!(run.language.as_deref(), Some("tr"));
    assert!(run.features.iter().any(|(tag, value)| tag == "liga" && *value == 0));
}

#[test]
fn font_style_change() {
    let original = base_style("Pretendard");
    let mut story = story("ABCD", original.clone());
    story.patch_char(TextRange::new(1, 3), &CharPatch { style: Some("Bold".to_owned()), ..CharPatch::default() });
    assert_eq!(style_at(&story, 0), original);
    assert_eq!(style_at(&story, 3), original);
    assert_eq!(style_at(&story, 1).style, "Bold");
    assert_eq!(style_at(&story, 1).family, "Pretendard");
    assert!(matches!(story.query_char(TextRange::new(1, 3)).style, CharPatchField::Same(ref value) if value == "Bold"));
    assert!(matches!(story.query_char(TextRange::new(0, 4)).style, CharPatchField::Mixed));
    let lib = FontLibrary::system();
    let pair = lib.families().into_iter().map(str::to_owned).find_map(|family| {
        let faces = lib.faces(&family);
        let regular = faces.iter().find(|face| face.style.eq_ignore_ascii_case("Regular"))?.id;
        let bold = faces.iter().find(|face| face.style.eq_ignore_ascii_case("Bold"))?.id;
        Some((family, regular, bold))
    });
    let Some((family, regular, bold)) = pair else { return };
    assert_eq!(lib.resolve(&family, "Regular"), Some(regular));
    assert_eq!(lib.resolve(&family, "Bold"), Some(bold));
    let regular_run = shape_styled(&lib, "ABCD", &base_style(&family));
    let mut bold_style = base_style(&family);
    bold_style.style = "Bold".to_owned();
    let bold_run = shape_styled(&lib, "ABCD", &bold_style);
    assert!(!regular_run.glyphs.is_empty() && !bold_run.glyphs.is_empty());
    assert_ne!(regular_run.glyphs[0].font, bold_run.glyphs[0].font, "bold style must shape with the bold face");
}