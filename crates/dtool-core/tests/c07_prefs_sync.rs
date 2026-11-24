use dtool_core::colour::{parse_intent, services, sync_app_prefs, ColourFormat, ColourPrefs, ProfileStore, RenderingIntent};
use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint};
use dtool_core::prefs::Prefs;
use dtool_core::{Command, Object, Session};
use serde_json::json;

#[test]
fn c07_prefs_sync_drives_later_colour_ops() {
    let mut prefs = Prefs::default();
    prefs.cmyk_profile = "Missing CMYK For Sync Test".to_string();
    prefs.rendering_intent = "saturation".to_string();
    prefs.black_point_compensation = false;
    prefs.ocio_config = String::new();
    sync_app_prefs(&prefs).expect("sync");
    {
        let services = services().read().unwrap();
        assert_eq!(services.preferences.cmyk, "Missing CMYK For Sync Test");
        assert_eq!(services.preferences.intent, RenderingIntent::Saturation);
        assert!(!services.preferences.bpc);
        assert!(services.preferences.ocio_config.is_none());
    }
    let mut document = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    document.create_object(Object { name: "a".into(), size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(30, 160, 90, 255) }, ..Object::default() });
    let mut s = Session::new(document);
    let error = s.run(&Command::with_params("color.profile.convert", json!({ "format": ColourFormat::CMYK_8.to_string() }))).unwrap_err();
    assert_eq!(error.code, "PROFILE_NOT_FOUND");
    sync_app_prefs(&Prefs::default()).expect("restore");
    assert_eq!(services().read().unwrap().preferences.cmyk, ProfileStore::FOGRA39L);
    let reply = s.run(&Command::with_params("color.profile.convert", json!({ "format": ColourFormat::CMYK_8.to_string() }))).expect("convert");
    assert_eq!(reply["profile"], ProfileStore::FOGRA39L, "{reply}");
}

#[test]
fn c07_prefs_mapping_and_intent_names() {
    let mut prefs = Prefs::default();
    prefs.grey_profile = String::new();
    prefs.rendering_intent = "nonsense".to_string();
    let mapped = ColourPrefs::from(&prefs);
    assert_eq!(mapped.grey, ColourPrefs::default().grey);
    assert_eq!(mapped.intent, ColourPrefs::default().intent);
    assert_eq!(mapped.cmyk, prefs.cmyk_profile);
    assert_eq!(parse_intent("relative_colorimetric"), Some(RenderingIntent::RelativeColorimetric));
    assert_eq!(parse_intent("Absolute Colorimetric"), Some(RenderingIntent::AbsoluteColorimetric));
    assert_eq!(parse_intent("perceptual"), Some(RenderingIntent::Perceptual));
}

#[test]
fn c07_fix14_lab_profile_has_one_name() {
    assert_eq!(ProfileStore::LAB_D50, "LAB D50");
    assert_eq!(ColourPrefs::default().lab, ProfileStore::LAB_D50);
    assert_eq!(Prefs::default().lab_profile, ProfileStore::LAB_D50);
    assert_eq!(ColourPrefs::from(&Prefs::default()).lab, ProfileStore::LAB_D50);
    let store = services().read().unwrap();
    assert_eq!(store.profiles.find("Lab D50").map(|p| p.name.as_str()), Some(ProfileStore::LAB_D50));
    assert!(store.profiles.list(dtool_core::colour::ColourModel::Lab).contains(&ProfileStore::LAB_D50));
}
