use dtool_core::colour::ColourModel;
use dtool_ui::prefs_dialog::profile_names;

#[test]
fn cmyk_combo_lists_store_profiles_only() {
    let cmyk = profile_names(ColourModel::Cmyk);
    let rgb = profile_names(ColourModel::Rgb);
    assert!(cmyk.iter().any(|name| name == "FOGRA39L Coated"));
    assert!(rgb.iter().any(|name| name == "sRGB IEC61966-2.1"));
    assert!(cmyk.iter().all(|name| !rgb.contains(name)));
    assert!(profile_names(ColourModel::Lab).iter().any(|name| name == "LAB D50"));
}
