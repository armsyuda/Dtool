use dtool_core::colour::{ColourPrefs, ColourServices, ProfileStore};
use dtool_core::document::{Document, Units};

#[test]
fn default_working_rgb_is_a_builtin_profile() {
    let store = ProfileStore::builtin();
    let prefs = ColourPrefs::default();
    assert_eq!(prefs.rgb, ProfileStore::SRGB);
    assert!(store.find(&prefs.rgb).is_some());
    assert!(store.find(&prefs.rgb32).is_some());
}

#[test]
fn short_srgb_name_resolves_to_builtin() {
    let store = ProfileStore::builtin();
    assert_eq!(store.find("sRGB").map(|profile| profile.name.as_str()), Some(ProfileStore::SRGB));
    assert_eq!(store.canonical("sRGB"), ProfileStore::SRGB);
    assert_eq!(store.canonical("Missing"), "Missing");
    assert!(store.find("Missing").is_none());
}

#[test]
fn old_prefs_with_short_name_still_match_documents() {
    let mut value = serde_json::to_value(ColourPrefs::default()).unwrap();
    value["rgb"] = serde_json::json!("sRGB");
    let preferences: ColourPrefs = serde_json::from_value(value).unwrap();
    let services = ColourServices { profiles: ProfileStore::builtin(), preferences };
    let mut document = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    assert_eq!(document.colour.profile, ProfileStore::SRGB);
    assert!(dtool_core::colour::convert::open_policy(&mut document, &services).is_none());
    document.colour.profile = "sRGB".to_string();
    assert!(dtool_core::colour::convert::open_policy(&mut document, &services).is_none());
}