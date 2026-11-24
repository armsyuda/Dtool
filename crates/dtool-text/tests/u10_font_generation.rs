use dtool_text::fonts::font_generation;
use dtool_text::FontLibrary;

#[test]
fn u10_loading_a_fixture_library_raises_font_generation() {
    let arial = std::path::Path::new("C:\\Windows\\Fonts\\arial.ttf");
    if !arial.exists() {
        eprintln!("skip: arial.ttf is not installed");
        return;
    }
    let before = font_generation();
    FontLibrary::from_fixtures(&[arial]).expect("fixture");
    let after = font_generation();
    assert!(after > before, "{before} {after}");
    let _ = FontLibrary::shared();
    assert!(font_generation() >= after);
}