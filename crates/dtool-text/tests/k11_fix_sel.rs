use dtool_text::fonts::{fallback_chain, fallback_families, family_stack_for, is_hangul, parse_style, style_name, FontId, FontLibrary};
use dtool_text::shape::{notdef_scalars, shape_run};
use std::path::PathBuf;

fn asset(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("dtool-ui").join("assets").join("fonts").join(rel)
}

fn with_weight(data: &[u8], weight: u16) -> Vec<u8> {
    let mut out = data.to_vec();
    let tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    for index in 0..tables {
        let record = 12 + index * 16;
        if &data[record..record + 4] == b"OS/2" {
            let offset = u32::from_be_bytes([data[record + 8], data[record + 9], data[record + 10], data[record + 11]]) as usize;
            out[offset + 4..offset + 6].copy_from_slice(&weight.to_be_bytes());
            return out;
        }
    }
    panic!("no OS/2 table");
}

fn weighted_inter(tag: &str) -> (FontLibrary, Vec<PathBuf>) {
    let regular = asset("inter/Inter-Regular.ttf");
    let data = std::fs::read(&regular).expect("Inter fixture");
    let dir = std::env::temp_dir().join(format!("dtool_k11_sel_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut paths = vec![regular];
    for weight in [300u16, 500, 600] {
        let path = dir.join(format!("Inter-{weight}.ttf"));
        std::fs::write(&path, with_weight(&data, weight)).unwrap();
        paths.push(path);
    }
    let lib = FontLibrary::from_fixtures(&paths).expect("fixtures");
    (lib, paths)
}

#[test]
fn k11_faces_with_close_weights_stay_distinct() {
    let (lib, _paths) = weighted_inter("distinct");
    let mut styles: Vec<String> = lib.faces("Inter").iter().map(|face| face.style.clone()).collect();
    styles.sort();
    assert_eq!(styles, vec!["Light", "Medium", "Regular", "SemiBold"]);
    let ids: Vec<FontId> = ["Light", "Regular", "Medium", "SemiBold"].iter().map(|style| lib.resolve("Inter", style).expect(style)).collect();
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(lib.face(ids[2]).unwrap().weight, 500);
    assert_eq!(lib.resolve("Inter", "Semi Bold"), Some(ids[3]));
    assert_eq!(lib.resolve("Inter", "600"), Some(ids[3]));
    assert_eq!(lib.resolve("Inter", "Bold"), None);
    assert!(ids.iter().all(|id| lib.has_glyph(*id, 'A')));
}

#[test]
fn k11_shape_uses_requested_face() {
    let (lib, _paths) = weighted_inter("shape");
    for style in ["Light", "Medium", "SemiBold", "Regular"] {
        let id = lib.resolve("Inter", style).unwrap();
        let run = shape_run(&lib, "Hi", "Inter", style, 12.0);
        assert!(!run.glyphs.is_empty());
        assert!(run.glyphs.iter().all(|glyph| glyph.font == id), "{style}: {:?}", run.glyphs.iter().map(|g| g.font).collect::<Vec<_>>());
    }
}

#[test]
fn k11_style_names_and_parse() {
    assert_eq!(style_name(300, false), "Light");
    assert_eq!(style_name(500, true), "Medium Italic");
    assert_eq!(style_name(400, true), "Italic");
    assert_eq!(style_name(700, false), "Bold");
    assert_eq!(parse_style("SemiBold Italic"), Some((600, true)));
    assert_eq!(parse_style("bold"), Some((700, false)));
    assert_eq!(parse_style("wobbly"), None);
}

fn mixed() -> FontLibrary {
    FontLibrary::from_fixtures(&[asset("inter/Inter-Regular.ttf"), asset("pretendard/Pretendard-Regular.ttf")]).expect("fixtures")
}

#[test]
fn k11_fallback_run_keeps_fallback_font() {
    let lib = mixed();
    let inter = lib.resolve("Inter", "Regular").unwrap();
    let pretendard = lib.resolve("Pretendard", "Regular").expect("Pretendard family");
    assert!(family_stack_for(&lib, inter, "A한").contains("\"Pretendard\""));
    let run = shape_run(&lib, "A한", "Inter", "Regular", 12.0);
    let latin: Vec<_> = run.glyphs.iter().filter(|g| g.cluster == 0).map(|g| g.font).collect();
    let hangul: Vec<_> = run.glyphs.iter().filter(|g| g.cluster == 1).map(|g| g.font).collect();
    assert!(!latin.is_empty() && latin.iter().all(|f| *f == inter), "{latin:?}");
    assert!(!hangul.is_empty() && hangul.iter().all(|f| *f == pretendard), "{hangul:?}");
    assert!(notdef_scalars(&lib, "A한", &run).is_empty());
}

#[test]
fn k11_missing_glyph_reported_notdef() {
    let lib = mixed();
    let text = "\u{10fffd}A";
    let run = shape_run(&lib, text, "Inter", "Regular", 12.0);
    assert_eq!(notdef_scalars(&lib, text, &run), vec![0]);
}

#[test]
fn k11_hangul_jamo_blocks_use_fallback_chain() {
    let lib = mixed();
    let inter = lib.resolve("Inter", "Regular").unwrap();
    let pretendard = lib.resolve("Pretendard", "Regular").unwrap();
    for ch in ['\u{1100}', '\u{3131}', '\u{318e}', '\u{a960}', '\u{a97c}', '\u{d7b0}', '\u{d7fb}', '한'] {
        assert!(is_hangul(ch), "{ch:?}");
        let chain = fallback_chain(&lib, inter, ch);
        assert_eq!(chain[0], inter);
        assert!(chain.contains(&pretendard), "{ch:?} {chain:?}");
        let families = fallback_families(ch);
        assert!(families.contains(&"Malgun Gothic") && families.contains(&"Noto Sans CJK KR") && families.contains(&"Apple SD Gothic Neo"));
    }
    assert!(!fallback_families('A').contains(&"Malgun Gothic"));
    if lib.has_glyph(pretendard, '\u{3131}') {
        assert_eq!(fallback_chain(&lib, inter, '\u{3131}')[1], pretendard);
    }
}