use dtool_ui::menus::vector_warp::PRESET_CHOICES;

#[test]
fn every_warp_preset_choice_has_a_ko_label() {
    assert_eq!(PRESET_CHOICES.len(), 15);
    for (_, key) in PRESET_CHOICES {
        assert!(dtool_strings::has(key), "{key}");
    }
}