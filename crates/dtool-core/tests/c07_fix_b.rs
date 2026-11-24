use dtool_core::colour::{ColourFormat, ColourModel, ColourPrefs, ProfileStore};
use dtool_core::document::{Document, Units};
use dtool_core::paint::{Colour, Paint};
use dtool_core::prefs::Prefs;
use dtool_core::{Command, Object, Session};
use serde_json::json;

fn swop_file_present() -> bool {
    ProfileStore::asset_dirs().iter().any(|dir| dir.join(ProfileStore::SWOP_FILE).is_file())
}

fn red_doc() -> Session {
    let mut document = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    document.create_object(Object { name: "a".into(), size: [10.0, 10.0], fill: Paint::Solid { colour: Colour::new(200, 40, 40, 255) }, ..Object::default() });
    Session::new(document)
}

#[test]
fn c07_default_cmyk_is_fogra39l() {
    assert_eq!(ColourPrefs::default().cmyk, ProfileStore::FOGRA39L);
    assert_eq!(Prefs::default().cmyk_profile, ProfileStore::FOGRA39L);
    assert_eq!(ColourPrefs::from(&Prefs::default()).cmyk, ProfileStore::FOGRA39L);
    let mut s = red_doc();
    let reply = s.run(&Command::with_params("color.profile.convert", json!({ "format": ColourFormat::CMYK_8.to_string() }))).expect("convert");
    assert_eq!(reply["profile"], ProfileStore::FOGRA39L, "{reply}");
    assert!(reply.get("profile_fallback").is_none());
    assert!(matches!(&s.document.objects[0].fill, Paint::Solid { colour } if colour.model == ColourModel::Cmyk));
}

#[test]
fn c07_swop_is_never_substituted() {
    let mut store = ProfileStore::default();
    store.insert(ProfileStore::FOGRA39L.into(), ProfileStore::builtin().find(ProfileStore::FOGRA39L).unwrap().bytes.to_vec()).unwrap();
    assert!(store.find(ProfileStore::SWOP_V2).is_none());
    if !swop_file_present() {
        assert!(ProfileStore::builtin().find(ProfileStore::SWOP_V2).is_none());
        let mut s = red_doc();
        let error = s.run(&Command::with_params("color.profile.convert", json!({ "format": ColourFormat::CMYK_8.to_string(), "profile": ProfileStore::SWOP_V2 }))).unwrap_err();
        assert_eq!(error.code, "PROFILE_NOT_FOUND");
    }
}

#[test]
fn c07_swop_selectable_when_bundled() {
    if !swop_file_present() {
        eprintln!("skip: {} not present in {:?}", ProfileStore::SWOP_FILE, ProfileStore::asset_dirs());
        return;
    }
    let store = ProfileStore::builtin();
    let swop = store.find(ProfileStore::SWOP_V2).expect("SWOP builtin");
    assert_eq!(swop.model, ColourModel::Cmyk);
    let cmyk = store.list(ColourModel::Cmyk);
    assert!(cmyk.contains(&ProfileStore::SWOP_V2) && cmyk.contains(&ProfileStore::FOGRA39L), "{cmyk:?}");
    let mut s = red_doc();
    let reply = s.run(&Command::with_params("color.profile.convert", json!({ "format": ColourFormat::CMYK_8.to_string(), "profile": ProfileStore::SWOP_V2 }))).expect("convert");
    assert_eq!(reply["profile"], ProfileStore::SWOP_V2);
    assert_eq!(s.document.colour.profile, ProfileStore::SWOP_V2);
}

#[test]
fn c07_zoom_actions_point_at_real_view_commands() {
    let mut s = Session::new(Document::new([100.0, 100.0], Units::Pt, 72.0, 1));
    s.run(&Command::with_params("ui.tool.set", json!({ "id": "tool.zoom" }))).expect("zoom tool");
    let query = s.run(&Command::with_params("ui.context_toolbar.query", json!({}))).expect("query");
    let options = query["options"].as_array().expect("options");
    let command = |id: &str| options.iter().find(|option| option["id"] == id).expect("option")["command"].clone();
    assert_eq!(command("zoom.fit"), "view.zoom.fit");
    assert_eq!(command("zoom.actual"), "view.zoom.actual");
    let catalog = dtool_protocol::catalog::CommandCatalog::new();
    for id in ["zoom.fit", "zoom.actual"] {
        assert!(catalog.resolve(command(id).as_str().unwrap()).is_some(), "{id}");
    }
}