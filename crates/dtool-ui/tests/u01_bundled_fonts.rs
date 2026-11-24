use dtool_ui::fonts::{ui_font_definitions, INTER, INTER_ID, MALGUN_ID, PRETENDARD, PRETENDARD_ID};

#[test]
fn inter_then_pretendard_then_malgun() {
    let fonts = ui_font_definitions(Some(INTER.to_vec()));
    let proportional = &fonts.families[&egui::FontFamily::Proportional];
    assert_eq!(&proportional[..3], &[INTER_ID.to_owned(), PRETENDARD_ID.to_owned(), MALGUN_ID.to_owned()]);
    assert!(fonts.font_data.contains_key(PRETENDARD_ID));
    assert!(PRETENDARD.len() > 1_000_000 && INTER.len() > 100_000);
    assert_eq!(dtool_design::font_family("ui"), "Pretendard");
    assert_eq!(dtool_design::font_family("ui_latin"), "Inter");
}

#[test]
fn bundled_fonts_work_without_malgun() {
    let fonts = ui_font_definitions(None);
    let proportional = &fonts.families[&egui::FontFamily::Proportional];
    assert_eq!(&proportional[..2], &[INTER_ID.to_owned(), PRETENDARD_ID.to_owned()]);
    assert!(!fonts.font_data.contains_key(MALGUN_ID));
}
