use dtool_core::colour::{ColourPrefs, ColourServices, ProfileStore};
use dtool_core::document::{Document, Units};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use serde_json::{json, Value};

const SRGB: &str = "sRGB IEC61966-2.1";

fn store() -> ProfileStore {
    let mut store = ProfileStore::builtin();
    let bytes = store.find(SRGB).unwrap().bytes.to_vec();
    store.insert("Old RGB".to_string(), bytes).unwrap();
    store
}

fn services(convert_on_open: bool, warn_on_convert: bool) -> ColourServices {
    let preferences = ColourPrefs { rgb: SRGB.to_string(), convert_on_open, warn_on_convert, ..ColourPrefs::default() };
    ColourServices { profiles: store(), preferences }
}

fn old_document() -> Document {
    let mut document = Document::new([200.0, 100.0], Units::Pt, 72.0, 1);
    document.colour.profile = "Old RGB".to_string();
    document.objects.push(dtool_core::object::Object {
        id: dtool_core::object::ObjectId(3),
        fill: dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(200, 40, 10, 255) },
        ..dtool_core::object::Object::default()
    });
    document
}

#[test]
fn keep_on_open_leaves_profile_and_warns() {
    let mut document = old_document();
    let fill = serde_json::to_value(&document.objects[0].fill).unwrap();
    let report = dtool_core::colour::convert::open_policy(&mut document, &services(false, true)).unwrap();
    assert_eq!(report["action"], "kept");
    assert_eq!(report["from"], "Old RGB");
    assert_eq!(report["to"], SRGB);
    assert_eq!(report["warn"], true);
    assert_eq!(report["message_key"], dtool_core::colour::convert::OPEN_MISMATCH);
    assert_eq!(document.colour.profile, "Old RGB");
    assert_eq!(serde_json::to_value(&document.objects[0].fill).unwrap(), fill);
}

#[test]
fn convert_on_open_relabels_and_converts_without_warning() {
    let mut document = old_document();
    let report = dtool_core::colour::convert::open_policy(&mut document, &services(true, false)).unwrap();
    assert_eq!(report["action"], "converted");
    assert_eq!(report["converted"], 1);
    assert_eq!(report["warn"], false);
    assert_eq!(report["message_key"], Value::Null);
    assert_eq!(document.colour.profile, SRGB);
}

#[test]
fn convert_on_open_warns_when_flag_is_set() {
    let mut document = old_document();
    let report = dtool_core::colour::convert::open_policy(&mut document, &services(true, true)).unwrap();
    assert_eq!(report["action"], "converted");
    assert_eq!(report["message_key"], dtool_core::colour::convert::OPEN_CONVERTED);
}

#[test]
fn matching_or_missing_working_profile_does_nothing() {
    let mut document = old_document();
    document.colour.profile = SRGB.to_string();
    assert!(dtool_core::colour::convert::open_policy(&mut document, &services(true, true)).is_none());
    let mut document = old_document();
    let mut missing = services(true, true);
    missing.preferences.rgb = "Missing RGB".to_string();
    let report = dtool_core::colour::convert::open_policy(&mut document, &missing).unwrap();
    assert_eq!(report["action"], "kept");
    assert_eq!(document.colour.profile, "Old RGB");
}

#[test]
fn doc_open_applies_working_space_policy_and_reports_it() {
    {
        let mut global = dtool_core::colour::services().write().unwrap();
        let bytes = global.profiles.find(SRGB).unwrap().bytes.to_vec();
        global.profiles.insert("Old RGB".to_string(), bytes).unwrap();
        global.preferences.rgb = SRGB.to_string();
        global.preferences.convert_on_open = true;
        global.preferences.warn_on_convert = true;
    }
    let path = std::env::temp_dir().join(format!("b33_convert_on_open_{}.dtool", std::process::id()));
    std::fs::write(&path, serde_json::to_string(&old_document()).unwrap()).unwrap();
    let mut workspace = Workspace::default();
    let (id, result) = workspace.run(Scope::active(), Command::with_params("doc.open", json!({ "path": path })));
    let reply = result.unwrap();
    assert!(id.is_some());
    assert_eq!(reply["colour_open"]["action"], "converted");
    assert_eq!(reply["colour_open"]["message_key"], dtool_core::colour::convert::OPEN_CONVERTED);
    let ctx = workspace.get(id.unwrap()).unwrap();
    assert_eq!(ctx.session.document.colour.profile, SRGB);
    assert!(!ctx.is_modified());
}