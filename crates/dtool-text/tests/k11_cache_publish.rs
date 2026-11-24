use dtool_text::fonts::font_generation;
use dtool_text::FontLibrary;

#[test]
fn k11_new_library_publishes_generation_to_core() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dtool-ui/assets/fonts/inter/Inter-Regular.ttf");
    let before = dtool_core::text_lines::font_generation();
    FontLibrary::from_fixtures(&[path]).expect("fixture");
    let core = dtool_core::text_lines::font_generation();
    assert!(core > before, "{before} {core}");
    assert!(core <= font_generation());
}